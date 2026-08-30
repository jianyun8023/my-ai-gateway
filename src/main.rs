mod config;
mod db;
mod health;
mod model_catalog;
mod protocol;
mod routing;
mod transport;
mod usage;

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
use futures_util::{stream, StreamExt};
use protocol::Protocol;
use routing::{ResolvedRoute, RouteResolver};
use serde::Deserialize;
use serde_json::{json, Value};
use tower::ServiceExt;
use tower_http::{services::ServeDir, trace::TraceLayer};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    config: Arc<GatewayConfig>,
    resolver: RouteResolver,
    http: reqwest::Client,
    db: Option<db::Database>,
    health: health::HealthRegistry,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let config = Arc::new(GatewayConfig::from_env());
    if let Err(errors) = config.validate() {
        for error in errors {
            tracing::error!(%error, "invalid gateway configuration");
        }
        return Err("invalid gateway configuration".into());
    }
    let addr: SocketAddr = config.listen_addr.parse()?;
    let db = db::Database::connect_from_env().await?;
    if let Some(database) = &db {
        database.sync_control_plane(&config).await?;
    }
    let state = AppState {
        resolver: RouteResolver::new(config.clone()),
        config,
        http: transport::client()?,
        db,
        health: health::HealthRegistry::new(std::time::Duration::from_secs(30)),
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
        .route("/admin/usage/events", get(usage_events))
        .route("/admin/usage/aggregate", get(usage_aggregate))
        .route("/admin/providers", get(admin_providers))
        .route("/admin/accounts", get(admin_accounts))
        .route("/admin/routes", get(admin_routes))
        .route("/admin/routes/:protocol/:model", get(resolve_route))
        .nest_service("/admin", ServeDir::new("web/dist"))
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

async fn usage_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
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
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(100);
    match database.list_usage_events(limit).await {
        Ok(events) => (
            StatusCode::OK,
            Json(json!({"data":events,"limit":limit.clamp(1,500)})),
        )
            .into_response(),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_events_failed",
            &error.to_string(),
        ),
    }
}

async fn usage_aggregate(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
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
    let filter = db::UsageFilter {
        from: query
            .get("from")
            .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
            .map(|v| v.with_timezone(&chrono::Utc)),
        to: query
            .get("to")
            .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
            .map(|v| v.with_timezone(&chrono::Utc)),
        model: query.get("model").cloned(),
        provider_id: query.get("provider").cloned(),
        account_id: query.get("account").cloned(),
        protocol: query.get("protocol").cloned(),
        source: query.get("source").cloned(),
    };
    let granularity = query
        .get("granularity")
        .map(String::as_str)
        .unwrap_or("hour");
    let dimension = query
        .get("breakdown")
        .map(String::as_str)
        .unwrap_or("model");
    let aggregate = match database.usage_aggregate(&filter).await {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_aggregate_failed",
                &error.to_string(),
            )
        }
    };
    let timeseries = match database.usage_timeseries(&filter, granularity).await {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_timeseries_failed",
                &error.to_string(),
            )
        }
    };
    let breakdown = match database.usage_breakdown(&filter, dimension).await {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_breakdown_failed",
                &error.to_string(),
            )
        }
    };
    (StatusCode::OK, Json(json!({"timezone":"UTC","aggregate":aggregate,"timeseries":timeseries,"breakdown_dimension":dimension,"breakdown":breakdown}))).into_response()
}

async fn admin_providers(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if !admin_authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    (
        StatusCode::OK,
        Json(json!({"data": state.config.providers})),
    )
        .into_response()
}

async fn admin_accounts(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if !admin_authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let accounts: Vec<Value> = state.config.accounts.iter().map(|account| json!({"id":account.id,"provider_id":account.provider_id,"display_name":account.display_name,"enabled":account.enabled,"weight":account.weight})).collect();
    (StatusCode::OK, Json(json!({"data": accounts}))).into_response()
}

async fn admin_routes(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if !admin_authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    (StatusCode::OK, Json(json!({"data": state.config.routes}))).into_response()
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
    if !state.health.is_available(&account.id).await {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "account_cooling_down",
            "primary account is cooling down",
        );
    }
    let usage_request_body = body.clone();
    let result_started = Instant::now();
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
    let mut attempts = Vec::new();
    let response = match result {
        Ok(response) if is_retryable(response.status()) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 0,
                provider_id: provider.id.clone(),
                account_id: account.id.clone(),
                upstream_model_id: None,
                status_code: response.status().as_u16() as i32,
                success: false,
                latency_ms: result_started.elapsed().as_millis() as i64,
            });
            state.health.mark_failure(&account.id).await;
            let (response, mut fallback_attempts) =
                try_fallback(&state, &route, provider, protocol, &headers, body, response).await;
            attempts.append(&mut fallback_attempts);
            response
        }
        Ok(response) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 0,
                provider_id: provider.id.clone(),
                account_id: account.id.clone(),
                upstream_model_id: None,
                status_code: response.status().as_u16() as i32,
                success: response.status().is_success(),
                latency_ms: result_started.elapsed().as_millis() as i64,
            });
            state.health.mark_success(&account.id).await;
            response
        }
        Err(error) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 0,
                provider_id: provider.id.clone(),
                account_id: account.id.clone(),
                upstream_model_id: None,
                status_code: 599,
                success: false,
                latency_ms: result_started.elapsed().as_millis() as i64,
            });
            state.health.mark_failure(&account.id).await;
            let (response, mut fallback_attempts) =
                try_fallback_error(&state, &route, provider, protocol, &headers, body, error).await;
            attempts.append(&mut fallback_attempts);
            response
        }
    };
    let usage = transport::usage_from_response(&response);
    if let Some(database) = &state.db {
        let source = headers
            .get("x-client-source")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown")
            .to_string();
        let final_account_id = attempts
            .iter()
            .rev()
            .find(|attempt| attempt.success)
            .map(|attempt| attempt.account_id.clone())
            .unwrap_or_else(|| account.id.clone());
        let event = db::UsageEvent {
            request_id,
            provider_id: route.provider_id.clone(),
            account_id: final_account_id,
            model: model.to_string(),
            logical_model: model.to_string(),
            upstream_model_id: None,
            source,
            protocol_in: protocol.to_string(),
            protocol_upstream: route.protocol_upstream.to_string(),
            mode: route.mode.clone(),
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
            degraded: route.allow_lossy_conversion,
        };
        if is_event_stream(&response) {
            return wrap_stream_usage(
                response,
                database.clone(),
                event,
                usage_request_body,
                attempts,
            );
        }
        if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await {
            tracing::warn!(%error, "failed to persist usage event");
        }
    }
    response
}

fn is_event_stream(response: &Response<Body>) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}

fn wrap_stream_usage(
    response: Response<Body>,
    database: db::Database,
    event: db::UsageEvent,
    request_body: Bytes,
    attempts: Vec<db::UsageAttempt>,
) -> Response<Body> {
    let (parts, body) = response.into_parts();
    let upstream = body.into_data_stream();
    let captured = Vec::new();
    let stream = stream::unfold(
        (upstream, captured, database, event, request_body, attempts),
        |(mut upstream, mut captured, database, mut event, request_body, attempts)| async move {
            match upstream.next().await {
                Some(Ok(chunk)) => {
                    captured.extend_from_slice(&chunk);
                    Some((
                        Ok::<Bytes, std::io::Error>(chunk),
                        (upstream, captured, database, event, request_body, attempts),
                    ))
                }
                Some(Err(error)) => Some((
                    Err(std::io::Error::other(error.to_string())),
                    (upstream, captured, database, event, request_body, attempts),
                )),
                None => {
                    if let Some(usage) =
                        crate::usage::extract_sse(&String::from_utf8_lossy(&captured))
                    {
                        event.input_tokens = usage.input_tokens;
                        event.output_tokens = usage.output_tokens;
                        event.reasoning_tokens = usage.reasoning_tokens;
                        event.cached_tokens = usage.cached_tokens;
                        event.total_tokens = usage.total_tokens;
                        event.usage_source = usage.source;
                    } else {
                        let usage = crate::usage::estimate(&request_body, &captured);
                        event.input_tokens = usage.input_tokens;
                        event.output_tokens = usage.output_tokens;
                        event.total_tokens = usage.total_tokens;
                        event.usage_source = usage.source;
                    }
                    tokio::spawn(async move {
                        if let Err(error) =
                            database.insert_usage_with_attempts(&event, &attempts).await
                        {
                            tracing::warn!(%error, "failed to persist streaming usage event");
                        }
                    });
                    None
                }
            }
        },
    );
    Response::from_parts(parts, Body::from_stream(stream))
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
) -> (Response<Body>, Vec<db::UsageAttempt>) {
    let mut attempts = Vec::new();
    if route.fallback_accounts.is_empty() {
        return (first, attempts);
    }
    let candidates: Vec<&config::AccountConfig> = route
        .fallback_accounts
        .iter()
        .filter_map(|id| state.config.account(id))
        .filter(|a| a.enabled && a.provider_id == provider.id)
        .collect();
    let mut available = Vec::new();
    for candidate in candidates {
        if state.health.is_available(&candidate.id).await {
            available.push(candidate);
        }
    }
    let mut candidates = available;
    if candidates.is_empty() {
        return (first, attempts);
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
    let started = Instant::now();
    match forward_account(state, route, provider, selected, protocol, headers, body).await {
        Ok(response) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: provider.id.clone(),
                account_id: selected.id.clone(),
                upstream_model_id: None,
                status_code: response.status().as_u16() as i32,
                success: response.status().is_success(),
                latency_ms: started.elapsed().as_millis() as i64,
            });
            if is_retryable(response.status()) {
                state.health.mark_failure(&selected.id).await;
            } else {
                state.health.mark_success(&selected.id).await;
            }
            (response, attempts)
        }
        Err(_) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: provider.id.clone(),
                account_id: selected.id.clone(),
                upstream_model_id: None,
                status_code: 599,
                success: false,
                latency_ms: started.elapsed().as_millis() as i64,
            });
            state.health.mark_failure(&selected.id).await;
            (first, attempts)
        }
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
) -> (Response<Body>, Vec<db::UsageAttempt>) {
    let mut attempts = Vec::new();
    let Some(account_id) = route.fallback_accounts.first() else {
        return (
            error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_request_failed",
                first_error.message(),
            ),
            attempts,
        );
    };
    let Some(account) = state.config.account(account_id) else {
        return (
            error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_request_failed",
                first_error.message(),
            ),
            attempts,
        );
    };
    let started = Instant::now();
    match forward_account(state, route, provider, account, protocol, headers, body).await {
        Ok(response) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: provider.id.clone(),
                account_id: account.id.clone(),
                upstream_model_id: None,
                status_code: response.status().as_u16() as i32,
                success: response.status().is_success(),
                latency_ms: started.elapsed().as_millis() as i64,
            });
            (response, attempts)
        }
        Err(_) => (
            error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_request_failed",
                first_error.message(),
            ),
            attempts,
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
    match state.resolver.resolve_detailed(protocol, &model) {
        Ok(route) => (StatusCode::OK, Json(json!(route))),
        Err(error) => {
            let status = if error.code == "route_not_found" {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            (status, Json(json!({"error": error})))
        }
    }
}

#[cfg(test)]
mod kimi_adapter_e2e_tests {
    use super::*;
    use axum::{
        body::to_bytes,
        extract::Request,
        http::{header, HeaderMap},
        Router,
    };
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    const ANTHROPIC_STREAM: &str = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_e2e\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello from kimi\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    type RecordedBody = Arc<Mutex<String>>;

    async fn spawn_mock_upstream<F>(handler: F) -> (String, RecordedBody)
    where
        F: Fn(&str, &HeaderMap) -> Response<Body> + Send + Sync + 'static,
    {
        let recorded = Arc::new(Mutex::new(String::new()));
        let handler = Arc::new(handler);
        let app = Router::new().fallback({
            let recorded = recorded.clone();
            move |request: Request| {
                let recorded = recorded.clone();
                let handler = handler.clone();
                async move {
                    let (parts, body) = request.into_parts();
                    let bytes = to_bytes(body, 16 * 1024 * 1024).await.unwrap_or_default();
                    let body = String::from_utf8_lossy(&bytes).to_string();
                    *recorded.lock().expect("recorded body mutex") = body.clone();
                    handler(&body, &parts.headers)
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock upstream");
        let addr = listener.local_addr().expect("mock upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock upstream server")
        });
        (format!("http://{addr}"), recorded)
    }

    fn test_state(base_url: String) -> AppState {
        let config = GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![config::ProviderConfig {
                id: "kimi".into(),
                name: "Kimi Code".into(),
                base_url,
                models: vec!["k3".into()],
                native_protocols: vec![
                    Protocol::AnthropicMessages,
                    Protocol::OpenAiChatCompletions,
                ],
                endpoints: HashMap::from([
                    (
                        Protocol::OpenAiChatCompletions,
                        "/v1/chat/completions".into(),
                    ),
                    (Protocol::AnthropicMessages, "/v1/messages".into()),
                ]),
                capabilities: config::Capabilities {
                    streaming: config::CapabilityMode::Native,
                    tools: config::CapabilityMode::Native,
                    thinking: config::CapabilityMode::Native,
                    web_search: config::CapabilityMode::Native,
                    usage: config::CapabilityMode::Native,
                    ..Default::default()
                },
                protocol_capabilities: HashMap::from([(
                    Protocol::OpenAiResponses,
                    config::ProtocolCapability::adapter(
                        Protocol::AnthropicMessages,
                        "kimi_responses_adapter",
                    ),
                )]),
                model_overrides: HashMap::new(),
            }],
            accounts: vec![config::AccountConfig {
                id: "kimi-account".into(),
                provider_id: "kimi".into(),
                display_name: "Kimi test account".into(),
                credential_env: None,
                credential: Some("upstream-test-key".into()),
                enabled: true,
                weight: 100,
                protocol_capabilities: HashMap::new(),
                capabilities: None,
                model_overrides: HashMap::new(),
            }],
            routes: vec![config::RouteConfig {
                id: "kimi-responses-adapter".into(),
                model: "k3".into(),
                provider_id: "kimi".into(),
                protocols: vec![Protocol::OpenAiResponses],
                primary_account_id: "kimi-account".into(),
                fallback_accounts: vec![],
                strategy: "primary_then_weighted_fallback".into(),
                mode: "adapter".into(),
                adapter: Some("kimi_responses_adapter".into()),
                allow_lossy_conversion: false,
            }],
        };
        let config = Arc::new(config);
        AppState {
            resolver: RouteResolver::new(config.clone()),
            config,
            http: transport::client().expect("http client"),
            db: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
        }
    }

    async fn invoke_responses(state: AppState, body: &str) -> (StatusCode, String) {
        let response =
            responses(State(state), HeaderMap::new(), Bytes::from(body.to_owned())).await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .expect("response body");
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn embedded_kimi_adapter_non_stream_preserves_thinking_and_web_search() {
        let (base, recorded) = spawn_mock_upstream(|_, headers| {
            assert_eq!(
                headers.get("authorization").and_then(|v| v.to_str().ok()),
                Some("Bearer upstream-test-key")
            );
            Response::builder()
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"id":"msg_e2e","type":"message","role":"assistant","model":"k3","content":[{"type":"thinking","thinking":"reasoning","signature":"sig-e2e"},{"type":"text","text":"Search results for query: x"},{"type":"server_tool_use","name":"web_search"},{"type":"web_search_tool_result","content":[]},{"type":"text","text":"answer"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"cache_read_input_tokens":2,"output_tokens":4,"output_tokens_details":{"thinking_tokens":1}}}"#,
                ))
                .unwrap()
        })
        .await;
        let (status, body) = invoke_responses(
            test_state(base),
            r#"{"model":"k3","stream":false,"input":"hello"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        let response: Value = serde_json::from_str(&body).expect("responses JSON");
        assert_eq!(response["status"], "completed");
        let output = response["output"].as_array().expect("output array");
        assert!(output.iter().any(|item| item["type"] == "reasoning"));
        assert!(output.iter().any(|item| item["type"] == "web_search_call"));
        assert_eq!(response["usage"]["input_tokens"], 12);
        assert!(recorded
            .lock()
            .expect("recorded body mutex")
            .contains("messages"));
    }

    #[tokio::test]
    async fn embedded_kimi_adapter_stream_translates_sse_events() {
        let (base, recorded) = spawn_mock_upstream(|body, headers| {
            if body.is_empty() {
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::empty())
                    .unwrap();
            }
            assert!(
                body.contains("messages"),
                "adapter must send Anthropic request: {body}"
            );
            assert_eq!(
                headers.get("authorization").and_then(|v| v.to_str().ok()),
                Some("Bearer upstream-test-key")
            );
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(ANTHROPIC_STREAM))
                .unwrap()
        })
        .await;
        let (status, body) = invoke_responses(
            test_state(base),
            r#"{"model":"k3","stream":true,"input":"hello"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert!(
            body.contains("event: response.output_text.delta"),
            "missing text delta: {body}"
        );
        assert!(
            body.contains("hello from kimi"),
            "missing translated text: {body}"
        );
        assert!(
            body.contains("event: response.completed"),
            "missing completion event: {body}"
        );
        assert!(recorded
            .lock()
            .expect("recorded body mutex")
            .contains("messages"));
    }
}
