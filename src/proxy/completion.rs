//! Request-level usage and settlement shared by both candidate policies.
use super::accounting::{is_event_stream, wrap_stream_usage};
use super::fallback::FallbackCandidate;
use super::service::finish_proxy;
use super::settlement::SettlementPermit;
use super::transport;
use crate::domain::{protocol::Protocol, routing::ResolvedRoute};
use crate::infra::db;
use crate::state::AppState;
use axum::body::{Body, Bytes};
use axum::http::Response;
use std::time::Instant;

pub(super) struct RequestCompletion<'a> {
    pub state: &'a AppState,
    pub route: &'a ResolvedRoute,
    pub request_id: &'a str,
    pub virtual_key_id: Option<i64>,
    pub client_source: String,
    pub model: &'a str,
    pub protocol: Protocol,
    pub is_streamed: bool,
    pub started: Instant,
    pub settlement_permit: Option<SettlementPermit>,
}

pub(super) struct FinalUpstream<'a> {
    pub provider_id: &'a str,
    pub source_id: &'a str,
    pub account_id: &'a str,
    pub upstream_model_id: &'a str,
    pub protocol: Protocol,
    pub mode: &'a str,
    pub degraded: bool,
}

impl<'a> FinalUpstream<'a> {
    pub fn route(route: &'a ResolvedRoute) -> Self {
        Self {
            provider_id: &route.provider_id,
            source_id: &route.source_id,
            account_id: &route.primary_account_id,
            upstream_model_id: &route.upstream_model_id,
            protocol: route.protocol_upstream,
            mode: &route.mode,
            degraded: route.is_degraded(),
        }
    }

    pub fn candidate(candidate: &'a FallbackCandidate<'_>) -> Self {
        Self {
            provider_id: &candidate.provider_id,
            source_id: &candidate.source_id,
            account_id: &candidate.account.id,
            upstream_model_id: &candidate.upstream_model,
            protocol: candidate.protocol_upstream,
            mode: &candidate.mode,
            degraded: !candidate.degraded_features.is_empty(),
        }
    }
}

impl RequestCompletion<'_> {
    fn event(
        &self,
        response: &Response<Body>,
        upstream: FinalUpstream<'_>,
        attempts: &[db::UsageAttempt],
        fallback_reason: Option<String>,
        error_summary: Option<String>,
    ) -> db::UsageEvent {
        let usage = transport::usage_from_response(response);
        db::UsageEvent {
            request_id: self.request_id.to_owned(),
            virtual_key_id: self.virtual_key_id,
            provider_id: upstream.provider_id.to_owned(),
            account_id: upstream.account_id.to_owned(),
            model: self.model.to_owned(),
            logical_model: self.model.to_owned(),
            upstream_model_id: Some(upstream.upstream_model_id.to_owned()),
            source_id: upstream.source_id.to_owned(),
            client_source: self.client_source.clone(),
            protocol_in: self.protocol.to_string(),
            protocol_upstream: upstream.protocol.to_string(),
            mode: upstream.mode.to_owned(),
            status_code: i32::from(response.status().as_u16()),
            success: response.status().is_success(),
            retry_count: attempts.len().saturating_sub(1) as i32,
            latency_ms: self.started.elapsed().as_millis() as i64,
            ttft_ms: None,
            input_tokens: usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
            output_tokens: usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
            reasoning_tokens: usage.as_ref().map(|u| u.reasoning_tokens).unwrap_or(0),
            cached_tokens: usage.as_ref().map(|u| u.cached_tokens).unwrap_or(0),
            cache_read_tokens: usage.as_ref().map(|u| u.cache_read_tokens).unwrap_or(0),
            cache_creation_tokens: usage.as_ref().map(|u| u.cache_creation_tokens).unwrap_or(0),
            total_tokens: usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
            usage_source: usage
                .as_ref()
                .map(|u| u.source.clone())
                .unwrap_or_else(|| "missing".into()),
            degraded: upstream.degraded,
            route_id: Some(self.route.route_id.clone()),
            streamed: self.is_streamed,
            error_summary: error_summary.or_else(|| {
                (!response.status().is_success())
                    .then(|| format!("HTTP {}", response.status().as_u16()))
            }),
            fallback_reason,
        }
    }

    pub async fn finish(
        self,
        response: Response<Body>,
        upstream: FinalUpstream<'_>,
        attempts: Vec<db::UsageAttempt>,
        request_body: Bytes,
        fallback_reason: Option<String>,
        error_summary: Option<String>,
    ) -> Response<Body> {
        if let Some(database) = &self.state.db {
            let event = self.event(
                &response,
                upstream,
                &attempts,
                fallback_reason,
                error_summary,
            );
            if is_event_stream(&response) {
                return wrap_stream_usage(
                    response,
                    database.clone(),
                    self.settlement_permit
                        .expect("database backed requests reserve settlement capacity"),
                    event,
                    request_body,
                    attempts,
                    self.started,
                    self.state.health.clone(),
                    self.protocol,
                    self.model,
                );
            }
            if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await {
                tracing::warn!(%error, "failed to persist usage event");
            }
        }
        finish_proxy(
            self.protocol,
            self.model,
            self.started,
            self.is_streamed,
            response,
        )
    }
}
