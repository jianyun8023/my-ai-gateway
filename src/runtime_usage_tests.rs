use super::*;
use axum::{body::to_bytes, extract::Request, http::header, Router};
use futures_util::stream;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Debug)]
struct RecordedRequest {
    path: String,
    authorization: Option<String>,
    api_key: Option<String>,
    body: Value,
}

async fn spawn_upstream<F>(handler: F) -> (String, Arc<Mutex<Vec<RecordedRequest>>>)
where
    F: Fn(&RecordedRequest) -> Response<Body> + Send + Sync + 'static,
{
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(handler);
    let app = Router::new().fallback({
        let recorded = recorded.clone();
        move |request: Request| {
            let recorded = recorded.clone();
            let handler = handler.clone();
            async move {
                let (parts, body) = request.into_parts();
                let body = to_bytes(body, 1024 * 1024).await.unwrap();
                let request = RecordedRequest {
                    path: parts.uri.path().to_string(),
                    authorization: parts
                        .headers
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned),
                    api_key: parts
                        .headers
                        .get("x-api-key")
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned),
                    body: serde_json::from_slice(&body).unwrap(),
                };
                let response = handler(&request);
                recorded.lock().unwrap().push(request);
                response
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind runtime upstream");
    let address = listener.local_addr().expect("runtime upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve runtime upstream")
    });
    (format!("http://{address}"), recorded)
}

fn provider(base_url: String) -> config::ProviderConfig {
    config::ProviderConfig {
        id: "runtime-provider".into(),
        name: "Runtime provider".into(),
        base_url,
        models: vec!["logical-model".into()],
        native_protocols: vec![
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ],
        endpoints: HashMap::from([
            (
                Protocol::OpenAiChatCompletions,
                "/v1/chat/completions".into(),
            ),
            (Protocol::OpenAiResponses, "/v1/responses".into()),
            (Protocol::AnthropicMessages, "/v1/messages".into()),
        ]),
        capabilities: config::Capabilities::native(),
        protocol_capabilities: HashMap::new(),
        model_overrides: HashMap::new(),
    }
}

fn account(id: &str, credential: &str, upstream_model: &str) -> config::AccountConfig {
    config::AccountConfig {
        id: id.into(),
        provider_id: "runtime-provider".into(),
        display_name: id.into(),
        credential_env: None,
        credential: Some(credential.into()),
        enabled: true,
        weight: 100,
        protocol_capabilities: HashMap::new(),
        capabilities: None,
        model_overrides: HashMap::new(),
        model_map: HashMap::from([("logical-model".into(), upstream_model.into())]),
    }
}

fn route(protocol: Protocol, fallback: bool) -> config::RouteConfig {
    config::RouteConfig {
        id: format!("runtime-{protocol}"),
        model: "logical-model".into(),
        provider_id: "runtime-provider".into(),
        protocols: vec![protocol],
        primary_account_id: "primary".into(),
        fallback_accounts: fallback.then(|| "fallback".into()).into_iter().collect(),
        strategy: "primary_then_weighted_fallback".into(),
        mode: "native".into(),
        adapter: None,
        allow_lossy_conversion: false,
    }
}

fn state(config: GatewayConfig, database: Option<db::Database>) -> AppState {
    let config = Arc::new(config);
    AppState {
        live: Arc::new(std::sync::RwLock::new(LiveConfig::legacy(config))),
        http: transport::client().expect("runtime HTTP client"),
        db: database,
        control_plane: None,
        health: health::HealthRegistry::new(Duration::from_secs(1)),
    }
}

fn static_auth_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(key) = std::env::var("GATEWAY_API_KEY") {
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {key}")).unwrap(),
        );
    }
    headers
}

async fn drain(response: Response<Body>) -> Bytes {
    to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("drain runtime response")
}

#[tokio::test]
async fn primary_account_model_map_is_applied_for_all_three_protocols() {
    let (base_url, recorded) = spawn_upstream(|_| {
        Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"id":"ok","usage":{"input_tokens":1,"output_tokens":1}}"#,
            ))
            .unwrap()
    })
    .await;
    let config = GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        providers: vec![provider(base_url)],
        accounts: vec![account("primary", "primary-secret", "primary-upstream")],
        routes: vec![
            route(Protocol::OpenAiChatCompletions, false),
            route(Protocol::OpenAiResponses, false),
            route(Protocol::AnthropicMessages, false),
        ],
    };
    let state = state(config, None);
    let cases = [
        (
            Protocol::OpenAiChatCompletions,
            json!({"model":"logical-model","messages":[{"role":"user","content":"hello"}],"tools":[{"type":"function"}],"extension":{"keep":1}}),
        ),
        (
            Protocol::OpenAiResponses,
            json!({"model":"logical-model","input":"hello","reasoning":{"effort":"high"},"tools":[{"type":"web_search_preview"}],"extension":{"keep":2}}),
        ),
        (
            Protocol::AnthropicMessages,
            json!({"model":"logical-model","messages":[{"role":"user","content":"hello"}],"max_tokens":32,"thinking":{"type":"enabled","budget_tokens":16},"extension":{"keep":3}}),
        ),
    ];
    for (protocol, payload) in &cases {
        let response = proxy(
            state.clone(),
            static_auth_headers(),
            Bytes::from(serde_json::to_vec(payload).unwrap()),
            *protocol,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        drain(response).await;
    }

    let recorded = recorded.lock().unwrap();
    assert_eq!(recorded.len(), 3);
    for ((_, original), request) in cases.iter().zip(recorded.iter()) {
        let mut expected = original.clone();
        expected["model"] = Value::String("primary-upstream".into());
        assert_eq!(request.body, expected);
    }
    assert_eq!(recorded[0].path, "/v1/chat/completions");
    assert_eq!(recorded[1].path, "/v1/responses");
    assert_eq!(recorded[2].path, "/v1/messages");
    assert_eq!(
        recorded[0].authorization.as_deref(),
        Some("Bearer primary-secret")
    );
    assert_eq!(
        recorded[1].authorization.as_deref(),
        Some("Bearer primary-secret")
    );
    assert_eq!(recorded[2].api_key.as_deref(), Some("primary-secret"));
}

#[tokio::test]
async fn retryable_primary_response_uses_mapped_fallback_after_primary() {
    let (base_url, recorded) = spawn_upstream(|request| {
        let status = if request.authorization.as_deref() == Some("Bearer primary-secret") {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::OK
        };
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(r#"{{"status":{}}}"#, status.as_u16())))
            .unwrap()
    })
    .await;
    let config = GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        providers: vec![provider(base_url)],
        accounts: vec![
            account("primary", "primary-secret", "primary-upstream"),
            account("fallback", "fallback-secret", "fallback-upstream"),
        ],
        routes: vec![route(Protocol::OpenAiChatCompletions, true)],
    };
    let response = proxy(
        state(config, None),
        static_auth_headers(),
        Bytes::from_static(
            br#"{"model":"logical-model","messages":[{"role":"user","content":"hello"}]}"#,
        ),
        Protocol::OpenAiChatCompletions,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    drain(response).await;

    let recorded = recorded.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].body["model"], "primary-upstream");
    assert_eq!(recorded[1].body["model"], "fallback-upstream");
    assert_eq!(
        recorded[0].authorization.as_deref(),
        Some("Bearer primary-secret")
    );
    assert_eq!(
        recorded[1].authorization.as_deref(),
        Some("Bearer fallback-secret")
    );
}

#[tokio::test]
async fn transport_error_path_uses_fallback_and_records_its_actual_model() {
    let (base_url, recorded) = spawn_upstream(|_| {
        Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"id":"fallback"}"#))
            .unwrap()
    })
    .await;
    let config = Arc::new(GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        providers: vec![provider(base_url)],
        accounts: vec![
            account("primary", "primary-secret", "primary-upstream"),
            account("fallback", "fallback-secret", "fallback-upstream"),
        ],
        routes: vec![route(Protocol::OpenAiChatCompletions, true)],
    });
    let resolved = RouteResolver::new(config.clone())
        .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-model")
        .unwrap();
    let (response, attempts) = try_fallback_error(
        &config,
        &health::HealthRegistry::new(Duration::from_secs(1)),
        &transport::client().unwrap(),
        &resolved,
        "logical-model",
        Protocol::OpenAiChatCompletions,
        &HeaderMap::new(),
        Bytes::from_static(br#"{"model":"logical-model","messages":[]}"#),
        transport::TransportError::Request("primary transport failed".into()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].account_id, "fallback");
    assert_eq!(
        attempts[0].upstream_model_id.as_deref(),
        Some("fallback-upstream")
    );
    drain(response).await;
    assert_eq!(
        recorded.lock().unwrap()[0].body["model"],
        "fallback-upstream"
    );
}

#[tokio::test]
async fn fallback_transport_failure_is_retained_as_the_final_actual_attempt() {
    let config = Arc::new(GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        providers: vec![provider("not a valid upstream URL".into())],
        accounts: vec![
            account("primary", "primary-secret", "primary-upstream"),
            account("fallback", "fallback-secret", "fallback-upstream"),
        ],
        routes: vec![route(Protocol::OpenAiChatCompletions, true)],
    });
    let resolved = RouteResolver::new(config.clone())
        .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-model")
        .unwrap();
    let (response, attempts) = try_fallback_error(
        &config,
        &health::HealthRegistry::new(Duration::from_secs(1)),
        &transport::client().unwrap(),
        &resolved,
        "logical-model",
        Protocol::OpenAiChatCompletions,
        &HeaderMap::new(),
        Bytes::from_static(br#"{"model":"logical-model","messages":[]}"#),
        transport::TransportError::Request("primary transport failed".into()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status_code, 599);
    assert!(!attempts[0].success);
    assert_eq!(attempts[0].account_id, "fallback");
    assert_eq!(
        attempts[0].upstream_model_id.as_deref(),
        Some("fallback-upstream")
    );
}

fn usage_event() -> db::UsageEvent {
    db::UsageEvent {
        request_id: "request".into(),
        virtual_key_id: None,
        provider_id: "provider".into(),
        account_id: "account".into(),
        model: "logical-model".into(),
        logical_model: "logical-model".into(),
        upstream_model_id: Some("upstream-model".into()),
        source: "test".into(),
        protocol_in: "openai_responses".into(),
        protocol_upstream: "openai_responses".into(),
        mode: "native".into(),
        status_code: 200,
        success: true,
        retry_count: 0,
        latency_ms: 1,
        ttft_ms: None,
        input_tokens: 0,
        output_tokens: 0,
        reasoning_tokens: 0,
        cached_tokens: 0,
        total_tokens: 0,
        usage_source: "missing".into(),
        degraded: false,
        route_id: Some("route".into()),
        streamed: true,
        error_summary: None,
    }
}

#[test]
fn failed_stream_keeps_ttft_absent_and_never_estimates_tokens() {
    let mut event = usage_event();
    let mut attempts = vec![db::UsageAttempt {
        attempt_no: 0,
        provider_id: "provider".into(),
        account_id: "account".into(),
        upstream_model_id: Some("upstream-model".into()),
        status_code: 200,
        success: true,
        latency_ms: 1,
    }];
    finalize_stream_usage(
        &mut event,
        &mut attempts,
        br#"{"model":"logical-model","stream":true}"#,
        usage::StreamObservation {
            captured: Vec::new(),
            ttft_ms: None,
            failed: true,
        },
    );
    assert_eq!(event.ttft_ms, None);
    assert!(!event.success);
    assert_eq!(event.status_code, 599);
    assert_eq!(event.usage_source, "missing");
    assert_eq!(event.total_tokens, 0);
    assert!(!attempts[0].success);
    assert_eq!(attempts[0].status_code, 599);
}

#[tokio::test]
async fn postgres_stream_ttft_and_fallback_attribution_are_persisted() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL runtime usage test: TEST_DATABASE_URL is not set");
        return;
    };
    let (base_url, _) = spawn_upstream(|request| {
        if request.authorization.as_deref() == Some("Bearer primary-secret") {
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"error":"retry"}"#))
                .unwrap();
        }
        let chunks = stream::once(async {
            tokio::time::sleep(Duration::from_millis(15)).await;
            Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"data: {\"usage\":{\"input_tokens\":2,\"output_tokens\":3}}\n\n",
            ))
        });
        Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(chunks))
            .unwrap()
    })
    .await;
    let database = db::Database::connect(&url)
        .await
        .expect("connect PostgreSQL runtime usage database");
    let suffix = Uuid::new_v4().to_string();
    let (virtual_key_id, virtual_key) = database
        .create_virtual_key(&format!("runtime-{suffix}"), &[])
        .await
        .expect("create runtime virtual key");
    let config = GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        providers: vec![provider(base_url)],
        accounts: vec![
            account("primary", "primary-secret", "primary-upstream"),
            account("fallback", "fallback-secret", "fallback-upstream"),
        ],
        routes: vec![route(Protocol::OpenAiResponses, true)],
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {virtual_key}")).unwrap(),
    );
    let response = proxy(
        state(config, Some(database.clone())),
        headers,
        Bytes::from_static(br#"{"model":"logical-model","input":"hello","stream":true}"#),
        Protocol::OpenAiResponses,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let forwarded = drain(response).await;
    assert!(String::from_utf8_lossy(&forwarded).contains("input_tokens"));

    let filter = db::UsageFilter {
        logical_model: Some("logical-model".into()),
        virtual_key_id: Some(virtual_key_id),
        ..Default::default()
    };
    let event = {
        let mut found = None;
        for _ in 0..100 {
            found = database
                .list_usage_events_page(&filter, 1, None)
                .await
                .expect("query runtime usage event")
                .data
                .into_iter()
                .next();
            if found.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        found.expect("streaming usage event was persisted")
    };
    assert_eq!(event.account_id, "fallback");
    assert_eq!(event.provider_id, "runtime-provider");
    assert_eq!(
        event.upstream_model_id.as_deref(),
        Some("fallback-upstream")
    );
    assert_eq!(event.retry_count, 1);
    assert!(event.ttft_ms.is_some_and(|value| value >= 15));
    assert_eq!(event.usage_source, "parsed");
    assert_eq!(event.total_tokens, 5);
    let attempts = database
        .list_attempts_for_event(&event.request_id)
        .await
        .expect("query runtime usage attempts");
    assert_eq!(attempts.len(), 2);
    assert_eq!(
        attempts[0].upstream_model_id.as_deref(),
        Some("primary-upstream")
    );
    assert_eq!(
        attempts[1].upstream_model_id.as_deref(),
        Some("fallback-upstream")
    );
    assert!(attempts[1].success);

    sqlx::query("DELETE FROM usage_events WHERE request_id=$1")
        .bind(&event.request_id)
        .execute(database.pool())
        .await
        .expect("clean runtime usage event");
    sqlx::query("DELETE FROM virtual_keys WHERE id=$1")
        .bind(virtual_key_id)
        .execute(database.pool())
        .await
        .expect("clean runtime virtual key");
}
