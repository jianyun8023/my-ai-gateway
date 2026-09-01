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
use crate::infra::db;
use crate::infra::health;
use crate::infra::observability;
use crate::infra::secrets;
use crate::state::{authorized_with_db, error_response, resolve_credential, AppState};

use super::stream;
use super::transport;
use super::usage;

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
                error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_json",
                    "request body must be valid JSON",
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
    let virtual_key_id = match authorized_with_db(&state, &headers, model).await {
        Some(virtual_key_id) => virtual_key_id,
        None => {
            return finish_proxy(
                protocol,
                model,
                started,
                is_streamed,
                error_response(
                    StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "invalid or revoked virtual key",
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
                error_response(status, &error.code, &error.message),
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
            error_response(
                StatusCode::BAD_GATEWAY,
                "provider_not_found",
                "route references an unknown provider",
            ),
        );
    };
    let Some(account) = config.account(&route.primary_account_id) else {
        return finish_proxy(
            protocol,
            model,
            started,
            is_streamed,
            error_response(
                StatusCode::BAD_GATEWAY,
                "account_not_found",
                "route references an unknown account",
            ),
        );
    };
    let primary_unavailable = !account.enabled || !state.health.is_available(&account.id).await;
    if primary_unavailable {
        let Some(candidate) =
            select_fallback_candidate(&config, &state.health, &route, model, protocol).await
        else {
            return finish_proxy(
                protocol,
                model,
                started,
                is_streamed,
                error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    if account.enabled {
                        "account_cooling_down"
                    } else {
                        "account_disabled"
                    },
                    "primary account is unavailable and no fallback succeeded",
                ),
            );
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
                    error_response(status, code, message),
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
                total_tokens: usage.as_ref().map(|value| value.total_tokens).unwrap_or(0),
                usage_source: usage
                    .as_ref()
                    .map(|value| value.source.clone())
                    .unwrap_or_else(|| "missing".into()),
                degraded,
                route_id: Some(route.route_id.clone()),
                streamed: is_streamed,
                error_summary,
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
            record_response_health(
                &state.health,
                &route.source_id,
                &account.id,
                response.status(),
            )
            .await;
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
            total_tokens: usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
            usage_source: usage
                .as_ref()
                .map(|u| u.source.clone())
                .unwrap_or_else(|| "missing".into()),
            degraded,
            route_id: Some(route.route_id.clone()),
            streamed: is_streamed,
            error_summary: error_summary.clone(),
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

fn finish_proxy(
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

fn client_source_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("x-client-source")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

fn warn_degraded_route(request_id: &str, route: &ResolvedRoute) {
    if !route.is_degraded() {
        return;
    }
    warn_degraded_features(request_id, &route.route_id, &route.degraded_features);
}

fn warn_degraded_features(request_id: &str, route_id: &str, degraded_features: &[String]) {
    tracing::warn!(
        request_id = %request_id,
        route_id = %route_id,
        degraded_features = ?degraded_features,
        "route has degraded features due to adapter conversion"
    );
}

fn is_event_stream(response: &Response<Body>) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}

#[allow(clippy::too_many_arguments)]
fn wrap_stream_usage(
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
    let report = usage::usage_for_sse_response(event.success, request_body, &observation.captured);
    event.input_tokens = report.input_tokens;
    event.output_tokens = report.output_tokens;
    event.reasoning_tokens = report.reasoning_tokens;
    event.cached_tokens = report.cached_tokens;
    event.total_tokens = report.total_tokens;
    event.usage_source = report.source;
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(name = "gateway.forward", skip_all, fields(
    source_id = %route.source_id,
    account_id = %account.id,
    upstream_model = %route.upstream_model_id,
))]
async fn forward_account(
    _config: &GatewayConfig,
    secrets: &secrets::SecretResolver,
    http: &transport::SourceHttpClient,
    route: &ResolvedRoute,
    provider: &config::ProviderConfig,
    account: &config::AccountConfig,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = resolve_credential(secrets, account);
    if route.mode == "adapter" {
        if route.adapter.as_deref() == Some("kimi_responses_adapter") {
            return embedded_kimi_adapter(
                http,
                provider,
                account,
                credential.as_deref(),
                headers,
                body,
                stream_config,
                request_started,
            )
            .await;
        }
        Err(transport::TransportError::Request)
    } else {
        transport::forward_url_with_config(
            http,
            &route.upstream_endpoint,
            account,
            credential.as_deref(),
            route.protocol_upstream,
            headers,
            body,
            stream_config,
            request_started,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn embedded_kimi_adapter(
    http: &transport::SourceHttpClient,
    provider: &config::ProviderConfig,
    _account: &config::AccountConfig,
    credential: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    http.validate_base_url(&provider.base_url)?;
    // The embedded adapter does not pass through the native transport helper,
    // so retain the original request and explicitly attach the same usage
    // report for completed JSON responses.
    let request_body = body.clone();
    let is_streaming = serde_json::from_slice::<Value>(&request_body)
        .ok()
        .and_then(|value| value.get("stream").and_then(Value::as_bool))
        .unwrap_or(false);
    let cfg = kimi_responses_adapter::adapter::config::Config {
        listen_addr: String::new(),
        kimi_base_url: provider.base_url.trim_end_matches('/').to_string(),
        anthropic_beta: String::new(),
        model_map: Default::default(),
        client_source: String::new(),
        models: provider.models.clone(),
        max_tokens: 32768,
        thinking_budgets: [
            ("low".into(), 4096),
            ("medium".into(), 16384),
            ("high".into(), 32768),
        ]
        .into_iter()
        .collect(),
        search_status_prefix: "Search results for query:".into(),
        stream_config: kimi_responses_adapter::adapter::config::StreamConfig::from_durations(
            stream_config.heartbeat_interval,
            stream_config.connection_timeout,
            stream_config.first_event_timeout,
            stream_config.idle_timeout,
            stream_config.total_timeout,
        ),
    };
    let adapter =
        kimi_responses_adapter::adapter::server::router_with_client(cfg, http.raw_client());
    let mut request = Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .body(Body::from(body))
        .map_err(|_| transport::TransportError::Request)?;
    request
        .extensions_mut()
        .insert(kimi_responses_adapter::adapter::server::StreamRequestStart(
            request_started,
        ));
    let request_headers = request.headers_mut();
    for (name, value) in headers {
        if !matches!(
            name.as_str(),
            "host" | "content-length" | "authorization" | "x-api-key"
        ) {
            request_headers.insert(name, value.clone());
        }
    }
    if let Some(value) = credential {
        if headers.contains_key("x-api-key") {
            if let Ok(value) = HeaderValue::from_str(value) {
                request_headers.insert("x-api-key", value);
            }
        } else if let Ok(value) = HeaderValue::from_str(&format!("Bearer {value}")) {
            request_headers.insert("authorization", value);
        }
    }
    let response = adapter
        .oneshot(request)
        .await
        .map_err(|_| transport::TransportError::Request)?;
    if is_streaming || is_event_stream(&response) {
        return Ok(response);
    }
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 16 * 1024 * 1024)
        .await
        .map_err(|_| transport::TransportError::Request)?;
    let report = usage::usage_for_json_response(parts.status.is_success(), &request_body, &bytes);
    let mut response = Response::from_parts(parts, Body::from(bytes));
    response.extensions_mut().insert(report);
    Ok(response)
}

struct FallbackCandidate<'a> {
    account: &'a config::AccountConfig,
    provider: &'a config::ProviderConfig,
    provider_id: String,
    source_id: String,
    upstream_model: String,
    protocol_upstream: Protocol,
    mode: String,
    adapter: Option<String>,
    upstream_endpoint: Option<String>,
    degraded_features: Vec<String>,
}

async fn select_fallback_candidate<'a>(
    config: &'a GatewayConfig,
    health: &health::HealthRegistry,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
) -> Option<FallbackCandidate<'a>> {
    let mut available = Vec::new();
    if !route.fallback_bindings.is_empty() {
        for binding in &route.fallback_bindings {
            if binding.account_id == route.primary_account_id {
                continue;
            }
            let Some(account) = config.account(&binding.account_id) else {
                continue;
            };
            if !account.enabled || !health.is_available(&account.id).await {
                continue;
            }
            let Some(provider) = config.provider(&binding.source_id) else {
                continue;
            };
            available.push(FallbackCandidate {
                account,
                provider,
                provider_id: binding.provider_id.clone(),
                source_id: binding.source_id.clone(),
                upstream_model: binding.upstream_model_id.clone(),
                protocol_upstream: binding.protocol_upstream,
                mode: binding.mode.clone(),
                adapter: binding.adapter.clone(),
                upstream_endpoint: Some(binding.upstream_endpoint.clone()),
                degraded_features: binding.degraded_features.clone(),
            });
        }
        if available.iter().any(|candidate| candidate.mode == "native") {
            available.retain(|candidate| candidate.mode == "native");
        }
    } else {
        for id in &route.fallback_accounts {
            if id == &route.primary_account_id {
                continue;
            }
            let Some(account) = config.account(id) else {
                continue;
            };
            if !account.enabled {
                continue;
            }
            if !health.is_available(&account.id).await {
                continue;
            }
            let Some(provider) = config.provider(&account.provider_id) else {
                continue;
            };
            if account.provider_id != route.source_id {
                let cap =
                    config.protocol_capability(&provider.id, Some(&account.id), model, protocol);
                if cap.mode != config::ProtocolMode::Native {
                    continue;
                }
            }
            let upstream_model = account
                .model_map
                .get(model)
                .cloned()
                .unwrap_or_else(|| model.to_string());
            available.push(FallbackCandidate {
                account,
                provider,
                provider_id: provider.id.clone(),
                source_id: provider.id.clone(),
                upstream_model,
                protocol_upstream: route.protocol_upstream,
                mode: route.mode.clone(),
                adapter: route.adapter.clone(),
                upstream_endpoint: None,
                degraded_features: route.degraded_features.clone(),
            });
        }
    }
    if available.is_empty() {
        return None;
    }
    let total: u32 = available.iter().map(|c| c.account.weight.max(1)).sum();
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
        % total.max(1);
    let mut cursor = 0;
    let mut selected_index = 0;
    for (i, candidate) in available.iter().enumerate() {
        cursor += candidate.account.weight.max(1);
        if tick < cursor {
            selected_index = i;
            break;
        }
    }
    Some(available.swap_remove(selected_index))
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(name = "gateway.fallback", skip_all, fields(model = %model, protocol = %protocol))]
async fn try_fallback(
    config: &GatewayConfig,
    secrets: &secrets::SecretResolver,
    health: &health::HealthRegistry,
    http: &transport::SourceHttpClient,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    first: Response<Body>,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> (Response<Body>, Vec<db::UsageAttempt>) {
    let mut attempts = Vec::new();
    let Some(candidate) = select_fallback_candidate(config, health, route, model, protocol).await
    else {
        return (first, attempts);
    };
    let prepared = transport::prepare_model_request(&body, model, &candidate.upstream_model);
    let started = Instant::now();
    match forward_fallback(
        config,
        secrets,
        http,
        candidate.provider,
        candidate.account,
        candidate.protocol_upstream,
        &candidate.mode,
        candidate.adapter.as_deref(),
        candidate.upstream_endpoint.as_deref(),
        headers,
        prepared.body,
        stream_config,
        request_started,
    )
    .await
    {
        Ok(response) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider_id.clone(),
                source_id: candidate.source_id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(prepared.upstream_model_id),
                status_code: response.status().as_u16() as i32,
                success: response.status().is_success(),
                latency_ms: started.elapsed().as_millis() as i64,
            });
            record_response_health(
                health,
                &candidate.source_id,
                &candidate.account.id,
                response.status(),
            )
            .await;
            observability::record_attempt(
                &protocol.to_string(),
                &candidate.source_id,
                &candidate.account.id,
                response.status().as_u16(),
                true,
            );
            (response, attempts)
        }
        Err(error) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider_id.clone(),
                source_id: candidate.source_id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(prepared.upstream_model_id),
                status_code: error.status_code(),
                success: false,
                latency_ms: started.elapsed().as_millis() as i64,
            });
            observability::record_attempt(
                &protocol.to_string(),
                &candidate.source_id,
                &candidate.account.id,
                error.status_code() as u16,
                true,
            );
            health
                .mark_failure_with_details(
                    &candidate.account.id,
                    "passive",
                    Some("upstream_transport_error"),
                    Some("upstream request failed"),
                )
                .await;
            (first, attempts)
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(name = "gateway.fallback_error", skip_all, fields(model = %model, protocol = %protocol))]
pub(crate) async fn try_fallback_error(
    config: &GatewayConfig,
    secrets: &secrets::SecretResolver,
    health: &health::HealthRegistry,
    http: &transport::SourceHttpClient,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    first_error: transport::TransportError,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> (Response<Body>, Vec<db::UsageAttempt>) {
    let mut attempts = Vec::new();
    let Some(candidate) = select_fallback_candidate(config, health, route, model, protocol).await
    else {
        return (
            error_response(
                transport_error_status(&first_error),
                "upstream_request_failed",
                first_error.message(),
            ),
            attempts,
        );
    };
    let prepared = transport::prepare_model_request(&body, model, &candidate.upstream_model);
    let started = Instant::now();
    match forward_fallback(
        config,
        secrets,
        http,
        candidate.provider,
        candidate.account,
        candidate.protocol_upstream,
        &candidate.mode,
        candidate.adapter.as_deref(),
        candidate.upstream_endpoint.as_deref(),
        headers,
        prepared.body,
        stream_config,
        request_started,
    )
    .await
    {
        Ok(response) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider_id.clone(),
                source_id: candidate.source_id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(prepared.upstream_model_id),
                status_code: response.status().as_u16() as i32,
                success: response.status().is_success(),
                latency_ms: started.elapsed().as_millis() as i64,
            });
            record_response_health(
                health,
                &candidate.source_id,
                &candidate.account.id,
                response.status(),
            )
            .await;
            observability::record_attempt(
                &protocol.to_string(),
                &candidate.source_id,
                &candidate.account.id,
                response.status().as_u16(),
                true,
            );
            (response, attempts)
        }
        Err(error) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider_id.clone(),
                source_id: candidate.source_id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(prepared.upstream_model_id),
                status_code: error.status_code(),
                success: false,
                latency_ms: started.elapsed().as_millis() as i64,
            });
            observability::record_attempt(
                &protocol.to_string(),
                &candidate.source_id,
                &candidate.account.id,
                error.status_code() as u16,
                true,
            );
            health
                .mark_failure_with_details(
                    &candidate.account.id,
                    "passive",
                    Some("upstream_transport_error"),
                    Some("upstream request failed"),
                )
                .await;
            (
                error_response(
                    transport_error_status(&error),
                    "upstream_request_failed",
                    error.message(),
                ),
                attempts,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn forward_fallback(
    _config: &GatewayConfig,
    secrets: &secrets::SecretResolver,
    http: &transport::SourceHttpClient,
    provider: &config::ProviderConfig,
    account: &config::AccountConfig,
    protocol: Protocol,
    mode: &str,
    adapter: Option<&str>,
    upstream_endpoint: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = resolve_credential(secrets, account);
    if mode == "adapter" {
        if adapter == Some("kimi_responses_adapter") {
            return embedded_kimi_adapter(
                http,
                provider,
                account,
                credential.as_deref(),
                headers,
                body,
                stream_config,
                request_started,
            )
            .await;
        }
        return Err(transport::TransportError::Request);
    }
    if let Some(endpoint) = upstream_endpoint {
        return transport::forward_url_with_config(
            http,
            endpoint,
            account,
            credential.as_deref(),
            protocol,
            headers,
            body,
            stream_config,
            request_started,
        )
        .await;
    }
    transport::forward_with_config(
        http,
        provider,
        account,
        credential.as_deref(),
        protocol,
        headers,
        body,
        stream_config,
        request_started,
    )
    .await
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn transport_error_status(error: &transport::TransportError) -> StatusCode {
    if matches!(error, transport::TransportError::Timeout(_)) {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    }
}

async fn record_response_health(
    health: &health::HealthRegistry,
    source_id: &str,
    account_id: &str,
    status: StatusCode,
) {
    if is_retryable(status) {
        observability::record_cooldown(source_id, account_id);
        let code = format!("upstream_http_{}", status.as_u16());
        health
            .mark_failure_with_details(
                account_id,
                "passive",
                Some(code.as_str()),
                Some("retryable upstream response"),
            )
            .await;
    } else if status.is_success() {
        health.mark_success(account_id).await;
    }
}
