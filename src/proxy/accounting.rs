use super::{stream, usage};
use crate::domain::protocol::Protocol;
use crate::infra::{db, health, observability};
use axum::body::{Body, Bytes};
use axum::http::Response;
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
    let body = usage::observe_stream_body(body, request_started, move |observation| {
        observability::track_stream_end();
        event.latency_ms = request_started.elapsed().as_millis() as i64;
        let account_id = attempts.last().map(|attempt| attempt.account_id.clone());
        let ttft_ms = observation.ttft_ms;
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
        if event.error_summary.as_deref() == Some("upstream stream error") {
            if let Some(account_id) = account_id {
                tokio::spawn(async move {
                    health
                        .mark_failure_with_details(
                            &account_id,
                            "passive",
                            Some("upstream_stream_error"),
                            Some("upstream stream failed"),
                        )
                        .await;
                });
            }
        }
        tokio::spawn(async move {
            if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await {
                tracing::warn!(%error, "failed to persist streaming usage event");
            }
        });
    });
    Response::from_parts(parts, body)
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
    let report = usage::usage_for_sse_response(
        &event.request_id,
        event.success,
        request_body,
        &observation.captured,
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
