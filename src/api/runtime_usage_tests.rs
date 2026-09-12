use crate::{
    auth::AdminAuth,
    control_plane,
    domain::{
        config::{self, GatewayConfig},
        protocol::Protocol,
        routing::RouteResolver,
    },
    http,
    infra::{db, health, observability, secrets},
    proxy::{
        accounting::finalize_stream_usage,
        fallback::try_fallback_error,
        service::proxy as proxy_fn,
        stream::{StreamConfig, StreamTermination},
        transport, usage,
    },
    state::{AppState, LiveConfig},
};
use axum::{
    body::{to_bytes, Body, Bytes},
    extract::Request,
    http::{header, HeaderMap, HeaderValue, Response, StatusCode},
    Router,
};
use futures_util::stream;
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::{
    collections::HashMap,
    str::FromStr,
    sync::atomic::{AtomicUsize, Ordering},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

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
        credential_ciphertext: None,
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

fn named_provider(id: &str, base_url: String, models: &[&str]) -> config::ProviderConfig {
    let mut provider = provider(base_url);
    provider.id = id.into();
    provider.name = id.into();
    provider.models = models.iter().map(|model| (*model).to_owned()).collect();
    provider
}

fn named_account(id: &str, source_id: &str, model_map: &[(&str, &str)]) -> config::AccountConfig {
    let mut account = account(id, &format!("{id}-secret"), "unused-upstream");
    account.provider_id = source_id.into();
    account.credential = None;
    account.credential_env = Some(format!(
        "TEST_{}_API_KEY",
        id.replace('-', "_").to_uppercase()
    ));
    account.model_map = model_map
        .iter()
        .map(|(logical, upstream)| ((*logical).to_owned(), (*upstream).to_owned()))
        .collect();
    account
}

fn named_route(
    id: &str,
    model: &str,
    source_id: &str,
    primary_account_id: &str,
    fallback_account_id: &str,
) -> config::RouteConfig {
    config::RouteConfig {
        id: id.into(),
        model: model.into(),
        provider_id: source_id.into(),
        protocols: vec![Protocol::OpenAiResponses],
        primary_account_id: primary_account_id.into(),
        fallback_accounts: vec![fallback_account_id.into()],
        strategy: "primary_then_weighted_fallback".into(),
        mode: "native".into(),
        adapter: None,
        allow_lossy_conversion: false,
    }
}

fn state(config: GatewayConfig, database: Option<db::Database>) -> AppState {
    let config = Arc::new(config);
    let events = database
        .as_ref()
        .map(|database| crate::infra::events::EventRepository::new(database.pool().clone()))
        .unwrap_or_else(crate::infra::events::EventRepository::disabled);
    AppState {
        live: Arc::new(std::sync::RwLock::new(LiveConfig::legacy(config))),
        http: http::test_client().expect("runtime HTTP client"),
        db: database,
        control_plane: None,
        events,
        health: health::HealthRegistry::new(Duration::from_secs(1)),
        admin_auth: AdminAuth::test(),
        secrets: secrets::SecretResolver::empty(),
        prometheus_handle: observability::prometheus_handle(),
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
        let response = proxy_fn(
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
            StatusCode::TOO_MANY_REQUESTS
        } else {
            StatusCode::OK
        };
        Response::builder()
            .status(status)
            .header(header::RETRY_AFTER, "0")
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
    let response = proxy_fn(
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
async fn short_retry_after_retries_same_account_once_when_no_fallback_exists() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (base_url, recorded) = spawn_upstream({
        let calls = calls.clone();
        move |_| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                return Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .header(header::RETRY_AFTER, "0")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"error":"rate limited"}"#))
                    .unwrap();
            }
            Response::builder()
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"id":"recovered"}"#))
                .unwrap()
        }
    })
    .await;
    let config = GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        providers: vec![provider(base_url)],
        accounts: vec![account("primary", "primary-secret", "primary-upstream")],
        routes: vec![route(Protocol::OpenAiChatCompletions, false)],
    };

    let response = proxy_fn(
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
    assert!(recorded
        .iter()
        .all(|request| request.authorization.as_deref() == Some("Bearer primary-secret")));
    assert!(recorded
        .iter()
        .all(|request| request.body["model"] == "primary-upstream"));
}

#[tokio::test]
async fn long_retry_after_is_not_retried_on_the_same_account() {
    let (base_url, recorded) = spawn_upstream(|_| {
        Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header(header::RETRY_AFTER, "3")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"error":"rate limited"}"#))
            .unwrap()
    })
    .await;
    let config = GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        providers: vec![provider(base_url)],
        accounts: vec![account("primary", "primary-secret", "primary-upstream")],
        routes: vec![route(Protocol::OpenAiChatCompletions, false)],
    };

    let response = proxy_fn(
        state(config, None),
        static_auth_headers(),
        Bytes::from_static(
            br#"{"model":"logical-model","messages":[{"role":"user","content":"hello"}]}"#,
        ),
        Protocol::OpenAiChatCompletions,
    )
    .await;

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    drain(response).await;
    assert_eq!(recorded.lock().unwrap().len(), 1);
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
        &secrets::SecretResolver::empty(),
        &crate::infra::events::EventRepository::disabled(),
        &health::HealthRegistry::new(Duration::from_secs(1)),
        &http::test_client().unwrap(),
        &resolved,
        "logical-model",
        Protocol::OpenAiChatCompletions,
        &HeaderMap::new(),
        Bytes::from_static(br#"{"model":"logical-model","messages":[]}"#),
        transport::TransportError::Request,
        &StreamConfig::default(),
        Instant::now(),
        "test-request-id",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].account_id, "fallback");
    assert_eq!(attempts[0].source_id, "runtime-provider");
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
        &secrets::SecretResolver::empty(),
        &crate::infra::events::EventRepository::disabled(),
        &health::HealthRegistry::new(Duration::from_secs(1)),
        &http::test_client().unwrap(),
        &resolved,
        "logical-model",
        Protocol::OpenAiChatCompletions,
        &HeaderMap::new(),
        Bytes::from_static(br#"{"model":"logical-model","messages":[]}"#),
        transport::TransportError::Request,
        &StreamConfig::default(),
        Instant::now(),
        "test-request-id",
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status_code, 599);
    assert!(!attempts[0].success);
    assert_eq!(attempts[0].account_id, "fallback");
    assert_eq!(attempts[0].source_id, "runtime-provider");
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
        source_id: "runtime-provider".into(),
        client_source: "test".into(),
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
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        total_tokens: 0,
        usage_source: "missing".into(),
        degraded: false,
        route_id: Some("route".into()),
        streamed: true,
        error_summary: None,
        fallback_reason: None,
    }
}

#[test]
fn failed_stream_keeps_ttft_absent_and_never_estimates_tokens() {
    let mut event = usage_event();
    let mut attempts = vec![db::UsageAttempt {
        attempt_no: 0,
        provider_id: "provider".into(),
        source_id: "runtime-provider".into(),
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
            termination: StreamTermination::UpstreamError,
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

#[test]
fn stream_termination_reasons_have_stable_usage_statuses_and_summaries() {
    let cases = [
        (
            StreamTermination::EmptyStream,
            599,
            "upstream stream ended without an event",
        ),
        (
            StreamTermination::ClientCancelled,
            499,
            "client disconnected",
        ),
        (
            StreamTermination::FirstEventTimeout,
            504,
            "first event timeout",
        ),
        (StreamTermination::IdleTimeout, 504, "upstream idle timeout"),
        (StreamTermination::TotalTimeout, 504, "stream total timeout"),
    ];
    for (termination, status, summary) in cases {
        let mut event = usage_event();
        let mut attempts = vec![db::UsageAttempt {
            attempt_no: 0,
            provider_id: "provider".into(),
            source_id: "runtime-provider".into(),
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
                termination,
            },
        );
        assert_eq!(event.status_code, status);
        assert!(!event.success);
        assert_eq!(event.error_summary.as_deref(), Some(summary));
        assert_eq!(attempts[0].status_code, status);
        assert!(!attempts[0].success);
    }
}

async fn unused_local_url() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind temporary unused port");
    let address = listener.local_addr().expect("temporary unused address");
    drop(listener);
    format!("http://{address}")
}

async fn runtime_request(
    state: &AppState,
    virtual_key: &str,
    client_source: &str,
    payload: Value,
) -> Response<Body> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {virtual_key}")).unwrap(),
    );
    headers.insert(
        "x-client-source",
        HeaderValue::from_str(client_source).unwrap(),
    );
    proxy_fn(
        state.clone(),
        headers,
        Bytes::from(serde_json::to_vec(&payload).unwrap()),
        Protocol::OpenAiResponses,
    )
    .await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via TEST_DATABASE_URL"]
async fn postgres_ordered_routing_api_records_each_attempt_once_and_deadline_exhaustion() {
    use tower::ServiceExt;
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let schema = format!("ordered_usage_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
        .execute(&admin)
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .unwrap();
    let database = db::Database::from_test_pool(pool.clone()).await.unwrap();
    let (upstream_url, requests) = spawn_upstream(|request| {
        let success =
            request.body["model"] == "upstream-c" && request.body["scenario"] != "all-failed";
        Response::builder()
            .status(if success {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            })
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                if success {
                    json!({"model":"upstream-c","usage":{"input_tokens":2,"output_tokens":3}})
                } else {
                    json!({"error":"unavailable"})
                }
                .to_string(),
            ))
            .unwrap()
    })
    .await;
    let mut ordered_route = named_route(
        "ordered-route",
        "logical-model",
        "source-a",
        "account-a",
        "account-b",
    );
    ordered_route.fallback_accounts.push("account-c".to_owned());
    let config = GatewayConfig {
        listen_addr: "127.0.0.1:0".to_owned(),
        providers: ["source-a", "source-b", "source-c"]
            .iter()
            .map(|source| named_provider(source, upstream_url.clone(), &["logical-model"]))
            .collect(),
        accounts: [
            ("account-a", "source-a", "upstream-a"),
            ("account-b", "source-b", "upstream-b"),
            ("account-c", "source-c", "upstream-c"),
        ]
        .iter()
        .map(|(account, source, upstream)| {
            named_account(account, source, &[("logical-model", upstream)])
        })
        .collect(),
        routes: vec![ordered_route],
    };
    let control_plane = control_plane::ControlPlane::with_url_policy(
        database.pool().clone(),
        "127.0.0.1:0",
        crate::source_url::test_policy(),
    );
    let initial = control_plane
        .initialize_from_config(&config, false)
        .await
        .unwrap()
        .unwrap();
    let id = control_plane
        .list_logical_models()
        .await
        .unwrap()
        .remove(0)
        .id;
    let state = AppState {
        live: Arc::new(std::sync::RwLock::new(LiveConfig::from_snapshot(initial))),
        http: http::test_client().unwrap(),
        db: Some(database.clone()),
        control_plane: Some(control_plane.clone()),
        events: database.event_repository(),
        health: health::HealthRegistry::with_database_config(
            database.clone(),
            health::HealthConfig {
                failure_threshold: 100,
                ..Default::default()
            },
        ),
        admin_auth: AdminAuth::test(),
        secrets: secrets::SecretResolver::empty(),
        prometheus_handle: observability::prometheus_handle(),
    };
    let app = crate::app::application(state.clone());
    let uri = format!("/admin/logical-models/{id}/routing");
    let mut payload = json!({"public_name":"logical-model","display_name":"Logical model","enabled":true,
        "lines":[
            {"source_id":"source-a","account_id":"account-a","upstream_model_id":"upstream-a"},
            {"source_id":"source-b","account_id":"account-b","upstream_model_id":"upstream-b"},
            {"source_id":"source-c","account_id":"account-c","upstream_model_id":"upstream-c"}
        ],"request_timeout_ms":null,"max_retries":null});
    let admin_request = |method: &str, body: Body| {
        axum::http::Request::builder()
            .method(method)
            .uri(&uri)
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", crate::test_helpers::TEST_ADMIN_KEY),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap()
    };
    let unauthorized = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(&uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let saved = app
        .clone()
        .oneshot(admin_request("PUT", Body::from(payload.to_string())))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    let saved: Value = serde_json::from_slice(&drain(saved).await).unwrap();
    assert_eq!(saved["data"]["strategy"], "ordered_fallback");
    assert_eq!(saved["snapshot_revision"], state.snapshot().revision);
    let read = app
        .clone()
        .oneshot(admin_request("GET", Body::empty()))
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
    let read: Value = serde_json::from_slice(&drain(read).await).unwrap();
    assert_eq!(read["data"]["lines"].as_array().unwrap().len(), 3);
    let original_routing = read["data"].clone();
    let original_revision = state.snapshot().revision;
    let creation_request = |body: &Value, authorized: bool| {
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri("/admin/model-routings")
            .header(header::CONTENT_TYPE, "application/json");
        if authorized {
            request = request.header(
                header::AUTHORIZATION,
                format!("Bearer {}", crate::test_helpers::TEST_ADMIN_KEY),
            );
        }
        request.body(Body::from(body.to_string())).unwrap()
    };
    let unauthorized_create = app
        .clone()
        .oneshot(creation_request(&payload, false))
        .await
        .unwrap();
    assert_eq!(unauthorized_create.status(), StatusCode::UNAUTHORIZED);
    let duplicate = app
        .clone()
        .oneshot(creation_request(&payload, true))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    drain(duplicate).await;
    let missing = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("PUT")
                .uri("/admin/logical-models/missing-id/routing")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", crate::test_helpers::TEST_ADMIN_KEY),
                )
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    drain(missing).await;
    assert_eq!(state.snapshot().revision, original_revision);
    assert_eq!(
        control_plane.load_snapshot().await.unwrap().revision,
        original_revision
    );

    // Reusing an existing model's ID as a new public name must create a
    // separate model, including when the original model has been renamed.
    assert_ne!(id, "logical-model");
    let mut create_payload = payload.clone();
    create_payload["public_name"] = json!(id);
    let created = app
        .clone()
        .oneshot(creation_request(&create_payload, true))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: Value = serde_json::from_slice(&drain(created).await).unwrap();
    assert_ne!(created["data"]["logical_model"]["id"], id);
    assert_eq!(created["data"]["logical_model"]["public_name"], id);
    let unchanged = app
        .clone()
        .oneshot(admin_request("GET", Body::empty()))
        .await
        .unwrap();
    let unchanged: Value = serde_json::from_slice(&drain(unchanged).await).unwrap();
    assert_eq!(unchanged["data"], original_routing);
    assert_eq!(state.snapshot().revision, original_revision + 1);

    let (key_id, key) = database
        .create_virtual_key("ordered-test", &[])
        .await
        .unwrap();
    for (client, scenario, expected) in [
        ("ordered-success", "success", StatusCode::OK),
        (
            "ordered-failed",
            "all-failed",
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ] {
        let response = runtime_request(
            &state,
            &key,
            client,
            json!({"model":"logical-model","input":"hello","scenario":scenario}),
        )
        .await;
        assert_eq!(response.status(), expected);
        drain(response).await;
    }
    let filter = db::UsageFilter {
        virtual_key_id: Some(key_id),
        ..Default::default()
    };
    let events = database
        .list_usage_events_page(&filter, 10, None)
        .await
        .unwrap()
        .data;
    assert_eq!(events.len(), 2);
    for event in &events {
        assert_eq!(event.source_id.as_deref(), Some("source-c"));
        assert_eq!(event.provider_id, "custom");
        assert_eq!(event.account_id, "account-c");
        assert_eq!(event.upstream_model_id.as_deref(), Some("upstream-c"));
        assert_eq!(event.logical_model, "logical-model");
        assert_eq!(event.retry_count, 2);
        assert_eq!(event.fallback_reason.as_deref(), Some("upstream_http_503"));
        let attempts = database
            .list_attempts_for_event(&event.request_id)
            .await
            .unwrap();
        assert_eq!(
            attempts
                .iter()
                .map(|attempt| attempt.attempt_no)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(
            attempts
                .iter()
                .map(|attempt| attempt.source_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("source-a"), Some("source-b"), Some("source-c")]
        );
        assert_eq!(
            attempts
                .iter()
                .map(|attempt| attempt.upstream_model_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("upstream-a"), Some("upstream-b"), Some("upstream-c")]
        );
        assert_eq!(event.total_tokens, if event.success { 5 } else { 0 });
        assert_eq!(
            attempts.iter().filter(|attempt| attempt.success).count(),
            usize::from(event.success)
        );
    }
    assert_eq!(
        database
            .usage_aggregate(&filter)
            .await
            .unwrap()
            .logical_requests,
        2
    );
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.body["model"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "upstream-a",
            "upstream-b",
            "upstream-c",
            "upstream-a",
            "upstream-b",
            "upstream-c"
        ]
    );

    // Simulate a slow passive-health write after a completed 503. The budget
    // expires before the next send; the previous HTTP error must become a
    // total-timeout response while preserving the single actual attempt.
    payload["request_timeout_ms"] = json!(80);
    let saved = app
        .clone()
        .oneshot(admin_request("PUT", Body::from(payload.to_string())))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    drain(saved).await;
    sqlx::raw_sql("CREATE FUNCTION ordered_slow_health() RETURNS TRIGGER AS $$ BEGIN PERFORM pg_sleep(0.15); RETURN NEW; END; $$ LANGUAGE plpgsql; CREATE TRIGGER ordered_slow_health AFTER UPDATE OF consecutive_failures ON accounts FOR EACH ROW WHEN (NEW.id='account-a' AND NEW.consecutive_failures > OLD.consecutive_failures) EXECUTE FUNCTION ordered_slow_health();")
        .execute(&pool).await.unwrap();
    let response = runtime_request(
        &state,
        &key,
        "ordered-deadline",
        json!({"model":"logical-model","input":"hello"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert!(String::from_utf8_lossy(&drain(response).await).contains("gateway_total_timeout"));
    let events = database
        .list_usage_events_page(&filter, 10, None)
        .await
        .unwrap()
        .data;
    assert_eq!(events.len(), 3);
    let deadline = events
        .iter()
        .find(|event| event.client_source == "ordered-deadline")
        .unwrap();
    assert_eq!(
        deadline.error_summary.as_deref(),
        Some("gateway_total_timeout")
    );
    assert_eq!(deadline.status_code, 504);
    assert_eq!(deadline.account_id, "account-a");
    assert_eq!(deadline.retry_count, 0);
    assert_eq!(
        database
            .list_attempts_for_event(&deadline.request_id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(requests.lock().unwrap().len(), 7);
    drop(app);
    drop(state);
    drop(control_plane);
    drop(database);
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn postgres_db_first_source_attribution_covers_primary_fallback_stream_and_failures() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL runtime usage test: TEST_DATABASE_URL is not set");
        return;
    };
    let (primary_url, primary_requests) = spawn_upstream(|request| {
        if request.body["scenario"] == "retry" {
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"error":"retry"}"#))
                .unwrap();
        }
        Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"id":"primary","usage":{"input_tokens":1,"output_tokens":1}}"#,
            ))
            .unwrap()
    })
    .await;
    let (fallback_url, fallback_requests) = spawn_upstream(|request| {
        if request.body["stream"].as_bool() == Some(true) {
            let chunks = stream::once(async {
                tokio::time::sleep(Duration::from_millis(15)).await;
                Ok::<Bytes, std::io::Error>(Bytes::from_static(
                    b"data: {\"usage\":{\"input_tokens\":2,\"output_tokens\":3}}\n\ndata: [DONE]\n\n",
                ))
            });
            return Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(chunks))
                .unwrap();
        }
        Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"id":"fallback","usage":{"input_tokens":2,"output_tokens":2}}"#,
            ))
            .unwrap()
    })
    .await;

    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect PostgreSQL runtime test admin database");
    let schema = format!("runtime_usage_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
        .execute(&admin)
        .await
        .expect("create isolated runtime usage schema");
    let options = PgConnectOptions::from_str(&url)
        .expect("parse TEST_DATABASE_URL")
        .options([("search_path", schema.as_str())]);
    let runtime_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .expect("connect isolated runtime usage schema");
    let database = db::Database::from_test_pool(runtime_pool.clone())
        .await
        .expect("migrate isolated runtime usage schema");
    let transport_primary_url = unused_local_url().await;
    let failed_primary_url = unused_local_url().await;
    let failed_fallback_url = unused_local_url().await;
    let config = GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        providers: vec![
            named_provider(
                "source-primary",
                primary_url,
                &["logical-model", "solo-model"],
            ),
            named_provider(
                "source-fallback",
                fallback_url,
                &["logical-model", "transport-model"],
            ),
            named_provider(
                "source-transport-primary",
                transport_primary_url,
                &["transport-model"],
            ),
            named_provider(
                "source-failed-primary",
                failed_primary_url,
                &["failure-model"],
            ),
            named_provider(
                "source-failed-fallback",
                failed_fallback_url,
                &["failure-model"],
            ),
        ],
        accounts: vec![
            named_account(
                "primary-account",
                "source-primary",
                &[
                    ("logical-model", "primary-upstream"),
                    ("solo-model", "solo-upstream"),
                ],
            ),
            named_account(
                "fallback-account",
                "source-fallback",
                &[
                    ("logical-model", "fallback-upstream"),
                    ("transport-model", "transport-fallback-upstream"),
                ],
            ),
            named_account(
                "transport-primary-account",
                "source-transport-primary",
                &[("transport-model", "transport-primary-upstream")],
            ),
            named_account(
                "failed-primary-account",
                "source-failed-primary",
                &[("failure-model", "failure-primary-upstream")],
            ),
            named_account(
                "failed-fallback-account",
                "source-failed-fallback",
                &[("failure-model", "failure-fallback-upstream")],
            ),
        ],
        routes: vec![
            named_route(
                "logical-route",
                "logical-model",
                "source-primary",
                "primary-account",
                "fallback-account",
            ),
            named_route(
                "transport-route",
                "transport-model",
                "source-transport-primary",
                "transport-primary-account",
                "fallback-account",
            ),
            named_route(
                "failure-route",
                "failure-model",
                "source-failed-primary",
                "failed-primary-account",
                "failed-fallback-account",
            ),
            config::RouteConfig {
                id: "solo-route".into(),
                model: "solo-model".into(),
                provider_id: "source-primary".into(),
                protocols: vec![Protocol::OpenAiResponses],
                primary_account_id: "primary-account".into(),
                fallback_accounts: Vec::new(),
                strategy: "primary_then_weighted_fallback".into(),
                mode: "native".into(),
                adapter: None,
                allow_lossy_conversion: false,
            },
        ],
    };
    let control_plane = control_plane::ControlPlane::with_url_policy(
        database.pool().clone(),
        "127.0.0.1:0",
        crate::source_url::test_policy(),
    );
    control_plane::model_catalog::install_builtin_presets(
        &control_plane::model_catalog::ModelCatalogRepository::new(database.pool().clone()),
    )
    .await
    .expect("install ProviderPreset fixtures");
    control_plane
        .initialize_from_config(&config, false)
        .await
        .expect("initialize DB-first runtime test control plane")
        .expect("empty isolated control plane publishes a snapshot");
    for (source_id, provider_preset_id) in [
        ("source-primary", "deepseek"),
        ("source-fallback", "deepseek"),
        ("source-transport-primary", "minimax"),
        ("source-failed-primary", "deepseek"),
        ("source-failed-fallback", "minimax"),
    ] {
        sqlx::query("UPDATE sources SET provider_preset_id=$2,provider_preset_version=1,provider_preset_snapshot=(SELECT definition FROM provider_presets WHERE id=$2 AND version=1) WHERE id=$1")
            .bind(source_id)
            .bind(provider_preset_id)
            .execute(&runtime_pool)
            .await
            .expect("assign stable Provider identity to Source fixture");
    }
    let snapshot = control_plane
        .load_snapshot()
        .await
        .expect("reload runtime snapshot with stable Provider identities");
    let state = AppState {
        live: Arc::new(std::sync::RwLock::new(LiveConfig::from_snapshot(snapshot))),
        http: http::test_client().expect("runtime HTTP client"),
        db: Some(database.clone()),
        control_plane: None,
        events: crate::infra::events::EventRepository::new(database.pool().clone()),
        health: health::HealthRegistry::with_config(health::HealthConfig {
            cooldown: Duration::from_secs(30),
            failure_threshold: 1,
            ..health::HealthConfig::default()
        }),
        admin_auth: AdminAuth::test(),
        secrets: secrets::SecretResolver::empty(),
        prometheus_handle: observability::prometheus_handle(),
    };

    let suffix = Uuid::new_v4().to_string();
    let (virtual_key_id, virtual_key) = database
        .create_virtual_key(&format!("runtime-{suffix}"), &[])
        .await
        .expect("create runtime virtual key");

    let response = runtime_request(
        &state,
        &virtual_key,
        "client-primary",
        json!({"model":"logical-model","input":"hello","scenario":"primary"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    drain(response).await;

    let response = runtime_request(
        &state,
        &virtual_key,
        "client-retry-stream",
        json!({"model":"logical-model","input":"hello","scenario":"retry","stream":true}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let streamed = drain(response).await;
    assert!(String::from_utf8_lossy(&streamed).contains("input_tokens"));

    let response = runtime_request(
        &state,
        &virtual_key,
        "client-early-fallback",
        json!({"model":"logical-model","input":"hello","scenario":"early"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    drain(response).await;

    let response = runtime_request(
        &state,
        &virtual_key,
        "client-transport-fallback",
        json!({"model":"transport-model","input":"hello"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    drain(response).await;

    let response = runtime_request(
        &state,
        &virtual_key,
        "client-all-failed",
        json!({"model":"failure-model","input":"hello"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    drain(response).await;

    let response = runtime_request(
        &state,
        &virtual_key,
        "client-cooldown-no-fallback",
        json!({"model":"solo-model","input":"hello"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    drain(response).await;

    let filter = db::UsageFilter {
        virtual_key_id: Some(virtual_key_id),
        ..Default::default()
    };
    let events = {
        let mut found = Vec::new();
        for _ in 0..100 {
            found = database
                .list_usage_events_page(&filter, 10, None)
                .await
                .expect("query runtime usage events")
                .data;
            if found.len() == 6 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(found.len(), 6, "all runtime usage events were persisted");
        found
    };
    let events = events
        .into_iter()
        .map(|event| (event.client_source.clone(), event))
        .collect::<HashMap<_, _>>();

    let primary = &events["client-primary"];
    assert_eq!(primary.provider_id, "deepseek");
    assert_eq!(primary.source_id.as_deref(), Some("source-primary"));
    assert_eq!(primary.account_id, "primary-account");
    assert_eq!(primary.retry_count, 0);

    let retry = &events["client-retry-stream"];
    assert_eq!(retry.provider_id, "deepseek");
    assert_eq!(retry.source_id.as_deref(), Some("source-fallback"));
    assert_eq!(retry.account_id, "fallback-account");
    assert_eq!(
        retry.upstream_model_id.as_deref(),
        Some("fallback-upstream")
    );
    assert_eq!(retry.retry_count, 1);
    assert!(retry.ttft_ms.is_some_and(|value| value >= 15));
    assert_eq!(retry.usage_source, "parsed");
    assert_eq!(retry.total_tokens, 5);
    let retry_attempts = database
        .list_attempts_for_event(&retry.request_id)
        .await
        .expect("query retry usage attempts");
    assert_eq!(retry_attempts.len(), 2);
    assert_eq!(
        retry_attempts
            .iter()
            .map(|attempt| attempt.provider_id.as_str())
            .collect::<Vec<_>>(),
        vec!["deepseek", "deepseek"]
    );
    assert_eq!(
        retry_attempts
            .iter()
            .map(|attempt| attempt.source_id.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("source-primary"), Some("source-fallback")]
    );
    assert_eq!(
        retry_attempts[0].upstream_model_id.as_deref(),
        Some("primary-upstream")
    );
    assert_eq!(
        retry_attempts[1].upstream_model_id.as_deref(),
        Some("fallback-upstream")
    );

    let early = &events["client-early-fallback"];
    assert_eq!(early.provider_id, "deepseek");
    assert_eq!(early.source_id.as_deref(), Some("source-fallback"));
    assert_eq!(early.account_id, "fallback-account");
    assert_eq!(early.retry_count, 0);
    let early_attempts = database
        .list_attempts_for_event(&early.request_id)
        .await
        .expect("query early fallback attempt");
    assert_eq!(early_attempts.len(), 1);
    assert_eq!(early_attempts[0].provider_id, "deepseek");
    assert_eq!(
        early_attempts[0].source_id.as_deref(),
        Some("source-fallback")
    );

    let blocked = &events["client-cooldown-no-fallback"];
    assert_eq!(blocked.status_code, 503);
    assert!(!blocked.success);
    assert_eq!(blocked.account_id, "primary-account");
    assert_eq!(blocked.retry_count, 0);
    assert_eq!(
        blocked.fallback_reason.as_deref(),
        Some("account_cooling_down")
    );
    assert!(database
        .list_attempts_for_event(&blocked.request_id)
        .await
        .expect("query blocked request attempts")
        .is_empty());

    let transport = &events["client-transport-fallback"];
    assert_eq!(transport.provider_id, "deepseek");
    assert_eq!(transport.source_id.as_deref(), Some("source-fallback"));
    assert_eq!(transport.retry_count, 1);
    let transport_attempts = database
        .list_attempts_for_event(&transport.request_id)
        .await
        .expect("query transport fallback attempts");
    assert_eq!(
        transport_attempts
            .iter()
            .map(|attempt| attempt.provider_id.as_str())
            .collect::<Vec<_>>(),
        vec!["minimax", "deepseek"]
    );
    assert_eq!(
        transport_attempts
            .iter()
            .map(|attempt| attempt.source_id.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("source-transport-primary"), Some("source-fallback")]
    );
    assert_eq!(transport_attempts[0].status_code, 599);
    assert!(transport_attempts[1].success);

    let failed = &events["client-all-failed"];
    assert_eq!(failed.provider_id, "minimax");
    assert_eq!(failed.source_id.as_deref(), Some("source-failed-fallback"));
    assert_eq!(failed.account_id, "failed-fallback-account");
    assert!(!failed.success);
    assert_eq!(failed.usage_source, "missing");
    let failed_attempts = database
        .list_attempts_for_event(&failed.request_id)
        .await
        .expect("query failed fallback attempts");
    assert_eq!(
        failed_attempts
            .iter()
            .map(|attempt| attempt.provider_id.as_str())
            .collect::<Vec<_>>(),
        vec!["deepseek", "minimax"]
    );
    assert_eq!(
        failed_attempts
            .iter()
            .map(|attempt| attempt.source_id.as_deref())
            .collect::<Vec<_>>(),
        vec![
            Some("source-failed-primary"),
            Some("source-failed-fallback")
        ]
    );
    assert!(failed_attempts.iter().all(|attempt| !attempt.success));

    let provider_breakdown = database
        .usage_breakdown(&filter, "provider")
        .await
        .expect("query Provider breakdown")
        .into_iter()
        .map(|row| (row.key.expect("Provider key"), row.logical_requests))
        .collect::<HashMap<_, _>>();
    assert_eq!(provider_breakdown.get("deepseek"), Some(&5));
    assert_eq!(provider_breakdown.get("minimax"), Some(&1));
    let source_breakdown = database
        .usage_breakdown(&filter, "source_id")
        .await
        .expect("query Source breakdown")
        .into_iter()
        .map(|row| (row.key.expect("Source key"), row.logical_requests))
        .collect::<HashMap<_, _>>();
    assert_eq!(source_breakdown.get("source-primary"), Some(&2));
    assert_eq!(source_breakdown.get("source-fallback"), Some(&3));
    assert_eq!(source_breakdown.get("source-failed-fallback"), Some(&1));
    let combined = db::UsageFilter {
        provider_id: Some("deepseek".into()),
        source_id: Some("source-fallback".into()),
        ..filter.clone()
    };
    assert_eq!(
        database
            .usage_aggregate(&combined)
            .await
            .expect("query combined Provider and Source filter")
            .logical_requests,
        3
    );

    assert_eq!(primary_requests.lock().unwrap().len(), 2);
    assert_eq!(fallback_requests.lock().unwrap().len(), 3);

    drop(state);
    drop(control_plane);
    drop(database);
    runtime_pool.close().await;
    sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
        .execute(&admin)
        .await
        .expect("drop isolated runtime usage schema");
    admin.close().await;
}
