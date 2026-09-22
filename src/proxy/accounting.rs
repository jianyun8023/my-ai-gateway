use super::settlement::SettlementPermit;
use super::{stream, usage};
use crate::domain::protocol::Protocol;
use crate::infra::{db, health, observability};
use axum::body::{Body, Bytes};
use axum::http::Response;
use std::future::Future;
use std::time::{Duration, Instant};

pub(super) fn is_event_stream(response: &Response<Body>) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn wrap_stream_usage(
    response: Response<Body>,
    database: db::Database,
    permit: SettlementPermit,
    mut event: db::UsageEvent,
    request_body: Bytes,
    mut attempts: Vec<db::UsageAttempt>,
    request_started: Instant,
    health: health::HealthRegistry,
    protocol: Protocol,
    model: &str,
) -> Response<Body> {
    observability::track_stream_start();
    let model = model.to_owned();
    let protocol = protocol.to_string();
    let (parts, body) = response.into_parts();
    let request_id = event.request_id.clone();
    let body = usage::observe_stream_body(body, request_started, &request_id, move |observation| {
        observability::track_stream_end();
        event.latency_ms = request_started.elapsed().as_millis() as i64;
        let account_id = attempts.last().map(|attempt| attempt.account_id.clone());
        let ttft_ms = observation.ttft_ms;
        let upstream_failure = (observation.failed && !observation.termination.is_failure())
            || matches!(
                observation.termination,
                stream::StreamTermination::UpstreamError
                    | stream::StreamTermination::TransportError
                    | stream::StreamTermination::IncompleteStream
                    | stream::StreamTermination::BufferLimitExceeded
            );

        finalize_stream_usage(&mut event, &mut attempts, &request_body, observation);
        observability::record_proxy_request(
            &protocol,
            &model,
            event.status_code as u16,
            request_started,
            true,
        );
        if let Some(ttft_ms) = ttft_ms {
            observability::record_ttft(&protocol, &model, Duration::from_millis(ttft_ms as u64));
        }
        if event.input_tokens > 0 {
            observability::record_tokens(&model, "input", event.input_tokens as u64);
        }
        if event.output_tokens > 0 {
            observability::record_tokens(&model, "output", event.output_tokens as u64);
        }
        permit.spawn(async move {
                if let Err(error) = retry_usage_write(&event.request_id, || {
                    database.insert_usage_with_attempts(&event, &attempts)
                })
                .await
                {
                    tracing::error!(request_id = %event.request_id, %error, "failed to persist streaming usage after retries");
                }
                if upstream_failure {
                    if let Some(account_id) = account_id {
                        health
                            .mark_failure_with_details(
                                &account_id,
                                "passive",
                                Some("upstream_stream_error"),
                                Some("upstream stream failed"),
                            )
                            .await;
                    }
                }
            });
    });
    Response::from_parts(parts, body)
}

async fn retry_usage_write<F, Fut, E>(request_id: &str, mut write: F) -> Result<(), E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: std::fmt::Display,
{
    for attempt in 1..=3 {
        match write().await {
            Ok(()) => return Ok(()),
            Err(error) if attempt < 3 => {
                tracing::warn!(request_id, %error, attempt, "retrying streaming usage write");
                tokio::time::sleep(Duration::from_millis(100 * attempt)).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the third attempt returns")
}

pub(crate) fn finalize_stream_usage(
    event: &mut db::UsageEvent,
    attempts: &mut [db::UsageAttempt],
    request_body: &[u8],
    observation: usage::StreamObservation,
) {
    event.ttft_ms = observation.ttft_ms;
    let termination = if observation.failed && !observation.termination.is_failure() {
        stream::StreamTermination::UpstreamError
    } else {
        observation.termination
    };
    tracing::debug!(
        request_id = %event.request_id,
        termination = termination.code(),
        ttft_ms = ?observation.ttft_ms,
        "stream terminated"
    );
    if termination.is_failure() {
        event.status_code = termination.status_code();
        event.success = false;
        event.error_summary = Some(
            match termination {
                stream::StreamTermination::UpstreamError => "upstream stream error",
                stream::StreamTermination::TransportError => "upstream body transport error",
                stream::StreamTermination::IncompleteStream => {
                    "upstream stream ended without a terminal event"
                }
                stream::StreamTermination::BufferLimitExceeded => {
                    "upstream stream observation limit exceeded"
                }
                stream::StreamTermination::EmptyStream => "upstream stream ended without an event",
                stream::StreamTermination::ClientCancelled => "client disconnected",
                stream::StreamTermination::ConnectionTimeout => "upstream connection timeout",
                stream::StreamTermination::FirstEventTimeout => "first event timeout",
                stream::StreamTermination::IdleTimeout => "upstream idle timeout",
                stream::StreamTermination::TotalTimeout => "stream total timeout",
                stream::StreamTermination::Completed => "",
            }
            .into(),
        );
        if let Some(attempt) = attempts.last_mut() {
            attempt.status_code = termination.status_code();
            attempt.success = false;
        }
    }
    let report = usage::usage_for_stream_observation(
        &event.request_id,
        event.success,
        request_body,
        &observation,
    );
    event.input_tokens = report.input_tokens;
    event.output_tokens = report.output_tokens;
    event.reasoning_tokens = report.reasoning_tokens;
    event.cached_tokens = report.cached_tokens;
    event.cache_read_tokens = report.cache_read_tokens;
    event.cache_creation_tokens = report.cache_creation_tokens;
    event.total_tokens = report.total_tokens;
    event.usage_source = report.source;
}

#[cfg(test)]
mod tests {
    use super::retry_usage_write;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn usage_write_retries_transient_failure() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let result = retry_usage_write("request", move || {
            let call = counter.fetch_add(1, Ordering::Relaxed);
            async move {
                if call == 0 {
                    Err("transient")
                } else {
                    Ok(())
                }
            }
        })
        .await;
        assert_eq!(result, Ok(()));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn usage_write_reports_permanent_failure_after_three_attempts() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let result = retry_usage_write("request", move || {
            counter.fetch_add(1, Ordering::Relaxed);
            async { Err::<(), _>("database unavailable") }
        })
        .await;
        assert_eq!(result, Err("database unavailable"));
        assert_eq!(calls.load(Ordering::Relaxed), 3);
    }
}
