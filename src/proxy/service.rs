use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{
    body::{to_bytes, Body, Bytes},
    http::{HeaderMap, HeaderValue, Request, Response, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

use crate::domain::config::{self, GatewayConfig};
use crate::domain::protocol::Protocol;
use crate::domain::routing::ResolvedRoute;
use crate::http::SourceHttpClient;
use crate::infra::db;
use crate::infra::health;
use crate::infra::observability;
use crate::infra::secrets;
use crate::state::{authorized_with_db, data_plane_error_response, resolve_credential, AppState};

use super::stream;
use super::transport;
use super::usage;

const MAX_SAME_ACCOUNT_RETRY_AFTER: Duration = Duration::from_secs(2);

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
    let stream_config = stream::StreamConfig::from_env();
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
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let is_streamed = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // Strip thinking / reasoning parameters when the conversation history is
    // missing provider-specific reasoning fields.  OpenAI-compatible clients
    // often drop non-standard fields (reasoning_content, reasoning_details,
    // reasoning_text, or type:"reasoning" items) when rebuilding history,
    // which makes MiniMax / DeepSeek return HTTP 400.
    let mut body = body;
    if transport::sanitize_thinking_params(&mut body, protocol) {
        tracing::warn!(
            request_id = %request_id,
            model = %model,
            protocol = %protocol,
            "stripped thinking/reasoning parameters: conversation history lacks reasoning fields"
        );
    }

    let virtual_key_id = match authorized_with_db(&state, &headers, Some(model)).await {
        Some(virtual_key_id) => virtual_key_id,
        None => {
            return finish_proxy(
                protocol,
                model,
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
    let Some(provider) = config.provider(&route.source_id) else {
        return finish_proxy(
            protocol,
            model,
            started,
            is_streamed,
            data_plane_error_response(
                protocol,
                StatusCode::BAD_GATEWAY,
                "provider_not_found",
                "route references an unknown provider",
                &request_id,
            ),
        );
    };
    let Some(account) = config.account(&route.primary_account_id) else {
        return finish_proxy(
            protocol,
            model,
            started,
            is_streamed,
            data_plane_error_response(
                protocol,
                StatusCode::BAD_GATEWAY,
                "account_not_found",
                "route references an unknown account",
                &request_id,
            ),
        );
    };
    let mut fallback_reason: Option<String> = None;
    let primary_unavailable = if !account.enabled {
        fallback_reason = Some("account_disabled".into());
        true
    } else {
        let health = state.health.get_health(&account.id).await;
        if health.available {
            false
        } else {
            fallback_reason = Some(primary_unavailable_reason(&health));
            true
        }
    };
    if primary_unavailable {
        let Some(candidate) =
            select_fallback_candidate(&config, &state.health, &route, model, protocol).await
        else {
            let response = data_plane_error_response(
                protocol,
                StatusCode::SERVICE_UNAVAILABLE,
                if account.enabled {
                    "account_cooling_down"
                } else {
                    "account_disabled"
                },
                "primary account is unavailable and no fallback succeeded",
                &request_id,
            );
            if let Some(database) = &state.db {
                let event = db::UsageEvent {
                    request_id: request_id.clone(),
                    virtual_key_id,
                    provider_id: route.provider_id.clone(),
                    account_id: account.id.clone(),
                    model: model.to_string(),
                    logical_model: model.to_string(),
                    upstream_model_id: Some(route.upstream_model_id.clone()),
                    source_id: route.source_id.clone(),
                    client_source: client_source_from_headers(&headers),
                    protocol_in: protocol.to_string(),
                    protocol_upstream: route.protocol_upstream.to_string(),
                    mode: route.mode.clone(),
                    status_code: StatusCode::SERVICE_UNAVAILABLE.as_u16() as i32,
                    success: false,
                    retry_count: 0,
                    latency_ms: started.elapsed().as_millis() as i64,
                    ttft_ms: None,
                    input_tokens: 0,
                    output_tokens: 0,
                    reasoning_tokens: 0,
                    cached_tokens: 0,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    total_tokens: 0,
                    usage_source: "missing".into(),
                    degraded: route.is_degraded(),
                    route_id: Some(route.route_id.clone()),
                    streamed: is_streamed,
                    error_summary: Some("HTTP 503".into()),
                    fallback_reason: fallback_reason.clone(),
                };
                if let Err(error) = database.insert_usage_with_attempts(&event, &[]).await {
                    tracing::warn!(%error, "failed to persist usage event");
                }
            }
            return finish_proxy(protocol, model, started, is_streamed, response);
        };
        if !route.is_degraded() && !candidate.degraded_features.is_empty() {
            warn_degraded_features(&request_id, &route.route_id, &candidate.degraded_features);
        }
        let prepared = transport::prepare_model_request(&body, model, &candidate.upstream_model);
        let usage_request_body = prepared.body.clone();
        let attempt_started = Instant::now();
        let (response, attempt_status, attempt_success) = match forward_fallback(
            &config,
            &state.secrets,
            &state.http,
            candidate.provider,
            candidate.account,
            candidate.protocol_upstream,
            &candidate.mode,
            candidate.adapter.as_deref(),
            candidate.upstream_endpoint.as_deref(),
            &headers,
            prepared.body,
            &stream_config,
            started,
        )
        .await
        {
            Ok(response) => {
                let status = response.status();
                record_response_health(
                    &state.health,
                    &candidate.source_id,
                    &candidate.account.id,
                    status,
                )
                .await;
                observability::record_attempt(
                    &protocol.to_string(),
                    &candidate.source_id,
                    &candidate.account.id,
                    status.as_u16(),
                    true,
                );
                (response, status.as_u16() as i32, status.is_success())
            }
            Err(error) => {
                state
                    .health
                    .mark_failure_with_details(
                        &candidate.account.id,
                        "passive",
                        Some("upstream_transport_error"),
                        Some("upstream request failed"),
                    )
                    .await;
                let (status, code, message) =
                    if matches!(&error, transport::TransportError::Timeout(_)) {
                        (
                            transport_error_status(&error),
                            "upstream_request_failed",
                            error.message(),
                        )
                    } else {
                        (
                            StatusCode::SERVICE_UNAVAILABLE,
                            if account.enabled {
                                "account_cooling_down"
                            } else {
                                "account_disabled"
                            },
                            "primary account is unavailable and no fallback succeeded",
                        )
                    };
                (
                    data_plane_error_response(protocol, status, code, message, &request_id),
                    error.status_code(),
                    false,
                )
            }
        };
        let usage = transport::usage_from_response(&response);
        let degraded = !candidate.degraded_features.is_empty();
        let error_summary = if !response.status().is_success() {
            Some(format!("HTTP {}", response.status().as_u16()))
        } else {
            None
        };
        if let Some(database) = &state.db {
            let client_source = client_source_from_headers(&headers);
            let event = db::UsageEvent {
                request_id,
                virtual_key_id,
                provider_id: candidate.provider_id.clone(),
                account_id: candidate.account.id.clone(),
                model: model.to_string(),
                logical_model: model.to_string(),
                upstream_model_id: Some(prepared.upstream_model_id.clone()),
                source_id: candidate.source_id.clone(),
                client_source,
                protocol_in: protocol.to_string(),
                protocol_upstream: candidate.protocol_upstream.to_string(),
                mode: candidate.mode.clone(),
                status_code: response.status().as_u16() as i32,
                success: response.status().is_success(),
                retry_count: 0,
                latency_ms: started.elapsed().as_millis() as i64,
                ttft_ms: None,
                input_tokens: usage.as_ref().map(|value| value.input_tokens).unwrap_or(0),
                output_tokens: usage.as_ref().map(|value| value.output_tokens).unwrap_or(0),
                reasoning_tokens: usage
                    .as_ref()
                    .map(|value| value.reasoning_tokens)
                    .unwrap_or(0),
                cached_tokens: usage.as_ref().map(|value| value.cached_tokens).unwrap_or(0),
                cache_read_tokens: usage
                    .as_ref()
                    .map(|value| value.cache_read_tokens)
                    .unwrap_or(0),
                cache_creation_tokens: usage
                    .as_ref()
                    .map(|value| value.cache_creation_tokens)
                    .unwrap_or(0),
                total_tokens: usage.as_ref().map(|value| value.total_tokens).unwrap_or(0),
                usage_source: usage
                    .as_ref()
                    .map(|value| value.source.clone())
                    .unwrap_or_else(|| "missing".into()),
                degraded,
                route_id: Some(route.route_id.clone()),
                streamed: is_streamed,
                error_summary,
                fallback_reason: fallback_reason.clone(),
            };
            let attempts = vec![db::UsageAttempt {
                attempt_no: 0,
                provider_id: candidate.provider_id.clone(),
                source_id: candidate.source_id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(prepared.upstream_model_id),
                status_code: attempt_status,
                success: attempt_success,
                latency_ms: attempt_started.elapsed().as_millis() as i64,
            }];
            if is_event_stream(&response) {
                return wrap_stream_usage(
                    response,
                    database.clone(),
                    event,
                    usage_request_body,
                    attempts,
                    started,
                    state.health.clone(),
                    protocol,
                    model,
                );
            }
            if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await {
                tracing::warn!(%error, "failed to persist usage event");
            }
        }
        return finish_proxy(protocol, model, started, is_streamed, response);
    }
    let primary_upstream_model = if route.binding_id.is_none() {
        account
            .model_map
            .get(model)
            .cloned()
            .unwrap_or_else(|| route.upstream_model_id.clone())
    } else {
        route.upstream_model_id.clone()
    };
    let primary_request = transport::prepare_model_request(&body, model, &primary_upstream_model);
    let usage_request_body = body.clone();
    let result_started = Instant::now();
    let result = forward_account(
        &config,
        &state.secrets,
        &state.http,
        &route,
        provider,
        account,
        &headers,
        primary_request.body,
        &stream_config,
        started,
    )
    .await;
    let mut attempts = Vec::new();
    let response = match result {
        Ok(response) if is_retryable(response.status()) => {
            let retry_after = (response.status() == StatusCode::TOO_MANY_REQUESTS)
                .then(|| retry_after_delay(response.headers()))
                .flatten();
            attempts.push(db::UsageAttempt {
                attempt_no: 0,
                provider_id: route.provider_id.clone(),
                source_id: route.source_id.clone(),
                account_id: account.id.clone(),
                upstream_model_id: Some(primary_request.upstream_model_id.clone()),
                status_code: response.status().as_u16() as i32,
                success: false,
                latency_ms: result_started.elapsed().as_millis() as i64,
            });
            fallback_reason = Some(format!("upstream_http_{}", response.status().as_u16()));
            record_response_health(
                &state.health,
                &route.source_id,
                &account.id,
                response.status(),
            )
            .await;
            let fallback_available =
                select_fallback_candidate(&config, &state.health, &route, model, protocol)
                    .await
                    .is_some();
            if let Some(delay) = retry_after.filter(|_| !fallback_available) {
                tokio::time::sleep(delay).await;
                let retry_started = Instant::now();
                let retry_request =
                    transport::prepare_model_request(&body, model, &primary_upstream_model);
                match forward_account(
                    &config,
                    &state.secrets,
                    &state.http,
                    &route,
                    provider,
                    account,
                    &headers,
                    retry_request.body,
                    &stream_config,
                    started,
                )
                .await
                {
                    Ok(retry_response) => {
                        attempts.push(db::UsageAttempt {
                            attempt_no: 1,
                            provider_id: route.provider_id.clone(),
                            source_id: route.source_id.clone(),
                            account_id: account.id.clone(),
                            upstream_model_id: Some(retry_request.upstream_model_id),
                            status_code: retry_response.status().as_u16() as i32,
                            success: retry_response.status().is_success(),
                            latency_ms: retry_started.elapsed().as_millis() as i64,
                        });
                        record_response_health(
                            &state.health,
                            &route.source_id,
                            &account.id,
                            retry_response.status(),
                        )
                        .await;
                        retry_response
                    }
                    Err(error) => {
                        attempts.push(db::UsageAttempt {
                            attempt_no: 1,
                            provider_id: route.provider_id.clone(),
                            source_id: route.source_id.clone(),
                            account_id: account.id.clone(),
                            upstream_model_id: Some(retry_request.upstream_model_id),
                            status_code: error.status_code(),
                            success: false,
                            latency_ms: retry_started.elapsed().as_millis() as i64,
                        });
                        state
                            .health
                            .mark_failure_with_details(
                                &account.id,
                                "passive",
                                Some("upstream_transport_error"),
                                Some("upstream request failed"),
                            )
                            .await;
                        data_plane_error_response(
                            protocol,
                            transport_error_status(&error),
                            "upstream_request_failed",
                            error.message(),
                            &request_id,
                        )
                    }
                }
            } else {
                let (response, mut fallback_attempts) = try_fallback(
                    &config,
                    &state.secrets,
                    &state.health,
                    &state.http,
                    &route,
                    model,
                    protocol,
                    &headers,
                    body,
                    response,
                    &stream_config,
                    started,
                )
                .await;
                attempts.append(&mut fallback_attempts);
                response
            }
        }
        Ok(response) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 0,
                provider_id: route.provider_id.clone(),
                source_id: route.source_id.clone(),
                account_id: account.id.clone(),
                upstream_model_id: Some(primary_request.upstream_model_id.clone()),
                status_code: response.status().as_u16() as i32,
                success: response.status().is_success(),
                latency_ms: result_started.elapsed().as_millis() as i64,
            });
            record_response_health(
                &state.health,
                &route.source_id,
                &account.id,
                response.status(),
            )
            .await;
            response
        }
        Err(error) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 0,
                provider_id: route.provider_id.clone(),
                source_id: route.source_id.clone(),
                account_id: account.id.clone(),
                upstream_model_id: Some(primary_request.upstream_model_id.clone()),
                status_code: error.status_code(),
                success: false,
                latency_ms: result_started.elapsed().as_millis() as i64,
            });
            fallback_reason = Some("upstream_transport_error".into());
            state
                .health
                .mark_failure_with_details(
                    &account.id,
                    "passive",
                    Some("upstream_transport_error"),
                    Some("upstream request failed"),
                )
                .await;
            let (response, mut fallback_attempts) = try_fallback_error(
                &config,
                &state.secrets,
                &state.health,
                &state.http,
                &route,
                model,
                protocol,
                &headers,
                body,
                error,
                &stream_config,
                started,
                &request_id,
            )
            .await;
            attempts.append(&mut fallback_attempts);
            response
        }
    };
    let usage = transport::usage_from_response(&response);
    let final_attempt = attempts
        .iter()
        .rev()
        .find(|attempt| attempt.success)
        .or_else(|| attempts.last());
    let final_binding = final_attempt.and_then(|attempt| {
        route.fallback_bindings.iter().find(|binding| {
            attempt.source_id == binding.source_id
                && binding.account_id == attempt.account_id
                && attempt.upstream_model_id.as_deref() == Some(binding.upstream_model_id.as_str())
        })
    });
    let final_protocol_upstream = final_binding
        .map(|binding| binding.protocol_upstream)
        .unwrap_or(route.protocol_upstream);
    let final_mode = final_binding
        .map(|binding| binding.mode.as_str())
        .unwrap_or(route.mode.as_str());
    let final_degraded_features = final_binding
        .map(|binding| &binding.degraded_features)
        .unwrap_or(&route.degraded_features);
    let degraded = !final_degraded_features.is_empty();
    if final_binding.is_some() && !route.is_degraded() && degraded {
        warn_degraded_features(&request_id, &route.route_id, final_degraded_features);
    }
    let error_summary = if !response.status().is_success() {
        Some(format!("HTTP {}", response.status().as_u16()))
    } else {
        None
    };
    if let Some(database) = &state.db {
        let client_source = client_source_from_headers(&headers);
        let final_account_id = final_attempt
            .map(|attempt| attempt.account_id.clone())
            .unwrap_or_else(|| account.id.clone());
        let final_provider_id = final_attempt
            .map(|attempt| attempt.provider_id.clone())
            .unwrap_or_else(|| route.provider_id.clone());
        let final_source_id = final_attempt
            .map(|attempt| attempt.source_id.clone())
            .unwrap_or_else(|| route.source_id.clone());
        let final_upstream_model_id = final_attempt
            .and_then(|attempt| attempt.upstream_model_id.clone())
            .or_else(|| Some(route.upstream_model_id.clone()));
        let event = db::UsageEvent {
            request_id,
            virtual_key_id,
            provider_id: final_provider_id,
            account_id: final_account_id,
            model: model.to_string(),
            logical_model: model.to_string(),
            upstream_model_id: final_upstream_model_id,
            source_id: final_source_id,
            client_source,
            protocol_in: protocol.to_string(),
            protocol_upstream: final_protocol_upstream.to_string(),
            mode: final_mode.to_owned(),
            status_code: response.status().as_u16() as i32,
            success: response.status().is_success(),
            retry_count: attempts.len().saturating_sub(1) as i32,
            latency_ms: started.elapsed().as_millis() as i64,
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
            degraded,
            route_id: Some(route.route_id.clone()),
            streamed: is_streamed,
            error_summary: error_summary.clone(),
            fallback_reason: if attempts.len() > 1 {
                fallback_reason.clone()
            } else {
                None
            },
        };
        if is_event_stream(&response) {
            return wrap_stream_usage(
                response,
                database.clone(),
                event,
                usage_request_body,
                attempts,
                started,
                state.health.clone(),
                protocol,
                model,
            );
        }
        if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await {
            tracing::warn!(%error, "failed to persist usage event");
        }
    }
    finish_proxy(protocol, model, started, is_streamed, response)
}

include!("service/response.rs");
include!("service/forward.rs");
include!("service/fallback.rs");
include!("service/policy.rs");

#[cfg(test)]
include!("service_tests.rs");
