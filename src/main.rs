mod config;
mod db;
mod health;
mod model_catalog;
mod protocol;
mod routing;
mod transport;
mod usage;

use std::{
    fmt::Write as _,
    net::SocketAddr,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        HeaderMap, HeaderValue, Request, Response, StatusCode,
    },
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

struct LiveConfig {
    config: Arc<GatewayConfig>,
    resolver: RouteResolver,
}

#[derive(Clone)]
struct AppState {
    live: Arc<std::sync::RwLock<LiveConfig>>,
    http: reqwest::Client,
    db: Option<db::Database>,
    health: health::HealthRegistry,
    listen_addr: String,
}

impl AppState {
    fn config(&self) -> Arc<GatewayConfig> {
        self.live.read().unwrap().config.clone()
    }

    fn resolver(&self) -> RouteResolver {
        self.live.read().unwrap().resolver.clone()
    }

    fn reload_config(&self, config: GatewayConfig) {
        let config = Arc::new(config);
        let resolver = RouteResolver::new(config.clone());
        *self.live.write().unwrap() = LiveConfig { config, resolver };
    }
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
    let listen_addr = config.listen_addr.clone();
    let db = db::Database::connect_from_env().await?;
    if let Some(database) = &db {
        database.sync_control_plane(&config).await?;
    }
    let config = Arc::new(config.as_ref().clone());
    let live = LiveConfig {
        resolver: RouteResolver::new(config.clone()),
        config,
    };
    let state = AppState {
        live: Arc::new(std::sync::RwLock::new(live)),
        http: transport::client()?,
        db,
        health: health::HealthRegistry::new(std::time::Duration::from_secs(30)),
        listen_addr,
    };
    let app = application(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "AI gateway listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn application(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/responses", post(responses))
        .route("/v1/messages", post(messages))
        .route("/admin/keys", get(list_keys).post(create_key))
        .route("/admin/keys/{id}/revoke", post(revoke_key))
        .route("/admin/usage/summary", get(usage_summary))
        .route("/admin/usage/timeseries", get(usage_timeseries))
        .route("/admin/usage/breakdown", get(usage_breakdown))
        .route("/admin/usage/events", get(usage_events))
        .route("/admin/usage/export", get(usage_export))
        .route("/admin/usage/aggregate", get(usage_aggregate))
        .route("/admin/usage/events/{request_id}", get(usage_event_detail))
        .route(
            "/admin/providers",
            get(admin_providers).post(create_or_update_provider),
        )
        .route(
            "/admin/providers/{id}",
            axum::routing::delete(delete_provider),
        )
        .route(
            "/admin/accounts",
            get(admin_accounts).post(create_or_update_account),
        )
        .route(
            "/admin/accounts/{id}",
            axum::routing::delete(delete_account),
        )
        .route(
            "/admin/routes",
            get(admin_routes).post(create_or_update_route),
        )
        .route(
            "/admin/routes/{id}",
            axum::routing::delete(delete_route_by_id),
        )
        .route("/admin/config/reload", post(reload_config))
        .route("/admin/capabilities", get(admin_capabilities))
        .route("/admin/health", get(admin_health))
        .route("/admin/routes/{protocol}/{model}", get(resolve_route))
        .nest_service("/admin", ServeDir::new("web/dist"))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

async fn healthz(State(state): State<AppState>) -> Json<Value> {
    let config = state.config();
    Json(
        json!({"status":"ok", "providers":config.providers.len(), "accounts":config.accounts.len()}),
    )
}

async fn models(State(state): State<AppState>) -> Json<Value> {
    let config = state.config();
    let data: Vec<Value> = config
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

#[derive(Debug)]
struct UsageQuery {
    filter: db::UsageFilter,
    granularity: String,
    breakdown: String,
    limit: i64,
    cursor: Option<db::UsageCursor>,
    format: String,
}

fn parse_usage_query(
    query: &std::collections::HashMap<String, String>,
) -> Result<UsageQuery, String> {
    let parse_time = |name: &str| -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
        query
            .get(name)
            .map(|value| {
                chrono::DateTime::parse_from_rfc3339(value)
                    .map(|time| time.with_timezone(&chrono::Utc))
                    .map_err(|_| format!("{name} must be an RFC3339 timestamp"))
            })
            .transpose()
    };
    let from = parse_time("from")?;
    let to = parse_time("to")?;
    if from.zip(to).is_some_and(|(from, to)| from >= to) {
        return Err("from must be earlier than to".into());
    }
    let virtual_key_id = query
        .get("virtual_key")
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| "virtual_key must be a positive integer".to_string())
        })
        .transpose()?;
    let status_code = query
        .get("status_code")
        .map(|value| {
            value
                .parse::<i32>()
                .ok()
                .filter(|code| (100..=599).contains(code))
                .ok_or_else(|| "status_code must be between 100 and 599".to_string())
        })
        .transpose()?;
    let success = match query.get("status").map(String::as_str) {
        None => None,
        Some("success") => Some(true),
        Some("failure") => Some(false),
        Some(_) => return Err("status must be success or failure".into()),
    };
    if let Some(value) = query.get("usage_source") {
        if !matches!(
            value.as_str(),
            "upstream" | "parsed" | "estimated" | "missing"
        ) {
            return Err("usage_source must be upstream, parsed, estimated, or missing".into());
        }
    }
    let granularity = query
        .get("granularity")
        .cloned()
        .unwrap_or_else(|| "hour".into());
    if !matches!(granularity.as_str(), "hour" | "day") {
        return Err("granularity must be hour or day".into());
    }
    let breakdown = query
        .get("breakdown")
        .cloned()
        .unwrap_or_else(|| "logical_model".into());
    if !matches!(
        breakdown.as_str(),
        "logical_model"
            | "upstream_model"
            | "provider"
            | "source"
            | "account"
            | "protocol_in"
            | "protocol_upstream"
            | "virtual_key"
            | "status"
            | "usage_source"
    ) {
        return Err("unsupported breakdown dimension".into());
    }
    let limit = query
        .get("limit")
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|limit| (1..=500).contains(limit))
                .ok_or_else(|| "limit must be between 1 and 500".to_string())
        })
        .transpose()?
        .unwrap_or(100);
    let cursor = query
        .get("cursor")
        .map(|value| db::UsageCursor::decode(value).ok_or_else(|| "cursor is invalid".to_string()))
        .transpose()?;
    let format = query
        .get("format")
        .cloned()
        .unwrap_or_else(|| "json".into());
    if !matches!(format.as_str(), "json" | "csv") {
        return Err("format must be json or csv".into());
    }
    Ok(UsageQuery {
        filter: db::UsageFilter {
            from,
            to,
            logical_model: query.get("logical_model").cloned(),
            upstream_model_id: query.get("upstream_model").cloned(),
            provider_id: query.get("provider").cloned(),
            source: query.get("source").cloned(),
            account_id: query.get("account").cloned(),
            protocol_in: query.get("protocol_in").cloned(),
            protocol_upstream: query.get("protocol_upstream").cloned(),
            virtual_key_id,
            success,
            status_code,
            usage_source: query.get("usage_source").cloned(),
        },
        granularity,
        breakdown,
        limit,
        cursor,
        format,
    })
}

fn usage_range(filter: &db::UsageFilter) -> Value {
    json!({
        "from": filter.from.as_ref().map(chrono::DateTime::to_rfc3339),
        "to": filter.to.as_ref().map(chrono::DateTime::to_rfc3339),
        "boundary": "[from,to)"
    })
}

fn invalid_usage_query(message: &str) -> Response<Body> {
    error_response(StatusCode::BAD_REQUEST, "invalid_usage_query", message)
}

async fn usage_summary(
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    match database.usage_aggregate(&query.filter).await {
        Ok(data) => (StatusCode::OK, Json(json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"data":data}))).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "usage_summary_failed", &error.to_string()),
    }
}

async fn usage_timeseries(
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    match database.usage_timeseries(&query.filter, &query.granularity).await {
        Ok(data) => (StatusCode::OK, Json(json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"granularity":query.granularity,"data":data}))).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "usage_timeseries_failed", &error.to_string()),
    }
}

async fn usage_breakdown(
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    match database.usage_breakdown(&query.filter, &query.breakdown).await {
        Ok(data) => (StatusCode::OK, Json(json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"dimension":query.breakdown,"data":data}))).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "usage_breakdown_failed", &error.to_string()),
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    match database
        .list_usage_events_page(&query.filter, query.limit, query.cursor.as_ref())
        .await
    {
        Ok(page) => (StatusCode::OK, Json(json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"data":page.data,"page":{"limit":query.limit,"has_more":page.has_more,"next_cursor":page.next_cursor}}))).into_response(),
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    let aggregate = match database.usage_aggregate(&query.filter).await {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_aggregate_failed",
                &error.to_string(),
            )
        }
    };
    let timeseries = match database
        .usage_timeseries(&query.filter, &query.granularity)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_timeseries_failed",
                &error.to_string(),
            )
        }
    };
    let breakdown = match database
        .usage_breakdown(&query.filter, &query.breakdown)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_breakdown_failed",
                &error.to_string(),
            )
        }
    };
    (StatusCode::OK, Json(json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"granularity":query.granularity,"breakdown_dimension":query.breakdown,"aggregate":aggregate,"timeseries":timeseries,"breakdown":breakdown}))).into_response()
}

async fn usage_export(
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    let export_limit: i64 = 10_000;
    let events = match database
        .export_usage_events(&query.filter, export_limit + 1)
        .await
    {
        Ok(events) => events,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_export_failed",
                &error.to_string(),
            )
        }
    };
    if events.len() as i64 > export_limit {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "export_too_large",
            &format!("export exceeds {export_limit} rows; narrow the time range or add filters"),
        );
    }
    if query.format == "csv" {
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/csv; charset=utf-8")
            .header(CONTENT_DISPOSITION, "attachment; filename=usage-events.csv")
            .body(Body::from(usage_events_csv(&events)))
            .expect("valid CSV export response")
    } else {
        let payload = json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"data":events});
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/json")
            .header(
                CONTENT_DISPOSITION,
                "attachment; filename=usage-events.json",
            )
            .body(Body::from(
                serde_json::to_vec(&payload).expect("serializable usage export"),
            ))
            .expect("valid JSON export response")
    }
}

fn usage_events_csv(events: &[db::UsageEventRecord]) -> String {
    let mut output = String::from("request_id,created_at,virtual_key_id,logical_model,upstream_model_id,provider_id,source,account_id,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,ttft_ms,input_tokens,output_tokens,reasoning_tokens,cached_tokens,total_tokens,usage_source,degraded,route_id,streamed,error_summary\n");
    for event in events {
        let values = [
            event.request_id.clone(),
            event.created_at.to_rfc3339(),
            event
                .virtual_key_id
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event.logical_model.clone(),
            event.upstream_model_id.clone().unwrap_or_default(),
            event.provider_id.clone(),
            event.source.clone(),
            event.account_id.clone(),
            event.protocol_in.clone(),
            event.protocol_upstream.clone(),
            event.mode.clone(),
            event.status_code.to_string(),
            event.success.to_string(),
            event.retry_count.to_string(),
            event.latency_ms.to_string(),
            event
                .ttft_ms
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event.input_tokens.to_string(),
            event.output_tokens.to_string(),
            event.reasoning_tokens.to_string(),
            event.cached_tokens.to_string(),
            event.total_tokens.to_string(),
            event.usage_source.clone(),
            event.degraded.to_string(),
            event.route_id.clone().unwrap_or_default(),
            event.streamed.to_string(),
            event.error_summary.clone().unwrap_or_default(),
        ];
        let line = values
            .iter()
            .map(|value| csv_field(value))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(output, "{line}").expect("writing to String cannot fail");
    }
    output
}

fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
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
        Json(json!({"data": state.config().providers})),
    )
        .into_response()
}

async fn usage_event_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
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
    let event = match database.get_usage_event_detail(&request_id).await {
        Ok(Some(event)) => event,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "event_not_found",
                "usage event not found",
            )
        }
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "event_detail_failed",
                &error.to_string(),
            )
        }
    };
    let attempts = match database.list_attempts_for_event(&request_id).await {
        Ok(attempts) => attempts,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "event_detail_failed",
                &error.to_string(),
            )
        }
    };
    (
        StatusCode::OK,
        Json(json!({"version":"v1","data":event,"attempts":attempts})),
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
    let accounts: Vec<Value> = state.config().accounts.iter().map(|account| json!({"id":account.id,"provider_id":account.provider_id,"display_name":account.display_name,"enabled":account.enabled,"weight":account.weight})).collect();
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
    (StatusCode::OK, Json(json!({"data": state.config().routes}))).into_response()
}

async fn create_or_update_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(provider): Json<config::ProviderConfig>,
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
    if let Err(error) = database.upsert_provider(&provider).await {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "db_error",
            &error.to_string(),
        );
    }
    reload_config_inner(&state).await
}

async fn delete_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
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
    match database.delete_provider(&id).await {
        Ok(true) => {}
        Ok(false) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "provider not found")
        }
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                &error.to_string(),
            )
        }
    }
    reload_config_inner(&state).await
}

async fn create_or_update_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(account): Json<config::AccountConfig>,
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
    if let Err(error) = database.upsert_account(&account).await {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "db_error",
            &error.to_string(),
        );
    }
    reload_config_inner(&state).await
}

async fn delete_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
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
    match database.delete_account(&id).await {
        Ok(true) => {}
        Ok(false) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "account not found")
        }
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                &error.to_string(),
            )
        }
    }
    reload_config_inner(&state).await
}

async fn create_or_update_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(route): Json<config::RouteConfig>,
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
    if let Err(error) = database.upsert_route(&route).await {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "db_error",
            &error.to_string(),
        );
    }
    reload_config_inner(&state).await
}

async fn delete_route_by_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
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
    match database.delete_route(&id).await {
        Ok(true) => {}
        Ok(false) => return error_response(StatusCode::NOT_FOUND, "not_found", "route not found"),
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                &error.to_string(),
            )
        }
    }
    reload_config_inner(&state).await
}

async fn admin_health(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if !admin_authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let health_map = state.health.all_health().await;
    let config = state.config();
    let mut data = Vec::new();
    for account in &config.accounts {
        let health = match health_map.get(&account.id) {
            Some(h) => h.clone(),
            None => health::AccountHealth {
                available: account.enabled,
                consecutive_failures: 0,
                cooldown_remaining_ms: 0,
            },
        };
        data.push(json!({
            "account_id": account.id,
            "provider_id": account.provider_id,
            "display_name": account.display_name,
            "enabled": account.enabled,
            "health": health,
        }));
    }
    (StatusCode::OK, Json(json!({"data": data}))).into_response()
}

async fn admin_capabilities(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if !admin_authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let config = state.config();
    let mut result = Vec::new();
    for route in &config.routes {
        let model = &route.model;
        let provider_id = &route.provider_id;
        let account_id = &route.primary_account_id;
        let protocol_caps =
            config.effective_protocol_capabilities(provider_id, Some(account_id), model);
        let feature_caps = config.capabilities(provider_id, Some(account_id), model);
        result.push(json!({
            "route_id": route.id,
            "model": model,
            "provider_id": provider_id,
            "account_id": account_id,
            "mode": route.mode,
            "protocols": route.protocols,
            "protocol_capabilities": protocol_caps,
            "capabilities": feature_caps,
        }));
    }
    (StatusCode::OK, Json(json!({"data": result}))).into_response()
}

async fn reload_config(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if !admin_authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    reload_config_inner(&state).await
}

async fn reload_config_inner(state: &AppState) -> Response<Body> {
    let Some(database) = &state.db else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        );
    };
    match database.load_gateway_config(&state.listen_addr).await {
        Ok(new_config) => {
            if let Err(errors) = new_config.validate() {
                let msg = errors.join("; ");
                return error_response(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed", &msg);
            }
            state.reload_config(new_config);
            (StatusCode::OK, Json(json!({"status":"reloaded"}))).into_response()
        }
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "reload_failed",
            &error.to_string(),
        ),
    }
}

async fn proxy(
    state: AppState,
    headers: HeaderMap,
    body: Bytes,
    protocol: Protocol,
) -> Response<Body> {
    let started = Instant::now();
    let request_id = Uuid::new_v4().to_string();
    let config = state.config();
    let resolver = state.resolver();
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
    let virtual_key_id = match authorized_with_db(&state, &headers, model).await {
        Some(virtual_key_id) => virtual_key_id,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "invalid or revoked virtual key",
            )
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
            return error_response(status, &error.code, &error.message);
        }
    };
    warn_degraded_route(&request_id, &route);
    let Some(provider) = config.provider(&route.provider_id) else {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "provider_not_found",
            "route references an unknown provider",
        );
    };
    let Some(account) = config.account(&route.primary_account_id) else {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "account_not_found",
            "route references an unknown account",
        );
    };
    let primary_unavailable = !account.enabled || !state.health.is_available(&account.id).await;
    if primary_unavailable {
        if let Some(candidate) =
            select_fallback_candidate(&config, &state.health, &route, model, protocol).await
        {
            let forwarded_body = if candidate.upstream_model != model {
                rewrite_model_in_body(&body, &candidate.upstream_model)
            } else {
                body.clone()
            };
            let started = Instant::now();
            if let Ok(response) = forward_fallback(
                &config,
                &state.http,
                candidate.provider,
                candidate.account,
                protocol,
                &headers,
                forwarded_body,
            )
            .await
            {
                let usage = transport::usage_from_response(&response);
                let is_streamed = payload
                    .get("stream")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let degraded = route.is_degraded();
                let error_summary = if !response.status().is_success() {
                    Some(format!("HTTP {}", response.status().as_u16()))
                } else {
                    None
                };
                if let Some(database) = &state.db {
                    let source = headers
                        .get("x-client-source")
                        .and_then(|v| v.to_str().ok())
                        .filter(|v| !v.is_empty())
                        .unwrap_or("unknown")
                        .to_string();
                    let event = db::UsageEvent {
                        request_id,
                        virtual_key_id,
                        provider_id: candidate.provider.id.clone(),
                        account_id: candidate.account.id.clone(),
                        model: model.to_string(),
                        logical_model: model.to_string(),
                        upstream_model_id: Some(candidate.upstream_model.clone()),
                        source,
                        protocol_in: protocol.to_string(),
                        protocol_upstream: route.protocol_upstream.to_string(),
                        mode: route.mode.clone(),
                        status_code: response.status().as_u16() as i32,
                        success: response.status().is_success(),
                        retry_count: 0,
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
                        error_summary,
                    };
                    let attempts = vec![db::UsageAttempt {
                        attempt_no: 0,
                        provider_id: candidate.provider.id.clone(),
                        account_id: candidate.account.id.clone(),
                        upstream_model_id: Some(candidate.upstream_model),
                        status_code: response.status().as_u16() as i32,
                        success: response.status().is_success(),
                        latency_ms: started.elapsed().as_millis() as i64,
                    }];
                    if is_event_stream(&response) {
                        return wrap_stream_usage(
                            response,
                            database.clone(),
                            event,
                            body,
                            attempts,
                        );
                    }
                    if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await
                    {
                        tracing::warn!(%error, "failed to persist usage event");
                    }
                }
                return response;
            }
        }
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            if account.enabled {
                "account_cooling_down"
            } else {
                "account_disabled"
            },
            "primary account is unavailable and no fallback succeeded",
        );
    }
    let usage_request_body = body.clone();
    let result_started = Instant::now();
    let result = forward_account(
        &config,
        &state.http,
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
            let (response, mut fallback_attempts) = try_fallback(
                &config,
                &state.health,
                &state.http,
                &route,
                model,
                protocol,
                &headers,
                body,
                response,
            )
            .await;
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
            let (response, mut fallback_attempts) = try_fallback_error(
                &config,
                &state.health,
                &state.http,
                &route,
                model,
                protocol,
                &headers,
                body,
                error,
            )
            .await;
            attempts.append(&mut fallback_attempts);
            response
        }
    };
    let usage = transport::usage_from_response(&response);
    let is_streamed = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let degraded = route.is_degraded();
    let error_summary = if !response.status().is_success() {
        Some(format!("HTTP {}", response.status().as_u16()))
    } else {
        None
    };
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
            virtual_key_id,
            provider_id: route.provider_id.clone(),
            account_id: final_account_id,
            model: model.to_string(),
            logical_model: model.to_string(),
            upstream_model_id: Some(model.to_string()),
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
            );
        }
        if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await {
            tracing::warn!(%error, "failed to persist usage event");
        }
    }
    response
}

fn warn_degraded_route(request_id: &str, route: &ResolvedRoute) {
    if route.is_degraded() {
        tracing::warn!(
            request_id = %request_id,
            route_id = %route.route_id,
            degraded_features = ?route.degraded_features,
            "route has degraded features due to adapter conversion"
        );
    }
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
                    let usage = crate::usage::usage_for_sse_response(
                        event.success,
                        &request_body,
                        &captured,
                    );
                    event.input_tokens = usage.input_tokens;
                    event.output_tokens = usage.output_tokens;
                    event.reasoning_tokens = usage.reasoning_tokens;
                    event.cached_tokens = usage.cached_tokens;
                    event.total_tokens = usage.total_tokens;
                    event.usage_source = usage.source;
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

#[allow(clippy::too_many_arguments)]
async fn forward_account(
    config: &GatewayConfig,
    http: &reqwest::Client,
    route: &ResolvedRoute,
    provider: &config::ProviderConfig,
    account: &config::AccountConfig,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = config.credential_for(account);
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
            http,
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

struct FallbackCandidate<'a> {
    account: &'a config::AccountConfig,
    provider: &'a config::ProviderConfig,
    upstream_model: String,
}

async fn select_fallback_candidate<'a>(
    config: &'a GatewayConfig,
    health: &health::HealthRegistry,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
) -> Option<FallbackCandidate<'a>> {
    let mut available = Vec::new();
    for id in &route.fallback_accounts {
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
        if account.provider_id != route.provider_id {
            let cap = config.protocol_capability(&provider.id, Some(&account.id), model, protocol);
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
            upstream_model,
        });
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

fn rewrite_model_in_body(body: &Bytes, new_model: &str) -> Bytes {
    if let Ok(mut payload) = serde_json::from_slice::<Value>(body) {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("model".into(), Value::String(new_model.into()));
        }
        Bytes::from(serde_json::to_vec(&payload).unwrap_or_else(|_| body.to_vec()))
    } else {
        body.clone()
    }
}

#[allow(clippy::too_many_arguments)]
async fn try_fallback(
    config: &GatewayConfig,
    health: &health::HealthRegistry,
    http: &reqwest::Client,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    first: Response<Body>,
) -> (Response<Body>, Vec<db::UsageAttempt>) {
    let mut attempts = Vec::new();
    let Some(candidate) = select_fallback_candidate(config, health, route, model, protocol).await
    else {
        return (first, attempts);
    };
    let forwarded_body = if candidate.upstream_model != model {
        rewrite_model_in_body(&body, &candidate.upstream_model)
    } else {
        body
    };
    let started = Instant::now();
    match forward_fallback(
        config,
        http,
        candidate.provider,
        candidate.account,
        protocol,
        headers,
        forwarded_body,
    )
    .await
    {
        Ok(response) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider.id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(candidate.upstream_model),
                status_code: response.status().as_u16() as i32,
                success: response.status().is_success(),
                latency_ms: started.elapsed().as_millis() as i64,
            });
            if is_retryable(response.status()) {
                health.mark_failure(&candidate.account.id).await;
            } else {
                health.mark_success(&candidate.account.id).await;
            }
            (response, attempts)
        }
        Err(_) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider.id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(candidate.upstream_model),
                status_code: 599,
                success: false,
                latency_ms: started.elapsed().as_millis() as i64,
            });
            health.mark_failure(&candidate.account.id).await;
            (first, attempts)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn try_fallback_error(
    config: &GatewayConfig,
    health: &health::HealthRegistry,
    http: &reqwest::Client,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    first_error: transport::TransportError,
) -> (Response<Body>, Vec<db::UsageAttempt>) {
    let mut attempts = Vec::new();
    let Some(candidate) = select_fallback_candidate(config, health, route, model, protocol).await
    else {
        return (
            error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_request_failed",
                first_error.message(),
            ),
            attempts,
        );
    };
    let forwarded_body = if candidate.upstream_model != model {
        rewrite_model_in_body(&body, &candidate.upstream_model)
    } else {
        body
    };
    let started = Instant::now();
    match forward_fallback(
        config,
        http,
        candidate.provider,
        candidate.account,
        protocol,
        headers,
        forwarded_body,
    )
    .await
    {
        Ok(response) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider.id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(candidate.upstream_model),
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

async fn forward_fallback(
    config: &GatewayConfig,
    http: &reqwest::Client,
    provider: &config::ProviderConfig,
    account: &config::AccountConfig,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = config.credential_for(account);
    transport::forward(
        http,
        provider,
        account,
        credential.as_deref(),
        protocol,
        headers,
        body,
    )
    .await
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

async fn authorized_with_db(
    state: &AppState,
    headers: &HeaderMap,
    model: &str,
) -> Option<Option<i64>> {
    if let Ok(expected) = std::env::var("GATEWAY_API_KEY") {
        if supplied_key(headers) == Some(expected.as_str()) {
            return Some(None);
        }
    }
    let Some(database) = &state.db else {
        return std::env::var("GATEWAY_API_KEY").is_err().then_some(None);
    };
    let key = supplied_key(headers)?;
    database
        .authenticate_virtual_key(key, model)
        .await
        .ok()
        .flatten()
        .map(Some)
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

fn error_response(status: StatusCode, code: &str, message: &str) -> Response<Body> {
    (
        status,
        Json(json!({"error":{"code":code,"type":code,"message":message}})),
    )
        .into_response()
}

async fn resolve_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((protocol, model)): Path<(String, String)>,
) -> impl IntoResponse {
    if !admin_authorized(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(
                json!({"error":{"code":"unauthorized","type":"unauthorized","message":"admin key required"}}),
            ),
        );
    }
    let Ok(protocol) = protocol.parse::<Protocol>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"unknown protocol"})),
        );
    };
    match state.resolver().resolve_detailed(protocol, &model) {
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
mod audit_closeout_tests {
    use super::*;
    use axum::{body::to_bytes, extract::Request, Router};
    use std::{
        collections::HashMap,
        io::Write,
        sync::{Arc, Mutex as StdMutex},
    };
    use tracing_subscriber::fmt::MakeWriter;

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct EnvRestore {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn set(name: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(name);
            std::env::set_var(name, value);
            Self { name, previous }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            if let Some(value) = &self.previous {
                std::env::set_var(self.name, value);
            } else {
                std::env::remove_var(self.name);
            }
        }
    }

    #[derive(Clone, Default)]
    struct CapturedLogs(Arc<StdMutex<Vec<u8>>>);

    struct CapturedLogWriter(Arc<StdMutex<Vec<u8>>>);

    impl Write for CapturedLogWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for CapturedLogs {
        type Writer = CapturedLogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            CapturedLogWriter(self.0.clone())
        }
    }

    impl CapturedLogs {
        fn content(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }

    fn account(id: &str, provider_id: &str) -> config::AccountConfig {
        config::AccountConfig {
            id: id.into(),
            provider_id: provider_id.into(),
            display_name: id.into(),
            credential_env: None,
            credential: None,
            enabled: true,
            weight: 100,
            protocol_capabilities: HashMap::new(),
            capabilities: None,
            model_overrides: HashMap::new(),
            model_map: HashMap::new(),
        }
    }

    fn provider(id: &str, base_url: String) -> config::ProviderConfig {
        config::ProviderConfig {
            id: id.into(),
            name: id.into(),
            base_url,
            models: vec!["audit-model".into()],
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

    fn route(protocol: Protocol, mode: &str) -> config::RouteConfig {
        config::RouteConfig {
            id: format!("audit-{mode}-{protocol}"),
            model: "audit-model".into(),
            provider_id: "audit-provider".into(),
            protocols: vec![protocol],
            primary_account_id: "audit-primary".into(),
            fallback_accounts: vec![],
            strategy: "primary_then_weighted_fallback".into(),
            mode: mode.into(),
            adapter: None,
            allow_lossy_conversion: false,
        }
    }

    fn state(config: GatewayConfig) -> AppState {
        let config = Arc::new(config);
        let live = LiveConfig {
            resolver: RouteResolver::new(config.clone()),
            config,
        };
        AppState {
            live: Arc::new(std::sync::RwLock::new(live)),
            http: transport::client().expect("audit HTTP client"),
            db: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(30)),
            listen_addr: "127.0.0.1:0".into(),
        }
    }

    fn proxy_request(uri: &str) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header(CONTENT_TYPE, "application/json");
        if let Ok(key) = std::env::var("GATEWAY_API_KEY") {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        builder
            .body(Body::from(r#"{"model":"audit-model","input":"hello"}"#))
            .expect("proxy request")
    }

    async fn json_body(response: Response<Body>) -> Value {
        serde_json::from_slice(
            &to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("JSON response body"),
        )
        .expect("JSON response")
    }

    #[tokio::test]
    async fn admin_route_resolution_rejects_unauthenticated_requests_without_topology_leak() {
        let _environment_lock = ENV_LOCK.lock().await;
        let _admin_key = EnvRestore::set("GATEWAY_ADMIN_KEY", "configured-admin-secret");
        let config = GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![provider(
                "topology-provider",
                "https://sensitive-topology.invalid".into(),
            )],
            accounts: vec![account("topology-account", "topology-provider")],
            routes: vec![config::RouteConfig {
                id: "topology-route".into(),
                model: "audit-model".into(),
                provider_id: "topology-provider".into(),
                protocols: vec![Protocol::OpenAiResponses],
                primary_account_id: "topology-account".into(),
                fallback_accounts: vec![],
                strategy: "primary_then_weighted_fallback".into(),
                mode: "native".into(),
                adapter: None,
                allow_lossy_conversion: false,
            }],
        };
        let response = application(state(config))
            .oneshot(
                Request::builder()
                    .uri("/admin/routes/openai_responses/audit-model")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("admin route response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = json_body(response).await;
        assert_eq!(body["error"]["code"], "unauthorized");
        let serialized = body.to_string();
        for secret in [
            "topology-provider",
            "topology-account",
            "topology-route",
            "sensitive-topology.invalid",
        ] {
            assert!(
                !serialized.contains(secret),
                "unauthorized response leaked {secret}: {serialized}"
            );
        }
    }

    #[tokio::test]
    async fn proxy_preserves_structured_unsupported_and_lossy_route_errors() {
        let unsupported = GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![provider("audit-provider", "https://unused.invalid".into())],
            accounts: vec![account("audit-primary", "audit-provider")],
            routes: vec![route(Protocol::OpenAiResponses, "unsupported")],
        };
        let response = application(state(unsupported))
            .oneshot(proxy_request("/v1/responses"))
            .await
            .expect("unsupported proxy response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = json_body(response).await;
        assert_eq!(body["error"]["code"], "unsupported_protocol");

        let mut lossy_provider = provider("audit-provider", "https://unused.invalid".into());
        lossy_provider.native_protocols = vec![Protocol::AnthropicMessages];
        lossy_provider.protocol_capabilities.insert(
            Protocol::OpenAiResponses,
            config::ProtocolCapability::adapter(
                Protocol::AnthropicMessages,
                "kimi_responses_adapter",
            ),
        );
        let mut lossy_route = route(Protocol::OpenAiResponses, "adapter");
        lossy_route.adapter = Some("kimi_responses_adapter".into());
        let lossy = GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![lossy_provider],
            accounts: vec![account("audit-primary", "audit-provider")],
            routes: vec![lossy_route],
        };
        let response = application(state(lossy))
            .oneshot(proxy_request("/v1/responses"))
            .await
            .expect("lossy proxy response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = json_body(response).await;
        assert_eq!(body["error"]["code"], "lossy_conversion_not_allowed");
    }

    async fn spawn_fallback_upstream() -> String {
        let app = Router::new().fallback(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"id":"fallback-response"}"#))
                .unwrap()
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fallback upstream");
        let address = listener.local_addr().expect("fallback upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve fallback upstream")
        });
        format!("http://{address}")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn degraded_warning_covers_primary_unavailable_early_fallback() {
        let mut fallback_provider = provider("audit-provider", spawn_fallback_upstream().await);
        fallback_provider.native_protocols = vec![Protocol::AnthropicMessages];
        fallback_provider.protocol_capabilities.insert(
            Protocol::OpenAiResponses,
            config::ProtocolCapability::adapter(
                Protocol::AnthropicMessages,
                "kimi_responses_adapter",
            ),
        );
        let mut degraded_route = route(Protocol::OpenAiResponses, "adapter");
        degraded_route.id = "degraded-fallback-route".into();
        degraded_route.adapter = Some("kimi_responses_adapter".into());
        degraded_route.allow_lossy_conversion = true;
        degraded_route.fallback_accounts = vec!["audit-fallback".into()];
        let config = GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![fallback_provider],
            accounts: vec![
                account("audit-primary", "audit-provider"),
                account("audit-fallback", "audit-provider"),
            ],
            routes: vec![degraded_route],
        };
        let state = state(config);
        state.health.mark_failure("audit-primary").await;

        let logs = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_target(false)
            .with_ansi(false)
            .with_writer(logs.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("install degraded warning test subscriber");
        let response = application(state)
            .oneshot(proxy_request("/v1/responses"))
            .await
            .expect("early fallback response");
        assert_eq!(response.status(), StatusCode::OK);

        let logs = logs.content();
        assert_eq!(
            logs.matches("route has degraded features due to adapter conversion")
                .count(),
            1,
            "expected one degraded warning: {logs}"
        );
        for field in [
            "request_id=",
            "route_id=degraded-fallback-route",
            "degraded_features=",
            "file_search",
        ] {
            assert!(logs.contains(field), "missing {field} in warning: {logs}");
        }
    }
}

#[cfg(test)]
mod usage_api_tests {
    use super::*;
    use std::collections::HashMap;

    fn admin_request(uri: &str) -> Request<Body> {
        let mut builder = Request::builder().uri(uri);
        if let Ok(key) =
            std::env::var("GATEWAY_ADMIN_KEY").or_else(|_| std::env::var("GATEWAY_API_KEY"))
        {
            builder = builder.header("authorization", format!("Bearer {key}"));
        }
        builder.body(Body::empty()).expect("admin request")
    }

    fn usage_test_state(database: db::Database) -> AppState {
        let config = Arc::new(GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![],
            accounts: vec![],
            routes: vec![],
        });
        let live = LiveConfig {
            resolver: RouteResolver::new(config.clone()),
            config,
        };
        AppState {
            live: Arc::new(std::sync::RwLock::new(live)),
            http: transport::client().expect("HTTP client"),
            db: Some(database),
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
            listen_addr: "127.0.0.1:0".into(),
        }
    }

    #[test]
    fn usage_query_validates_utc_boundaries_and_dimensions() {
        let query = HashMap::from([
            ("from".into(), "2026-01-01T08:00:00+08:00".into()),
            ("to".into(), "2026-01-02T00:00:00Z".into()),
            ("logical_model".into(), "logical-a".into()),
            ("upstream_model".into(), "upstream-a".into()),
            ("status".into(), "failure".into()),
            ("breakdown".into(), "protocol_upstream".into()),
        ]);
        let parsed = parse_usage_query(&query).expect("valid usage query");
        assert_eq!(
            parsed.filter.from.unwrap().to_rfc3339(),
            "2026-01-01T00:00:00+00:00"
        );
        assert_eq!(parsed.filter.success, Some(false));
        assert_eq!(parsed.breakdown, "protocol_upstream");

        let parsed_source =
            parse_usage_query(&HashMap::from([("usage_source".into(), "parsed".into())]))
                .expect("parsed is a supported usage source");
        assert_eq!(parsed_source.filter.usage_source.as_deref(), Some("parsed"));

        let invalid = HashMap::from([
            ("from".into(), "2026-01-02T00:00:00Z".into()),
            ("to".into(), "2026-01-01T00:00:00Z".into()),
        ]);
        assert_eq!(
            parse_usage_query(&invalid).unwrap_err(),
            "from must be earlier than to"
        );
    }

    #[test]
    fn csv_export_escapes_fields_and_omits_bodies() {
        assert_eq!(csv_field("a,b\"c"), "\"a,b\"\"c\"");
        let header = usage_events_csv(&[]);
        assert!(header.contains("logical_model,upstream_model_id"));
        assert!(header.contains("route_id,streamed,error_summary"));
        assert!(!header.contains("prompt"));
        assert!(!header.contains("response_body"));
    }

    #[tokio::test]
    async fn postgres_usage_endpoints_share_filters_and_export_contract() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("skipping PostgreSQL API test: TEST_DATABASE_URL is not set");
            return;
        };
        let database = db::Database::connect(&url)
            .await
            .expect("connect PostgreSQL API test database");
        let prefix = format!("usage-api-{}-", Uuid::new_v4());
        let logical_model = format!("model-{prefix}");
        let event = db::UsageEvent {
            request_id: format!("{prefix}request"),
            virtual_key_id: None,
            provider_id: "provider-api".into(),
            account_id: "account-api".into(),
            model: logical_model.clone(),
            logical_model: logical_model.clone(),
            upstream_model_id: Some("upstream-api".into()),
            source: "api-test".into(),
            protocol_in: "openai_responses".into(),
            protocol_upstream: "anthropic_messages".into(),
            mode: "adapter".into(),
            status_code: 200,
            success: true,
            retry_count: 1,
            latency_ms: 42,
            ttft_ms: None,
            input_tokens: 10,
            output_tokens: 5,
            reasoning_tokens: 2,
            cached_tokens: 1,
            total_tokens: 17,
            usage_source: "upstream".into(),
            degraded: false,
            route_id: Some("test-route".into()),
            streamed: false,
            error_summary: None,
        };
        let attempts = [
            db::UsageAttempt {
                attempt_no: 0,
                provider_id: "provider-api".into(),
                account_id: "account-api".into(),
                upstream_model_id: Some("upstream-api".into()),
                status_code: 429,
                success: false,
                latency_ms: 10,
            },
            db::UsageAttempt {
                attempt_no: 1,
                provider_id: "provider-api".into(),
                account_id: "account-api".into(),
                upstream_model_id: Some("upstream-api".into()),
                status_code: 200,
                success: true,
                latency_ms: 32,
            },
        ];
        database
            .insert_usage_with_attempts(&event, &attempts)
            .await
            .expect("insert API fixture");
        let app = application(usage_test_state(database.clone()));

        let summary = app
            .clone()
            .oneshot(admin_request(&format!(
                "/admin/usage/summary?logical_model={logical_model}"
            )))
            .await
            .expect("summary response");
        assert_eq!(summary.status(), StatusCode::OK);
        let summary: Value = serde_json::from_slice(
            &axum::body::to_bytes(summary.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(summary["version"], "v1");
        assert_eq!(summary["data"]["logical_requests"], 1);
        assert_eq!(summary["data"]["upstream_attempts"], 2);

        let events = app
            .clone()
            .oneshot(admin_request(&format!(
                "/admin/usage/events?logical_model={logical_model}&limit=1"
            )))
            .await
            .expect("events response");
        let events: Value = serde_json::from_slice(
            &axum::body::to_bytes(events.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(events["data"][0]["logical_model"], logical_model);
        assert!(events["data"][0].get("prompt").is_none());

        let export = app
            .oneshot(admin_request(&format!(
                "/admin/usage/export?logical_model={logical_model}&format=csv"
            )))
            .await
            .expect("export response");
        assert_eq!(export.status(), StatusCode::OK);
        assert_eq!(
            export.headers().get(CONTENT_TYPE).unwrap(),
            "text/csv; charset=utf-8"
        );
        let export = axum::body::to_bytes(export.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&export).contains(&event.request_id));
        database
            .delete_usage_events_for_test(&prefix)
            .await
            .expect("clean API fixture");
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
                model_map: HashMap::new(),
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
        let live = LiveConfig {
            resolver: RouteResolver::new(config.clone()),
            config,
        };
        AppState {
            live: Arc::new(std::sync::RwLock::new(live)),
            http: transport::client().expect("http client"),
            db: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
            listen_addr: "127.0.0.1:0".into(),
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
