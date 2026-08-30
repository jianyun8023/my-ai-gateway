mod config;
mod db;
mod protocol;
mod routing;
mod transport;

use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Request, Response, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use config::GatewayConfig;
use protocol::Protocol;
use routing::{ResolvedRoute, RouteResolver};
use serde::Deserialize;
use serde_json::{json, Value};
use tower::ServiceExt;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    config: Arc<GatewayConfig>,
    resolver: RouteResolver,
    http: reqwest::Client,
    db: Option<db::Database>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let config = Arc::new(GatewayConfig::from_env());
    let addr: SocketAddr = config.listen_addr.parse()?;
    let db = db::Database::connect_from_env().await?;
    let state = AppState {
        resolver: RouteResolver::new(config.clone()),
        config,
        http: transport::client()?,
        db,
    };
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/responses", post(responses))
        .route("/v1/messages", post(messages))
        .route("/admin/keys", get(list_keys).post(create_key))
        .route("/admin/keys/:id/revoke", post(revoke_key))
        .route("/admin/usage/summary", get(usage_summary))
        .route("/admin/routes/:protocol/:model", get(resolve_route))
        .with_state(state)
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "AI gateway listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz(State(state): State<AppState>) -> Json<Value> {
    Json(
        json!({"status":"ok", "providers":state.config.providers.len(), "accounts":state.config.accounts.len()}),
    )
}

async fn models(State(state): State<AppState>) -> Json<Value> {
    let data: Vec<Value> = state
        .config
        .models()
        .into_iter()
        .map(|model| json!({"id":model,"object":"model","owned_by":"gateway"}))
        .collect();
    Json(json!({"object":"list","data":data}))
}

async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    proxy(state, headers, body, Protocol::OpenAiChatCompletions).await
}
async fn responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    proxy(state, headers, body, Protocol::OpenAiResponses).await
}
async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    proxy(state, headers, body, Protocol::AnthropicMessages).await
}

#[derive(Deserialize)]
struct CreateKeyRequest {
    name: String,
    #[serde(default)]
    allowed_models: Vec<String>,
}

async fn create_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateKeyRequest>,
) -> Response<Body> {
    if !admin_authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let Some(database) = &state.db else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        );
    };
    match database.create_virtual_key(&request.name, &request.allowed_models).await {
        Ok((id, key)) => (StatusCode::CREATED, Json(json!({"id":id,"key":key,"name":request.name,"allowed_models":request.allowed_models}))).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "key_create_failed", &error.to_string()),
    }
}

async fn list_keys(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if !admin_authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let Some(database) = &state.db else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        );
    };
    match database.list_virtual_keys().await {
        Ok(keys) => (StatusCode::OK, Json(json!({"data":keys}))).into_response(),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "key_list_failed",
            &error.to_string(),
        ),
    }
}

async fn revoke_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response<Body> {
    if !admin_authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let Some(database) = &state.db else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        );
    };
    match database.revoke_virtual_key(id).await {
        Ok(true) => (StatusCode::OK, Json(json!({"id":id,"revoked":true}))).into_response(),
        Ok(false) => error_response(
            StatusCode::NOT_FOUND,
            "key_not_found",
            "virtual key not found or already revoked",
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "key_revoke_failed",
            &error.to_string(),
        ),
    }
}

async fn usage_summary(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if !admin_authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let Some(database) = &state.db else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        );
    };
    match database.usage_summary().await {
        Ok((requests, successes, input_tokens, output_tokens)) => (StatusCode::OK, Json(json!({"requests":requests,"successes":successes,"failures":requests-successes,"input_tokens":input_tokens,"output_tokens":output_tokens,"total_tokens":input_tokens+output_tokens}))).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "usage_summary_failed", &error.to_string()),
    }
}

async fn proxy(
    state: AppState,
    headers: HeaderMap,
    body: Bytes,
    protocol: Protocol,
) -> Response<Body> {
    if std::env::var("GATEWAY_API_KEY").is_ok() && !authorized(&headers) && state.db.is_none() {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or invalid gateway key",
        );
    }
    let started = Instant::now();
    let request_id = Uuid::new_v4().to_string();
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "request body must be valid JSON",
            )
        }
    };
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("default");
    if !authorized_with_db(&state, &headers, model).await {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "invalid or revoked virtual key",
        );
    }
    let Some(route) = state.resolver.resolve(protocol, model) else {
        return error_response(
            StatusCode::NOT_FOUND,
            "route_not_found",
            "no route matches protocol and model",
        );
    };
    let Some(provider) = state.config.provider(&route.provider_id) else {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "provider_not_found",
            "route references an unknown provider",
        );
    };
    if route.mode == "native" && !provider.native_protocols.contains(&protocol) {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported_protocol",
            "provider does not natively support this protocol and route has no adapter",
        );
    }
    let Some(account) = state.config.account(&route.primary_account_id) else {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "account_not_found",
            "route references an unknown account",
        );
    };
    if !account.enabled {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "account_disabled",
            "primary account is disabled",
        );
    }
    let result = forward_account(
        &state,
        &route,
        provider,
        account,
        protocol,
        &headers,
        body.clone(),
    )
    .await;
    let response = match result {
        Ok(response) if is_retryable(response.status()) => {
            try_fallback(&state, &route, provider, protocol, &headers, body, response).await
        }
        Ok(response) => response,
        Err(error) => {
            try_fallback_error(&state, &route, provider, protocol, &headers, body, error).await
        }
    };
    if let Some(database) = &state.db {
        let event = db::UsageEvent {
            request_id,
            provider_id: route.provider_id.clone(),
            account_id: route.primary_account_id.clone(),
            model: model.to_string(),
            protocol_in: protocol.to_string(),
            protocol_upstream: protocol.to_string(),
            mode: route.mode.clone(),
            status_code: response.status().as_u16() as i32,
            success: response.status().is_success(),
            retry_count: 0,
            latency_ms: started.elapsed().as_millis() as i64,
            ttft_ms: None,
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            cached_tokens: 0,
            total_tokens: 0,
            usage_source: "missing".into(),
            degraded: route.allow_lossy_conversion,
        };
        if let Err(error) = database.insert_usage(&event).await {
            tracing::warn!(%error, "failed to persist usage event");
        }
    }
    response
}

async fn forward_account(
    state: &AppState,
    route: &ResolvedRoute,
    provider: &config::ProviderConfig,
    account: &config::AccountConfig,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = state.config.credential_for(account);
    if route.mode == "adapter" {
        if route.adapter.as_deref() == Some("kimi_responses_adapter") {
            return embedded_kimi_adapter(provider, account, credential.as_deref(), headers, body)
                .await;
        }
        Err(transport::TransportError::Request(
            "unknown embedded adapter".into(),
        ))
    } else {
        transport::forward(
            &state.http,
            provider,
            account,
            credential.as_deref(),
            protocol,
            headers,
            body,
        )
        .await
    }
}

async fn embedded_kimi_adapter(
    provider: &config::ProviderConfig,
    _account: &config::AccountConfig,
    credential: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, transport::TransportError> {
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
    };
    let adapter = kimi_responses_adapter::adapter::server::router(cfg);
    let mut request = Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .body(Body::from(body))
        .map_err(|e| transport::TransportError::Request(e.to_string()))?;
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
    adapter
        .oneshot(request)
        .await
        .map_err(|error| transport::TransportError::Request(error.to_string()))
}

async fn try_fallback(
    state: &AppState,
    route: &ResolvedRoute,
    provider: &config::ProviderConfig,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    first: Response<Body>,
) -> Response<Body> {
    if route.fallback_accounts.is_empty() {
        return first;
    }
    let mut candidates: Vec<&config::AccountConfig> = route
        .fallback_accounts
        .iter()
        .filter_map(|id| state.config.account(id))
        .filter(|a| a.enabled && a.provider_id == provider.id)
        .collect();
    if candidates.is_empty() {
        return first;
    }
    let total: u32 = candidates.iter().map(|a| a.weight.max(1)).sum();
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
        % total.max(1);
    let mut cursor = 0;
    let mut selected = candidates[0];
    for candidate in candidates.drain(..) {
        cursor += candidate.weight.max(1);
        if tick < cursor {
            selected = candidate;
            break;
        }
    }
    match forward_account(state, route, provider, selected, protocol, headers, body).await {
        Ok(response) => response,
        Err(_) => first,
    }
}

async fn try_fallback_error(
    state: &AppState,
    route: &ResolvedRoute,
    provider: &config::ProviderConfig,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    first_error: transport::TransportError,
) -> Response<Body> {
    let Some(account_id) = route.fallback_accounts.first() else {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "upstream_request_failed",
            first_error.message(),
        );
    };
    let Some(account) = state.config.account(account_id) else {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "upstream_request_failed",
            first_error.message(),
        );
    };
    match forward_account(state, route, provider, account, protocol, headers, body).await {
        Ok(response) => response,
        Err(_) => error_response(
            StatusCode::BAD_GATEWAY,
            "upstream_request_failed",
            first_error.message(),
        ),
    }
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn authorized(headers: &HeaderMap) -> bool {
    let Ok(expected) = std::env::var("GATEWAY_API_KEY") else {
        return true;
    };
    let bearer = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let supplied = bearer.or_else(|| {
        headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
    });
    supplied == Some(expected.as_str())
}

async fn authorized_with_db(state: &AppState, headers: &HeaderMap, model: &str) -> bool {
    if let Ok(expected) = std::env::var("GATEWAY_API_KEY") {
        if supplied_key(headers) == Some(expected.as_str()) {
            return true;
        }
    }
    let Some(database) = &state.db else {
        return std::env::var("GATEWAY_API_KEY").is_err();
    };
    let Some(key) = supplied_key(headers) else {
        return false;
    };
    database
        .authenticate_virtual_key(key, model)
        .await
        .unwrap_or(false)
}

fn supplied_key(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
        })
}

fn admin_authorized(headers: &HeaderMap) -> bool {
    let expected = std::env::var("GATEWAY_ADMIN_KEY").or_else(|_| std::env::var("GATEWAY_API_KEY"));
    let Ok(expected) = expected else {
        return true;
    };
    supplied_key(headers) == Some(expected.as_str())
}

fn error_response(status: StatusCode, kind: &str, message: &str) -> Response<Body> {
    (
        status,
        Json(json!({"error":{"type":kind,"message":message}})),
    )
        .into_response()
}

async fn resolve_route(
    State(state): State<AppState>,
    Path((protocol, model)): Path<(String, String)>,
) -> impl IntoResponse {
    let Ok(protocol) = protocol.parse::<Protocol>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"unknown protocol"})),
        );
    };
    match state.resolver.resolve(protocol, &model) {
        Some(route) => (StatusCode::OK, Json(json!(route))),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"route not found"})),
        ),
    }
}
