//! One upstream attempt, independent of candidate selection and retry policy.
use super::fallback::FallbackCandidate;
use super::forward::forward_fallback;
use super::policy::record_response_health;
use super::{stream, transport};
use crate::domain::protocol::Protocol;
use crate::http::SourceHttpClient;
use crate::infra::{db, events, health, observability, secrets};
use crate::state::AppState;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Response};
use std::time::Instant;

pub(crate) struct AttemptContext<'a> {
    pub secrets: &'a secrets::SecretResolver,
    pub events: &'a events::EventRepository,
    pub health: &'a health::HealthRegistry,
    pub http: &'a SourceHttpClient,
    pub headers: &'a HeaderMap,
    pub body: &'a Bytes,
    pub model: &'a str,
    pub protocol: Protocol,
    pub request_id: &'a str,
    pub stream_config: &'a stream::StreamConfig,
    pub started: Instant,
}

pub(crate) struct AttemptResult {
    pub result: Result<Response<Body>, transport::TransportError>,
    pub usage: db::UsageAttempt,
    pub request_body: Bytes,
}

impl<'a> AttemptContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        state: &'a AppState,
        headers: &'a HeaderMap,
        body: &'a Bytes,
        model: &'a str,
        protocol: Protocol,
        request_id: &'a str,
        stream_config: &'a stream::StreamConfig,
        started: Instant,
    ) -> Self {
        Self {
            secrets: &state.secrets,
            events: &state.events,
            health: &state.health,
            http: &state.http,
            headers,
            body,
            model,
            protocol,
            request_id,
            stream_config,
            started,
        }
    }

    #[tracing::instrument(name = "gateway.forward", skip_all, fields(
        source_id = %candidate.source_id,
        account_id = %candidate.account.id,
        upstream_model = %candidate.upstream_model,
        attempt_no,
    ))]
    pub async fn execute(
        &self,
        candidate: &FallbackCandidate<'_>,
        attempt_no: usize,
        is_fallback: bool,
    ) -> AttemptResult {
        let prepared =
            transport::prepare_model_request(self.body, self.model, &candidate.upstream_model);
        let request_body = prepared.body.clone();
        let started = Instant::now();
        let result = forward_fallback(
            self.secrets,
            self.events,
            self.http,
            candidate.provider,
            candidate.account,
            &candidate.source_id,
            self.request_id,
            candidate.protocol_upstream,
            &candidate.mode,
            candidate.upstream_endpoint.as_deref(),
            self.headers,
            prepared.body,
            self.stream_config,
            self.started,
        )
        .await;
        // Attempt latency ends with upstream forwarding. Health persistence
        // and request settlement belong to the logical request's duration.
        let latency_ms = started.elapsed().as_millis() as i64;
        let status = result.as_ref().ok().map(|response| response.status());
        let (status_code, success) = if let Some(status) = status {
            record_response_health(
                self.health,
                &candidate.source_id,
                &candidate.account.id,
                status,
            )
            .await;
            (i32::from(status.as_u16()), status.is_success())
        } else {
            let status_code = result
                .as_ref()
                .expect_err("attempt has a response or transport error")
                .status_code();
            self.health
                .mark_failure_with_details(
                    &candidate.account.id,
                    "passive",
                    Some("upstream_transport_error"),
                    Some("upstream request failed"),
                )
                .await;
            (status_code, false)
        };
        observability::record_attempt(
            &self.protocol.to_string(),
            &candidate.source_id,
            &candidate.account.id,
            status_code as u16,
            is_fallback,
        );
        AttemptResult {
            result,
            request_body,
            usage: db::UsageAttempt {
                attempt_no: attempt_no as i32,
                provider_id: candidate.provider_id.clone(),
                source_id: candidate.source_id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(prepared.upstream_model_id),
                status_code,
                success,
                latency_ms,
            },
        }
    }
}
