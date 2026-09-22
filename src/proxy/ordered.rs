use super::attempt::AttemptContext;
use super::completion::{FinalUpstream, RequestCompletion};
use super::fallback::{available_fallback_candidates, FallbackCandidate};
use super::policy::{
    is_retryable, primary_unavailable_reason, transport_error_status, warn_degraded_features,
};
use super::{stream, transport};
use crate::domain::config::GatewayConfig;
use crate::http::response::data_plane_error_response;
use axum::body::Body;
use axum::http::{Response, StatusCode};

/// Ordered selection owns its retry allowance and stop conditions; attempt
/// execution and request settlement are the same as the weighted policy.
pub(super) async fn proxy_ordered(
    config: &GatewayConfig,
    context: &AttemptContext<'_>,
    completion: RequestCompletion<'_>,
) -> Response<Body> {
    let route = completion.route;
    let mut candidates = Vec::new();
    let mut fallback_reason = None;
    if let (Some(account), Some(provider)) = (
        config.account(&route.primary_account_id),
        config.provider(&route.source_id),
    ) {
        let health = context.health.get_health(&account.id).await;
        if account.enabled && health.available {
            candidates.push(FallbackCandidate::primary(
                route,
                account,
                provider,
                context.model,
            ));
        } else {
            fallback_reason = Some(if account.enabled {
                primary_unavailable_reason(&health)
            } else {
                "account_disabled".to_owned()
            });
        }
    }
    candidates.extend(
        available_fallback_candidates(
            config,
            context.health,
            route,
            context.model,
            context.protocol,
        )
        .await,
    );
    let attempt_limit = route
        .max_retries
        .map(|value| value as usize + 1)
        .unwrap_or(usize::MAX);
    let mut attempts = Vec::new();
    let mut last_response = None;
    let mut final_candidate = None;
    let mut usage_request_body = context.body.clone();
    let mut total_timeout = false;
    for candidate in &candidates {
        if attempts.len() >= attempt_limit {
            break;
        }
        // A preceding attempt can cool down the same account. Skipped lines
        // do not consume the request's retry allowance.
        if !context.health.is_available(&candidate.account.id).await {
            continue;
        }
        if !context.stream_config.total_timeout.is_zero()
            && context.started.elapsed() >= context.stream_config.total_timeout
        {
            total_timeout = true;
            last_response = Some(data_plane_error_response(
                context.protocol,
                StatusCode::GATEWAY_TIMEOUT,
                "gateway_total_timeout",
                stream::StreamTermination::TotalTimeout.message(),
                context.request_id,
            ));
            break;
        }
        let is_fallback = !attempts.is_empty()
            || fallback_reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("account_"));
        let attempted = context
            .execute(candidate, attempts.len(), is_fallback)
            .await;
        usage_request_body = attempted.request_body;
        attempts.push(attempted.usage);
        let (response, retryable) = match attempted.result {
            Ok(response) => {
                let retryable = is_retryable(response.status());
                if retryable && fallback_reason.is_none() {
                    fallback_reason = Some(format!("upstream_http_{}", response.status().as_u16()));
                }
                (response, retryable)
            }
            Err(error) => {
                fallback_reason.get_or_insert_with(|| "upstream_transport_error".to_owned());
                total_timeout = matches!(
                    error,
                    transport::TransportError::Timeout(stream::StreamTermination::TotalTimeout)
                );
                let response = data_plane_error_response(
                    context.protocol,
                    transport_error_status(&error),
                    if total_timeout {
                        "gateway_total_timeout"
                    } else {
                        "upstream_request_failed"
                    },
                    error.message(),
                    context.request_id,
                );
                (response, !total_timeout)
            }
        };
        final_candidate = Some(candidate);
        last_response = Some(response);
        if !retryable {
            break;
        }
    }
    let response = last_response.unwrap_or_else(|| {
        data_plane_error_response(
            context.protocol,
            StatusCode::SERVICE_UNAVAILABLE,
            "route_unavailable",
            "no upstream line is currently available",
            context.request_id,
        )
    });
    let degraded_features = final_candidate
        .map(|candidate| candidate.degraded_features.as_slice())
        .unwrap_or(&route.degraded_features);
    if !degraded_features.is_empty() {
        warn_degraded_features(context.request_id, &route.route_id, degraded_features);
    }
    let upstream = final_candidate
        .map(FinalUpstream::candidate)
        .unwrap_or_else(|| FinalUpstream::route(route));
    let fallback_reason = (attempts.len() > 1
        || fallback_reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("account_")))
    .then_some(fallback_reason)
    .flatten();
    completion
        .finish(
            response,
            upstream,
            attempts,
            usage_request_body,
            fallback_reason,
            total_timeout.then(|| "gateway_total_timeout".to_owned()),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AdminAuth;
    use crate::domain::config::Capabilities;
    use crate::domain::protocol::Protocol;
    use crate::domain::routing::{RouteResolver, RuntimeBinding, RuntimeRoute};
    use crate::infra::observability;
    use crate::infra::{events, health, secrets};
    use crate::state::AppState;
    use crate::state::LiveConfig;
    use crate::test_helpers::{EnvRestore, ENV_LOCK, TEST_ADMIN_KEY};
    use axum::{body::to_bytes, Json, Router};
    use axum::{body::Bytes, http::HeaderMap};
    use futures_util::{stream as futures_stream, StreamExt};
    use serde_json::{json, Value};
    use std::time::Instant;
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
    async fn request_timeout_is_shared_across_policies_and_protocols() {
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
                let started = Instant::now();
                let state = state(&url, protocol, None, Some(120));
                configure_strategy(&state, strategy, 2);
                let response = request(state, protocol, false).await;
                // Weighted fallback retains the primary HTTP error if its fallback
                // transport times out; ordered routes return the last timeout.
                assert_eq!(
                    response.status(),
                    if strategy == "ordered_fallback" {
                        StatusCode::GATEWAY_TIMEOUT
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                );
                assert_eq!(*calls.lock().unwrap(), vec!["a", "b"]);
                assert!(started.elapsed() < Duration::from_millis(500));
                task.abort();
            }
        }
    }

    #[tokio::test]
    async fn request_timeout_terminates_sse_without_replaying_on_backup() {
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
                let (url, task) = upstream(Router::new().fallback(move |Json(body):Json<Value>| {
            recorded.lock().unwrap().push(body["model"].as_str().unwrap().to_owned());
            async {
                let first = futures_stream::once(async { Ok::<_,Infallible>(Bytes::from_static(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n")) });
                Response::builder().header("content-type", "text/event-stream")
                    .body(Body::from_stream(first.chain(futures_stream::pending()))).unwrap()
            }
        })).await;
                let state = state(&url, protocol, None, Some(80));
                configure_strategy(&state, strategy, 2);
                let response = request(state, protocol, true).await;
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
    }
    fn configure_strategy(state: &AppState, strategy: &str, lines: usize) {
        let mut live = state.live.write().unwrap();
        let mut routes = live.resolver.runtime_routes().unwrap().to_vec();
        routes[0].strategy = strategy.to_owned();
        routes[0].bindings.truncate(lines);
        live.resolver = RouteResolver::from_runtime(live.config.clone(), routes);
    }

    fn attempt_count(
        state: &AppState,
        protocol: Protocol,
        account: &str,
        status: u16,
        fallback: bool,
    ) -> u64 {
        let labels = [
            format!("protocol=\"{protocol}\""),
            format!("account=\"{account}\""),
            format!("status=\"{status}\""),
            format!("fallback=\"{fallback}\""),
            "source=\"source\"".to_owned(),
        ];
        state
            .prometheus_handle
            .render()
            .lines()
            .filter(|line| line.starts_with("gateway_upstream_attempts_total{"))
            .find(|line| labels.iter().all(|label| line.contains(label)))
            .map(|line| line.rsplit(' ').next().unwrap().parse().unwrap())
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn both_policies_preserve_retryable_statuses_and_primary_priority() {
        let _lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", TEST_ADMIN_KEY);
        for strategy in ["primary_then_weighted_fallback", "ordered_fallback"] {
            for protocol in [
                Protocol::OpenAiChatCompletions,
                Protocol::OpenAiResponses,
                Protocol::AnthropicMessages,
            ] {
                for status in [400, 401, 403, 404, 408, 429, 500, 502, 503] {
                    let calls = Arc::new(Mutex::new(Vec::new()));
                    let recorded = calls.clone();
                    let (url, task) =
                        upstream(Router::new().fallback(move |Json(body): Json<Value>| {
                            recorded
                                .lock()
                                .unwrap()
                                .push(body["model"].as_str().unwrap().to_owned());
                            async move {
                                Response::builder()
                                    .status(if body["model"] == "a" { status } else { 201 })
                                    .header("content-type", "application/json")
                                    .header("x-provider-extension", "preserved")
                                    .body(Body::from(body.to_string()))
                                    .unwrap()
                            }
                        }))
                        .await;
                    let state = state(&url, protocol, None, None);
                    configure_strategy(&state, strategy, 2);
                    let primary_before = attempt_count(&state, protocol, "a", status, false);
                    let backup_before = attempt_count(&state, protocol, "b", 201, true);
                    let response = request(state.clone(), protocol, false).await;
                    let retryable = status == 408 || status == 429 || status >= 500;
                    assert_eq!(
                        response.status().as_u16(),
                        if retryable { 201 } else { status },
                        "{strategy} {protocol} {status}"
                    );
                    assert_eq!(response.headers()["x-provider-extension"], "preserved");
                    assert_eq!(
                        *calls.lock().unwrap(),
                        if retryable { vec!["a", "b"] } else { vec!["a"] }
                    );
                    assert_eq!(
                        attempt_count(&state, protocol, "a", status, false),
                        primary_before + 1
                    );
                    assert_eq!(
                        attempt_count(&state, protocol, "b", 201, true),
                        backup_before + u64::from(retryable)
                    );
                    let primary_health = state.health.get_health("a").await;
                    assert_eq!(
                        primary_health.consecutive_failures,
                        if retryable { 1 } else { 0 }
                    );
                    task.abort();
                }
            }
        }
    }

    #[tokio::test]
    async fn transport_failures_preserve_each_policy_response_mapping() {
        let _lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", TEST_ADMIN_KEY);
        for strategy in ["primary_then_weighted_fallback", "ordered_fallback"] {
            for protocol in [
                Protocol::OpenAiChatCompletions,
                Protocol::OpenAiResponses,
                Protocol::AnthropicMessages,
            ] {
                for primary in ["http", "transport", "disabled"] {
                    let (url, task) = upstream(Router::new().fallback(|| async {
                        (
                            StatusCode::TOO_MANY_REQUESTS,
                            Json(json!({"error":{"message":"primary rate limit"}})),
                        )
                    }))
                    .await;
                    let state = state(&url, protocol, None, None);
                    configure_strategy(&state, strategy, 2);
                    {
                        let mut live = state.live.write().unwrap();
                        let mut routes = live.resolver.runtime_routes().unwrap().to_vec();
                        routes[0].bindings[1].upstream_endpoint = "invalid URL".into();
                        if primary == "transport" {
                            routes[0].bindings[0].upstream_endpoint = "invalid URL".into();
                        } else if primary == "disabled" {
                            let mut config = (*live.config).clone();
                            config.accounts[0].enabled = false;
                            live.config = Arc::new(config);
                        }
                        live.resolver = RouteResolver::from_runtime(live.config.clone(), routes);
                    }
                    let backup_before = attempt_count(&state, protocol, "b", 599, true);
                    let response = request(state.clone(), protocol, false).await;
                    assert_eq!(
                        attempt_count(&state, protocol, "b", 599, true),
                        backup_before + 1
                    );
                    let expected = match (strategy, primary) {
                        ("primary_then_weighted_fallback", "http") => 429,
                        ("primary_then_weighted_fallback", "disabled") => 503,
                        _ => 502,
                    };
                    assert_eq!(
                        response.status().as_u16(),
                        expected,
                        "{strategy} {protocol} {primary}"
                    );
                    let text = String::from_utf8(
                        to_bytes(response.into_body(), 64 * 1024)
                            .await
                            .unwrap()
                            .to_vec(),
                    )
                    .unwrap();
                    assert!(
                        text.contains(if expected == 429 {
                            "primary rate limit"
                        } else if expected == 503 {
                            "account_disabled"
                        } else {
                            "upstream_request_failed"
                        }),
                        "{text}"
                    );
                    assert_eq!(state.health.get_health("b").await.consecutive_failures, 1);
                    task.abort();
                }
            }
        }
    }

    #[tokio::test]
    async fn only_weighted_policy_retries_short_retry_after_without_backup() {
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
                        let mut calls = recorded.lock().unwrap();
                        calls.push(body["model"].as_str().unwrap().to_owned());
                        let status = if calls.len() == 1 { 429 } else { 200 };
                        async move {
                            Response::builder()
                                .status(status)
                                .header("retry-after", "0")
                                .header("content-type", "application/json")
                                .body(Body::from("{}"))
                                .unwrap()
                        }
                    }))
                    .await;
                let state = state(&url, protocol, None, None);
                configure_strategy(&state, strategy, 1);
                let rate_limited_before = attempt_count(&state, protocol, "a", 429, false);
                let success_before = attempt_count(&state, protocol, "a", 200, false);
                let response = request(state.clone(), protocol, false).await;
                let weighted = strategy == "primary_then_weighted_fallback";
                assert_eq!(
                    attempt_count(&state, protocol, "a", 429, false),
                    rate_limited_before + 1
                );
                assert_eq!(
                    attempt_count(&state, protocol, "a", 200, false),
                    success_before + u64::from(weighted)
                );
                assert_eq!(response.status().as_u16(), if weighted { 200 } else { 429 });
                assert_eq!(
                    *calls.lock().unwrap(),
                    if weighted { vec!["a", "a"] } else { vec!["a"] }
                );
                task.abort();
            }
        }
    }
}
