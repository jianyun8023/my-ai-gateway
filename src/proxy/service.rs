use super::attribution::client_source_from_headers;
use super::policy::warn_degraded_route;
use super::settlement::AdmissionError;
use super::stream;
use crate::auth::authorized_with_db;
use crate::domain::protocol::Protocol;
use crate::http::response::data_plane_error_response;
use crate::infra::observability;
use crate::state::AppState;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Response, StatusCode};
use serde_json::Value;
use std::time::Instant;
use uuid::Uuid;

#[tracing::instrument(name = "gateway.proxy", skip_all, fields(
    otel.kind = "server",
    request_id,
    protocol = %protocol,
))]
pub(crate) async fn proxy(
    state: AppState,
    headers: HeaderMap,
    body: Bytes,
    protocol: Protocol,
) -> Response<Body> {
    let started = Instant::now();
    let mut stream_config = stream::StreamConfig::from_env();
    let request_id = Uuid::new_v4().to_string();
    tracing::Span::current().record("request_id", request_id.as_str());
    let live = state.snapshot();
    let config = live.config;
    let resolver = live.resolver;
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return finish_proxy(
                protocol,
                "default",
                started,
                false,
                data_plane_error_response(
                    protocol,
                    StatusCode::BAD_REQUEST,
                    "invalid_json",
                    "request body must be valid JSON",
                    &request_id,
                ),
            );
        }
    };
    let model = payload.get("model").and_then(Value::as_str);
    let is_streamed = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let auth_identity = match authorized_with_db(state.db.as_ref(), &headers, model).await {
        Some(identity) => identity,
        None => {
            return finish_proxy(
                protocol,
                model.unwrap_or("default"),
                started,
                is_streamed,
                data_plane_error_response(
                    protocol,
                    StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "invalid or revoked virtual key",
                    &request_id,
                ),
            );
        }
    };
    let virtual_key_id = auth_identity.virtual_key_id();
    // `model` is required by all three northbound protocols.  A missing or
    // empty value is a client validation error (400), not a routing miss
    // (404) — surface-discovery tools such as llmprobe probe endpoints with
    // an empty body and interpret 404 as "endpoint not implemented".
    let Some(model) = model.filter(|value| !value.is_empty()) else {
        return finish_proxy(
            protocol,
            "default",
            started,
            is_streamed,
            data_plane_error_response(
                protocol,
                StatusCode::BAD_REQUEST,
                "missing_required_parameter",
                "missing required field: model",
                &request_id,
            ),
        );
    };
    let route = match resolver.resolve_detailed(protocol, model) {
        Ok(route) => route,
        Err(error) => {
            let status = match error.code.as_str() {
                "route_not_found" => StatusCode::NOT_FOUND,
                "account_disabled" | "account_cooling_down" => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::UNPROCESSABLE_ENTITY,
            };
            return finish_proxy(
                protocol,
                model,
                started,
                is_streamed,
                data_plane_error_response(
                    protocol,
                    status,
                    &error.code,
                    &error.message,
                    &request_id,
                ),
            );
        }
    };
    warn_degraded_route(&request_id, &route);
    if let Some(timeout_ms) = route.request_timeout_ms {
        stream_config.total_timeout = std::time::Duration::from_millis(timeout_ms as u64);
    }
    // Reserve before the first upstream attempt. A provider may return an SSE
    // content type even when the request did not declare `stream: true`.
    let settlement_permit = if state.db.is_some() {
        match state.settlements.try_reserve() {
            Ok(permit) => Some(permit),
            Err(reason) => {
                let code = match reason {
                    AdmissionError::Saturated => "settlement_capacity_exhausted",
                    AdmissionError::Closing => "gateway_shutting_down",
                };
                tracing::warn!(%request_id, code, "stream settlement admission rejected before upstream request");
                return finish_proxy(
                    protocol,
                    model,
                    started,
                    is_streamed,
                    data_plane_error_response(
                        protocol,
                        StatusCode::SERVICE_UNAVAILABLE,
                        code,
                        "stream accounting capacity is unavailable",
                        &request_id,
                    ),
                );
            }
        }
    } else {
        None
    };
    let completion = super::completion::RequestCompletion {
        state: &state,
        route: &route,
        request_id: &request_id,
        virtual_key_id,
        client_source: client_source_from_headers(&headers, Some(&auth_identity)),
        model,
        protocol,
        is_streamed,
        started,
        settlement_permit,
    };
    let context = super::attempt::AttemptContext::new(
        &state,
        &headers,
        &body,
        model,
        protocol,
        &request_id,
        &stream_config,
        started,
    );
    if route.strategy == "ordered_fallback" {
        super::ordered::proxy_ordered(&config, &context, completion).await
    } else {
        super::weighted::proxy_weighted(&config, &context, completion).await
    }
}

pub(super) fn finish_proxy(
    protocol: Protocol,
    model: &str,
    started: Instant,
    is_stream: bool,
    response: Response<Body>,
) -> Response<Body> {
    observability::record_proxy_request(
        &protocol.to_string(),
        model,
        response.status().as_u16(),
        started,
        is_stream,
    );
    response
}
