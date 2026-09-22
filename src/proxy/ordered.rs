use super::accounting::{is_event_stream, wrap_stream_usage};
use super::fallback::{available_fallback_candidates, FallbackCandidate};
use super::forward::forward_fallback;
use super::policy::{
    is_retryable, primary_unavailable_reason, record_response_health, transport_error_status,
    warn_degraded_features,
};
use super::service::finish_proxy;
use super::settlement::SettlementPermit;
use super::{stream, transport};
use crate::domain::{config::GatewayConfig, protocol::Protocol, routing::ResolvedRoute};
use crate::http::response::data_plane_error_response;
use crate::infra::{db, observability};
use crate::state::AppState;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Response, StatusCode};
use std::time::Instant;

/// Ordered routes have a separate attempt loop so the existing weighted
/// primary/fallback policy, including its Retry-After behavior, is unchanged.
#[allow(clippy::too_many_arguments)]
pub(super) async fn proxy_ordered(
    state: &AppState,
    config: &GatewayConfig,
    route: &ResolvedRoute,
    headers: &HeaderMap,
    body: Bytes,
    protocol: Protocol,
    model: &str,
    request_id: &str,
    virtual_key_id: Option<i64>,
    client_source: String,
    is_streamed: bool,
    settlement_permit: Option<SettlementPermit>,
    stream_config: &stream::StreamConfig,
    started: Instant,
) -> Response<Body> {
    let mut candidates = Vec::new();
    let mut fallback_reason = None;
    if let (Some(account), Some(provider)) = (
        config.account(&route.primary_account_id),
        config.provider(&route.source_id),
    ) {
        let health = state.health.get_health(&account.id).await;
        if account.enabled && health.available {
            candidates.push(FallbackCandidate {
                account,
                provider,
                provider_id: route.provider_id.clone(),
                source_id: route.source_id.clone(),
                upstream_model: if route.binding_id.is_none() {
                    account
                        .model_map
                        .get(model)
                        .cloned()
                        .unwrap_or_else(|| route.upstream_model_id.clone())
                } else {
                    route.upstream_model_id.clone()
                },
                protocol_upstream: route.protocol_upstream,
                mode: route.mode.clone(),
                upstream_endpoint: Some(route.upstream_endpoint.clone()),
                degraded_features: route.degraded_features.clone(),
            });
        } else {
            fallback_reason = Some(if account.enabled {
                primary_unavailable_reason(&health)
            } else {
                "account_disabled".to_owned()
            });
        }
    }
    candidates
        .extend(available_fallback_candidates(config, &state.health, route, model, protocol).await);
    let attempt_limit = route
        .max_retries
        .map(|value| value as usize + 1)
        .unwrap_or(usize::MAX);
    let mut attempts = Vec::new();
    let mut last_response = None;
    let mut final_candidate = None;
    let mut usage_request_body = body.clone();
    let mut total_timeout = false;
    for candidate in &candidates {
        if attempts.len() >= attempt_limit {
            break;
        }
        // A preceding attempt can put this same account into cooldown. A
        // skipped line never consumes the request's retry allowance.
        if !state.health.is_available(&candidate.account.id).await {
            continue;
        }
        if !stream_config.total_timeout.is_zero()
            && started.elapsed() >= stream_config.total_timeout
        {
            total_timeout = true;
            last_response = Some(data_plane_error_response(
                protocol,
                StatusCode::GATEWAY_TIMEOUT,
                "gateway_total_timeout",
                stream::StreamTermination::TotalTimeout.message(),
                request_id,
            ));
            break;
        }
        let prepared = transport::prepare_model_request(&body, model, &candidate.upstream_model);
        usage_request_body = prepared.body.clone();
        let attempt_started = Instant::now();
        let result = forward_fallback(
            &state.secrets,
            &state.events,
            &state.http,
            candidate.provider,
            candidate.account,
            &candidate.source_id,
            request_id,
            candidate.protocol_upstream,
            &candidate.mode,
            candidate.upstream_endpoint.as_deref(),
            headers,
            prepared.body,
            stream_config,
            started,
        )
        .await;
        let (response, status_code, success, retryable) = match result {
            Ok(response) => {
                let status = response.status();
                record_response_health(
                    &state.health,
                    &candidate.source_id,
                    &candidate.account.id,
                    status,
                )
                .await;
                if is_retryable(status) && fallback_reason.is_none() {
                    fallback_reason = Some(format!("upstream_http_{}", status.as_u16()));
                }
                (
                    response,
                    status.as_u16() as i32,
                    status.is_success(),
                    is_retryable(status),
                )
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
                fallback_reason.get_or_insert_with(|| "upstream_transport_error".to_owned());
                total_timeout = matches!(
                    error,
                    transport::TransportError::Timeout(stream::StreamTermination::TotalTimeout)
                );
                let response = data_plane_error_response(
                    protocol,
                    transport_error_status(&error),
                    if total_timeout {
                        "gateway_total_timeout"
                    } else {
                        "upstream_request_failed"
                    },
                    error.message(),
                    request_id,
                );
                (response, error.status_code(), false, !total_timeout)
            }
        };
        observability::record_attempt(
            &protocol.to_string(),
            &candidate.source_id,
            &candidate.account.id,
            status_code as u16,
            !attempts.is_empty()
                || fallback_reason
                    .as_deref()
                    .is_some_and(|reason| reason.starts_with("account_")),
        );
        attempts.push(db::UsageAttempt {
            attempt_no: attempts.len() as i32,
            provider_id: candidate.provider_id.clone(),
            source_id: candidate.source_id.clone(),
            account_id: candidate.account.id.clone(),
            upstream_model_id: Some(prepared.upstream_model_id),
            status_code,
            success,
            latency_ms: attempt_started.elapsed().as_millis() as i64,
        });
        final_candidate = Some(candidate);
        last_response = Some(response);
        if !retryable {
            break;
        }
    }
    let response = last_response.unwrap_or_else(|| {
        data_plane_error_response(
            protocol,
            StatusCode::SERVICE_UNAVAILABLE,
            "route_unavailable",
            "no upstream line is currently available",
            request_id,
        )
    });
    let final_source = final_candidate
        .map(|candidate| candidate.source_id.as_str())
        .unwrap_or(&route.source_id);
    let final_provider = final_candidate
        .map(|candidate| candidate.provider_id.as_str())
        .unwrap_or(&route.provider_id);
    let final_account = final_candidate
        .map(|candidate| candidate.account.id.as_str())
        .unwrap_or(&route.primary_account_id);
    let final_model = final_candidate
        .map(|candidate| candidate.upstream_model.as_str())
        .unwrap_or(&route.upstream_model_id);
    let final_protocol = final_candidate
        .map(|candidate| candidate.protocol_upstream)
        .unwrap_or(route.protocol_upstream);
    let final_mode = final_candidate
        .map(|candidate| candidate.mode.as_str())
        .unwrap_or(&route.mode);
    let degraded_features = final_candidate
        .map(|candidate| candidate.degraded_features.as_slice())
        .unwrap_or(&route.degraded_features);
    let degraded = !degraded_features.is_empty();
    if degraded {
        warn_degraded_features(request_id, &route.route_id, degraded_features);
    }
    if let Some(database) = &state.db {
        let usage = transport::usage_from_response(&response);
        let event = db::UsageEvent {
            request_id: request_id.to_owned(),
            virtual_key_id,
            provider_id: final_provider.to_owned(),
            account_id: final_account.to_owned(),
            model: model.to_owned(),
            logical_model: model.to_owned(),
            upstream_model_id: Some(final_model.to_owned()),
            source_id: final_source.to_owned(),
            client_source,
            protocol_in: protocol.to_string(),
            protocol_upstream: final_protocol.to_string(),
            mode: final_mode.to_owned(),
            status_code: response.status().as_u16() as i32,
            success: response.status().is_success(),
            retry_count: attempts.len().saturating_sub(1) as i32,
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
                .unwrap_or_else(|| "missing".to_owned()),
            degraded,
            route_id: Some(route.route_id.clone()),
            streamed: is_streamed,
            error_summary: if total_timeout {
                Some("gateway_total_timeout".to_owned())
            } else {
                (!response.status().is_success())
                    .then(|| format!("HTTP {}", response.status().as_u16()))
            },
            fallback_reason: (attempts.len() > 1
                || fallback_reason
                    .as_deref()
                    .is_some_and(|reason| reason.starts_with("account_")))
            .then_some(fallback_reason)
            .flatten(),
        };
        if is_event_stream(&response) {
            return wrap_stream_usage(
                response,
                database.clone(),
                settlement_permit.expect("database backed requests reserve settlement capacity"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AdminAuth;
    use crate::domain::config::Capabilities;
    use crate::domain::routing::{RouteResolver, RuntimeBinding, RuntimeRoute};
    use crate::infra::{events, health, secrets};
    use crate::state::LiveConfig;
    use crate::test_helpers::{EnvRestore, ENV_LOCK, TEST_ADMIN_KEY};
    use axum::{body::to_bytes, Json, Router};
    use futures_util::{stream as futures_stream, StreamExt};
    use serde_json::{json, Value};
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex, RwLock},
        time::Duration,
    };

    async fn upstream(app: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}"), task)
    }

    fn state(
        base_url: &str,
        protocol: Protocol,
        max_retries: Option<i32>,
        timeout: Option<i64>,
    ) -> AppState {
        let config: GatewayConfig = serde_json::from_value(json!({
            "listen_addr":"127.0.0.1:0",
            "providers":[{"id":"source", "name":"Source", "base_url":base_url,
                "endpoints":{"openai_chat_completions":"/chat","openai_responses":"/responses","anthropic_messages":"/messages"}}],
            "accounts":[
                {"id":"a","provider_id":"source","display_name":"A","weight":1},
                {"id":"b","provider_id":"source","display_name":"B","weight":1},
                {"id":"c","provider_id":"source","display_name":"C","weight":10000}
            ], "routes":[]
        })).unwrap();
        let config = Arc::new(config);
        let route = RuntimeRoute {
            route_id: "ordered".to_owned(),
            model: "public-model".to_owned(),
            protocol,
            strategy: "ordered_fallback".to_owned(),
            request_timeout_ms: timeout,
            max_retries,
            allow_lossy_conversion: false,
            bindings: ["a", "b", "c"]
                .iter()
                .enumerate()
                .map(|(position, id)| RuntimeBinding {
                    binding_id: position as i64,
                    source_id: "source".to_owned(),
                    provider_id: "custom".to_owned(),
                    account_id: (*id).to_owned(),
                    upstream_model_id: (*id).to_owned(),
                    protocol_upstream: protocol,
                    upstream_endpoint: format!("{base_url}/upstream"),
                    mode: "native".to_owned(),
                    adapter: None,
                    effective_capabilities: Capabilities::native(),
                    degraded_features: Vec::new(),
                })
                .collect(),
        };
        AppState {
            live: Arc::new(RwLock::new(LiveConfig {
                resolver: RouteResolver::from_runtime(config.clone(), vec![route]),
                config,
                models: Arc::new(Vec::new()),
                revision: 1,
                generated_at: chrono::Utc::now(),
            })),
            http: crate::http::test_client().unwrap(),
            db: None,
            control_plane: None,
            events: events::EventRepository::disabled(),
            health: health::HealthRegistry::new(Duration::from_secs(30)),
            admin_auth: AdminAuth::test(),
            secrets: secrets::SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
            settlements: crate::proxy::settlement::SettlementManager::default(),
        }
    }

    async fn request(state: AppState, protocol: Protocol, stream: bool) -> Response<Body> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {TEST_ADMIN_KEY}").parse().unwrap(),
        );
        headers.insert("content-type", "application/json".parse().unwrap());
        super::super::service::proxy(state, headers, Bytes::from(json!({
            "model":"public-model","stream":stream,"messages":[{"role":"user","content":"hello"}]
        }).to_string()), protocol).await
    }

    #[tokio::test]
    async fn native_thinking_cross_source_fallback_preserves_each_attempt() {
        let _lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", TEST_ADMIN_KEY);
        for strategy in ["primary_then_weighted_fallback", "ordered_fallback"] {
            for protocol in [
                Protocol::OpenAiChatCompletions,
                Protocol::OpenAiResponses,
                Protocol::AnthropicMessages,
            ] {
                let calls = Arc::new(Mutex::new(Vec::new()));
                let recorded = calls.clone();
                let (url, task) =
                    upstream(Router::new().fallback(move |Json(body): Json<Value>| {
                        recorded.lock().unwrap().push(body.clone());
                        async move {
                            (
                                if body["model"] == "b" {
                                    StatusCode::OK
                                } else {
                                    StatusCode::SERVICE_UNAVAILABLE
                                },
                                Json(json!({"usage":{"input_tokens":1,"output_tokens":1}})),
                            )
                        }
                    }))
                    .await;
                let state = state(&url, protocol, None, None);
                // Runtime bindings, unlike legacy configs, permit cross-source fallback.
                {
                    let mut live = state.live.write().unwrap();
                    let mut config = (*live.config).clone();
                    let mut source = config.providers[0].clone();
                    source.id = "second-source".into();
                    config.providers.push(source);
                    config.accounts[1].provider_id = "second-source".into();
                    let mut routes = live.resolver.runtime_routes().unwrap().to_vec();
                    routes[0].strategy = strategy.into();
                    routes[0].bindings.truncate(2);
                    routes[0].bindings[1].source_id = "second-source".into();
                    routes[0].bindings[1].provider_id = "second-provider".into();
                    live.config = Arc::new(config);
                    live.resolver = RouteResolver::from_runtime(live.config.clone(), routes);
                }
                let body = json!({
                    "model":"public-model",
                    "messages":[{"role":"assistant","content":"answer without reasoning"}],
                    "input":[{"role":"assistant","content":"answer without reasoning"}],
                    "thinking":{"type":"enabled","budget_tokens":4096},
                    "reasoning_effort":"high", "reasoning_split":true,
                    "reasoning":{"effort":"high"},
                    "provider_extension":{"signature":"opaque"}
                });
                let mut headers = HeaderMap::new();
                headers.insert("content-type", "application/json".parse().unwrap());
                headers.insert(
                    "authorization",
                    format!("Bearer {TEST_ADMIN_KEY}").parse().unwrap(),
                );
                let response = super::super::service::proxy(
                    state,
                    headers,
                    Bytes::from(body.to_string()),
                    protocol,
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK, "{strategy}: {protocol}");
                let recorded = calls.lock().unwrap();
                assert_eq!(recorded.len(), 2, "{strategy}: {protocol}");
                for (actual, model) in recorded.iter().zip(["a", "b"]) {
                    let mut expected = body.clone();
                    expected["model"] = json!(model);
                    assert_eq!(*actual, expected, "{strategy}: {protocol}");
                }
                task.abort();
            }
        }
    }

    #[tokio::test]
    async fn ordered_lines_try_three_upstreams_in_order_for_each_protocol() {
        let _lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", TEST_ADMIN_KEY);
        for protocol in [
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let recorded = calls.clone();
            let (url, task) = upstream(Router::new().fallback(move |Json(body): Json<Value>| {
                recorded.lock().unwrap().push(body["model"].as_str().unwrap().to_owned());
                async move {
                    (if body["model"] == "c" {StatusCode::OK} else {StatusCode::SERVICE_UNAVAILABLE}, Json(json!({"model":body["model"],"usage":{"input_tokens":1,"output_tokens":1}})))
                }
            })).await;
            let response = request(state(&url, protocol, None, None), protocol, false).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(*calls.lock().unwrap(), vec!["a", "b", "c"]);
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap())
                    .unwrap();
            assert_eq!(body["model"], "c");
            task.abort();
        }
    }

    #[tokio::test]
    async fn ordered_retry_limit_counts_actual_attempts_and_skips_cooling_lines() {
        let _lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", TEST_ADMIN_KEY);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let (url, task) = upstream(Router::new().fallback(move |Json(body): Json<Value>| {
            recorded
                .lock()
                .unwrap()
                .push(body["model"].as_str().unwrap().to_owned());
            async {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":"unavailable"})),
                )
            }
        }))
        .await;
        let protocol = Protocol::OpenAiChatCompletions;
        let response = request(state(&url, protocol, Some(1), None), protocol, false).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(*calls.lock().unwrap(), vec!["a", "b"]);
        calls.lock().unwrap().clear();
        let state = state(&url, protocol, Some(0), None);
        for _ in 0..3 {
            state.health.mark_failure("a").await;
            state.health.mark_failure("b").await;
        }
        assert!(!state.health.is_available("a").await);
        let response = request(state, protocol, false).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(*calls.lock().unwrap(), vec!["c"]);
        task.abort();
    }

    #[tokio::test]
    async fn ordered_request_timeout_is_shared_across_fallback_attempts() {
        let _lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", TEST_ADMIN_KEY);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let (url, task) = upstream(Router::new().fallback(move |Json(body): Json<Value>| {
            recorded
                .lock()
                .unwrap()
                .push(body["model"].as_str().unwrap().to_owned());
            async move {
                tokio::time::sleep(Duration::from_millis(if body["model"] == "a" {
                    50
                } else {
                    200
                }))
                .await;
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":"unavailable"})),
                )
            }
        }))
        .await;
        let protocol = Protocol::OpenAiChatCompletions;
        let started = Instant::now();
        let response = request(state(&url, protocol, None, Some(120)), protocol, false).await;
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(*calls.lock().unwrap(), vec!["a", "b"]);
        assert!(started.elapsed() < Duration::from_millis(500));
        task.abort();
    }

    #[tokio::test]
    async fn ordered_request_timeout_terminates_sse_without_replaying_on_backup() {
        let _lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", TEST_ADMIN_KEY);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let (url, task) = upstream(Router::new().fallback(move |Json(body):Json<Value>| {
            recorded.lock().unwrap().push(body["model"].as_str().unwrap().to_owned());
            async {
                let first = futures_stream::once(async { Ok::<_,Infallible>(Bytes::from_static(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n")) });
                Response::builder().header("content-type", "text/event-stream")
                    .body(Body::from_stream(first.chain(futures_stream::pending()))).unwrap()
            }
        })).await;
        let protocol = Protocol::OpenAiChatCompletions;
        let response = request(state(&url, protocol, None, Some(80)), protocol, true).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = tokio::time::timeout(
            Duration::from_secs(1),
            to_bytes(response.into_body(), 64 * 1024),
        )
        .await
        .unwrap()
        .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("hello"));
        assert!(body.contains("gateway_total_timeout"), "{body}");
        assert_eq!(*calls.lock().unwrap(), vec!["a"]);
        task.abort();
    }
}
