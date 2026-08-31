mod capabilities;
mod config;
mod control_plane;
mod db;
mod discovery_api;
mod health;
mod model_catalog;
mod model_discovery;
mod ops;
mod protocol;
mod provider_preset;
mod routing;
mod source_url;
mod stream_contract;
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
    extract::{rejection::JsonRejection, Path, Query, State},
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        HeaderMap, HeaderValue, Request, Response, StatusCode,
    },
    middleware::{self, Next},
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use config::GatewayConfig;
use protocol::Protocol;
use routing::{ResolvedRoute, RouteResolver};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tower::ServiceExt;
use tower_http::{services::ServeDir, trace::TraceLayer};
use uuid::Uuid;

#[derive(Clone)]
struct LiveConfig {
    config: Arc<GatewayConfig>,
    resolver: RouteResolver,
    models: Arc<Vec<control_plane::PublishedModel>>,
    revision: i64,
    generated_at: chrono::DateTime<chrono::Utc>,
}

impl LiveConfig {
    #[cfg(test)]
    fn legacy(config: Arc<GatewayConfig>) -> Self {
        let account_ids = config
            .accounts
            .iter()
            .filter(|account| account.enabled)
            .map(|account| account.id.clone())
            .collect::<Vec<_>>();
        let models = config
            .models()
            .into_iter()
            .map(|id| control_plane::PublishedModel {
                display_name: id.clone(),
                id,
                account_ids: account_ids.clone(),
            })
            .collect();
        Self {
            resolver: RouteResolver::new(config.clone()),
            config,
            models: Arc::new(models),
            revision: 0,
            generated_at: chrono::Utc::now(),
        }
    }

    fn from_snapshot(snapshot: control_plane::RuntimeSnapshot) -> Self {
        Self {
            config: snapshot.config,
            resolver: snapshot.resolver,
            models: snapshot.models,
            revision: snapshot.revision,
            generated_at: snapshot.generated_at,
        }
    }
}

#[derive(Clone)]
struct AppState {
    live: Arc<std::sync::RwLock<LiveConfig>>,
    http: transport::SourceHttpClient,
    db: Option<db::Database>,
    control_plane: Option<control_plane::ControlPlane>,
    health: health::HealthRegistry,
    admin_auth: AdminAuth,
}

#[derive(Clone)]
pub(crate) struct AdminAuth {
    key_digest: Option<[u8; 32]>,
}

impl AdminAuth {
    fn from_env() -> Self {
        Self::from_key(
            std::env::var("GATEWAY_ADMIN_KEY")
                .ok()
                .filter(|key| !key.is_empty())
                .as_deref(),
        )
    }

    fn from_key(key: Option<&str>) -> Self {
        Self {
            key_digest: key.map(key_digest),
        }
    }

    fn is_configured(&self) -> bool {
        self.key_digest.is_some()
    }

    fn authorized(&self, headers: &HeaderMap) -> bool {
        let (Some(expected), Some(supplied)) = (self.key_digest, supplied_key(headers)) else {
            return false;
        };
        key_matches_digest(&expected, supplied)
    }

    #[cfg(test)]
    pub(crate) fn test() -> Self {
        Self::from_key(Some(TEST_ADMIN_KEY))
    }
}

fn key_digest(key: &str) -> [u8; 32] {
    Sha256::digest(key.as_bytes()).into()
}

fn key_matches_digest(expected: &[u8; 32], supplied: &str) -> bool {
    bool::from(expected.ct_eq(&key_digest(supplied)))
}

#[cfg(test)]
pub(crate) const TEST_ADMIN_KEY: &str = "test-admin-key";

impl AppState {
    fn snapshot(&self) -> LiveConfig {
        self.live.read().unwrap().clone()
    }

    fn reload_snapshot(&self, snapshot: control_plane::RuntimeSnapshot) {
        let candidate = LiveConfig::from_snapshot(snapshot);
        let mut current = self.live.write().unwrap();
        if candidate.revision >= current.revision {
            *current = candidate;
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command_line = std::env::args().skip(1).collect::<Vec<_>>();
    if command_line.first().is_some_and(|value| value == "ops") {
        return run_ops_cli(&command_line[1..]).await;
    }
    tracing_subscriber::fmt::init();
    let database = db::Database::connect_from_env()
        .await?
        .ok_or("DATABASE_URL is required for the DB-first runtime")?;
    let explicit_listen_addr = std::env::var("GATEWAY_LISTEN_ADDR").ok();
    let mut listen_addr = explicit_listen_addr
        .clone()
        .unwrap_or_else(|| "127.0.0.1:8787".to_owned());
    let force_import = std::env::var("GATEWAY_CONFIG_IMPORT")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes"));
    let source_url_policy = Arc::new(source_url::SourceUrlPolicy::from_env()?);
    let initial_control_plane = control_plane::ControlPlane::with_url_policy(
        &database,
        &listen_addr,
        source_url_policy.clone(),
    );
    let should_import = force_import || initial_control_plane.is_empty().await?;
    let bootstrap = if should_import {
        match std::env::var("GATEWAY_CONFIG_JSON") {
            Ok(raw) if !raw.trim().is_empty() => {
                let config: GatewayConfig = serde_json::from_str(&raw)?;
                if explicit_listen_addr.is_none() {
                    listen_addr = config.listen_addr.clone();
                }
                Some(config)
            }
            Ok(_) | Err(_) if force_import => {
                return Err("GATEWAY_CONFIG_IMPORT requires GATEWAY_CONFIG_JSON".into())
            }
            Ok(_) | Err(_) => None,
        }
    } else {
        None
    };
    provider_preset::install_builtin_presets(&database.model_catalog()).await?;
    let control_plane = control_plane::ControlPlane::with_url_policy(
        &database,
        &listen_addr,
        source_url_policy.clone(),
    );
    let snapshot = match bootstrap {
        Some(config) => match control_plane
            .initialize_from_config(&config, force_import)
            .await?
        {
            Some(snapshot) => snapshot,
            None => control_plane.load_snapshot().await?,
        },
        None => control_plane.load_snapshot().await?,
    };
    let addr: SocketAddr = listen_addr.parse()?;
    let live = LiveConfig::from_snapshot(snapshot);
    let admin_auth = AdminAuth::from_env();
    if !admin_auth.is_configured() {
        tracing::warn!("GATEWAY_ADMIN_KEY is not configured; Admin API requests will be rejected");
    }
    let state = AppState {
        live: Arc::new(std::sync::RwLock::new(live)),
        http: transport::client(source_url_policy.clone())?,
        db: Some(database),
        control_plane: Some(control_plane),
        health: health::HealthRegistry::new(std::time::Duration::from_secs(30)),
        admin_auth,
    };
    let app = application(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "AI gateway listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn run_ops_cli(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(ops_cli_usage().into());
    };
    let database = db::Database::connect_from_env()
        .await?
        .ok_or("DATABASE_URL is required for gateway ops")?;
    let repository = ops::OpsRepository::from_database(&database);
    match command {
        "retention-cleanup" => {
            let mut request = ops::CleanupRequest::default();
            let mut index = 1;
            while index < args.len() {
                match args[index].as_str() {
                    "--dry-run" => request.dry_run = true,
                    "--batch-size" => {
                        index += 1;
                        request.batch_size = args
                            .get(index)
                            .ok_or("--batch-size requires a value")?
                            .parse()?;
                    }
                    "--max-batches" => {
                        index += 1;
                        request.max_batches = args
                            .get(index)
                            .ok_or("--max-batches requires a value")?
                            .parse()?;
                    }
                    "--operation-id" => {
                        index += 1;
                        request.operation_id = Some(
                            args.get(index)
                                .ok_or("--operation-id requires a value")?
                                .clone(),
                        );
                    }
                    "--requested-by" => {
                        index += 1;
                        request.requested_by = Some(
                            args.get(index)
                                .ok_or("--requested-by requires a value")?
                                .clone(),
                        );
                    }
                    value => {
                        return Err(format!("unknown retention-cleanup option '{value}'").into())
                    }
                }
                index += 1;
            }
            let run = repository.start_cleanup(&request).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"version":"v1","timezone":"UTC","operation_id":run.id,"data":run})
                )?
            );
        }
        "retention-cancel" | "retention-retry" => {
            let id = args.get(1).ok_or("operation id is required")?;
            let run = if command == "retention-cancel" {
                repository.cancel_cleanup(id, "cli").await?
            } else {
                repository.retry_cleanup(id, "cli").await?
            };
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"version":"v1","timezone":"UTC","operation_id":run.id,"data":run})
                )?
            );
        }
        "retention-policies" => {
            let policies = repository.list_retention_policies().await?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"version":"v1","timezone":"UTC","data":policies})
                )?
            );
        }
        "retention-policy-set" => {
            let key = args.get(1).ok_or("policy key is required")?.clone();
            let days = args.get(2).ok_or("retention days are required")?.parse()?;
            let enabled = !args.iter().any(|value| value == "--disabled");
            let policies = repository
                .update_retention_policies(
                    &[ops::RetentionPolicyWrite {
                        policy_key: key,
                        retention_days: days,
                        enabled,
                    }],
                    "cli",
                )
                .await?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"version":"v1","timezone":"UTC","data":policies})
                )?
            );
        }
        "control-plane-export" => {
            let output = cli_option(args, "--output");
            let policy = Arc::new(source_url::SourceUrlPolicy::from_env()?);
            let listen_addr =
                std::env::var("GATEWAY_LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
            let control_plane =
                control_plane::ControlPlane::with_url_policy(&database, listen_addr, policy);
            let result = repository
                .export_control_plane(&control_plane, "cli")
                .await?;
            let payload = json!({
                "version":"v1",
                "timezone":"UTC",
                "backup_id":result.backup_id,
                "checksum":result.checksum,
                "data":result.export,
            });
            let bytes = serde_json::to_vec_pretty(&payload)?;
            if let Some(path) = output {
                std::fs::write(path, bytes)?;
                println!("control-plane export written");
            } else {
                println!("{}", String::from_utf8(bytes)?);
            }
        }
        "control-plane-import" => {
            let path = cli_option(args, "--input").ok_or("--input is required")?;
            let bytes = std::fs::read(path)?;
            let payload: Value = serde_json::from_slice(&bytes)?;
            let replace = payload
                .get("replace")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || args.iter().any(|value| value == "--replace");
            let requested_by = payload
                .get("requested_by")
                .and_then(Value::as_str)
                .unwrap_or("cli")
                .to_owned();
            let expected_checksum = payload
                .get("checksum")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let export_value = payload.get("data").cloned().unwrap_or(payload);
            let export: ops::ControlPlaneExport = serde_json::from_value(export_value)?;
            if let Some(expected) = expected_checksum {
                let actual = ops::control_plane_export_checksum(&export)?;
                if actual != expected {
                    return Err("control-plane export checksum does not match".into());
                }
            }
            let policy = Arc::new(source_url::SourceUrlPolicy::from_env()?);
            let listen_addr =
                std::env::var("GATEWAY_LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
            let control_plane =
                control_plane::ControlPlane::with_url_policy(&database, listen_addr, policy);
            let result = repository
                .restore_control_plane(&control_plane, &export, replace, &requested_by)
                .await?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"version":"v1","timezone":"UTC","backup_id":result.backup_id,"verified":result.verified,"snapshot_revision":result.snapshot.revision,"snapshot_generated_at":result.snapshot.generated_at,"skipped_virtual_keys":result.skipped_virtual_keys})
                )?
            );
        }
        _ => return Err(ops_cli_usage().into()),
    }
    Ok(())
}

fn cli_option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|values| values[0] == name)
        .map(|values| values[1].as_str())
}

fn ops_cli_usage() -> &'static str {
    "usage: cargo run -- ops <retention-cleanup|retention-cancel|retention-retry|retention-policies|retention-policy-set|control-plane-export|control-plane-import>"
}

fn application(state: AppState) -> Router {
    let discovery_api = discovery_api::auxiliary_router(
        state.db.clone(),
        state.http.clone(),
        state.admin_auth.clone(),
    );
    let admin_api = Router::new()
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
            "/admin/retention/policies",
            get(list_retention_policies).put(update_retention_policies),
        )
        .route(
            "/admin/retention",
            get(list_retention_policies).put(update_retention_policies),
        )
        .route(
            "/admin/retention/cleanup",
            get(list_retention_cleanups).post(start_retention_cleanup),
        )
        .route(
            "/admin/retention/runs",
            get(list_retention_cleanups).post(start_retention_cleanup),
        )
        .route("/admin/retention/cleanup/{id}", get(get_retention_cleanup))
        .route("/admin/retention/runs/{id}", get(get_retention_cleanup))
        .route(
            "/admin/retention/cleanup/{id}/cancel",
            post(cancel_retention_cleanup),
        )
        .route(
            "/admin/retention/cleanup/{id}/retry",
            post(retry_retention_cleanup),
        )
        .route(
            "/admin/retention/runs/{id}/cancel",
            post(cancel_retention_cleanup),
        )
        .route(
            "/admin/retention/runs/{id}/retry",
            post(retry_retention_cleanup),
        )
        .route("/admin/control-plane/export", get(export_control_plane))
        .route("/admin/control-plane/import", post(import_control_plane))
        .route("/admin/backup/export", get(export_control_plane))
        .route("/admin/backup/import", post(import_control_plane))
        .route("/admin/audit", get(list_audit_logs))
        .route("/admin/backups/{id}", get(get_backup_run))
        .route("/admin/backups", get(list_backup_runs))
        .route("/admin/backup/{id}", get(get_backup_run))
        .route("/admin/ops/schema", get(ops_schema_metadata))
        .route("/admin/schema", get(ops_schema_metadata))
        .route("/admin/sources", get(list_sources).post(create_source))
        .route(
            "/admin/sources/{id}",
            get(get_source).put(update_source).delete(delete_source),
        )
        .route("/admin/sources/{id}/enabled", put(set_source_enabled))
        .route("/admin/accounts", get(list_accounts).post(create_account))
        .route(
            "/admin/accounts/{id}",
            get(get_account).put(update_account).delete(delete_account),
        )
        .route("/admin/accounts/{id}/enabled", put(set_account_enabled))
        .route(
            "/admin/logical-models",
            get(list_logical_models).post(create_logical_model),
        )
        .route(
            "/admin/logical-models/{id}",
            get(get_logical_model)
                .put(update_logical_model)
                .delete(delete_logical_model),
        )
        .route(
            "/admin/logical-models/{id}/enabled",
            put(set_logical_model_enabled),
        )
        .route(
            "/admin/model-bindings",
            get(list_model_bindings).post(create_model_binding),
        )
        .route(
            "/admin/model-bindings/{id}",
            get(get_model_binding)
                .put(update_model_binding)
                .delete(delete_model_binding),
        )
        .route(
            "/admin/model-bindings/{id}/enabled",
            put(set_model_binding_enabled),
        )
        .route("/admin/routes", get(list_routes).post(create_route))
        .route(
            "/admin/routes/{id}",
            get(get_route).put(update_route).delete(delete_route),
        )
        .route("/admin/routes/{id}/enabled", put(set_route_enabled))
        .route("/admin/config/reload", post(reload_config))
        .route("/admin/capabilities", get(admin_capabilities))
        .route("/admin/health", get(admin_health))
        .route("/admin/routes/{protocol}/{model}", get(resolve_route))
        .with_state(state.clone())
        .merge(discovery_api)
        .route_layer(middleware::from_fn_with_state(
            state.admin_auth.clone(),
            require_admin_auth,
        ));
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/responses", post(responses))
        .route("/v1/messages", post(messages))
        .nest_service("/admin", ServeDir::new("web/dist"))
        .with_state(state)
        .merge(admin_api)
        .layer(TraceLayer::new_for_http())
}

async fn require_admin_auth(
    State(auth): State<AdminAuth>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    if !auth.authorized(request.headers()) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    next.run(request).await
}

async fn healthz(State(state): State<AppState>) -> Json<Value> {
    let live = state.snapshot();
    Json(
        json!({"status":"ok", "sources":live.config.providers.len(), "accounts":live.config.accounts.len(), "snapshot_revision":live.revision, "snapshot_generated_at":live.generated_at}),
    )
}

async fn models(State(state): State<AppState>) -> Json<Value> {
    let live = state.snapshot();
    let mut data = Vec::new();
    for model in live.models.iter() {
        let mut healthy = false;
        for account_id in &model.account_ids {
            if state.health.is_available(account_id).await {
                healthy = true;
                break;
            }
        }
        if healthy {
            data.push(json!({"id":model.id,"object":"model","owned_by":"gateway"}));
        }
    }
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
    if !state.admin_auth.authorized(&headers) {
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
    if !state.admin_auth.authorized(&headers) {
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
    if !state.admin_auth.authorized(&headers) {
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
    if query.contains_key("source") {
        return Err("source is no longer supported; use source_id or client_source".into());
    }
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
            | "source_id"
            | "client_source"
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
            source_id: query.get("source_id").cloned(),
            client_source: query.get("client_source").cloned(),
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
    if !state.admin_auth.authorized(&headers) {
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
    if !state.admin_auth.authorized(&headers) {
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
    if !state.admin_auth.authorized(&headers) {
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
    if !state.admin_auth.authorized(&headers) {
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
    if !state.admin_auth.authorized(&headers) {
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
    if !state.admin_auth.authorized(&headers) {
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
    let mut output = String::from("request_id,created_at,virtual_key_id,logical_model,upstream_model_id,provider_id,source_id,client_source,account_id,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,ttft_ms,input_tokens,output_tokens,reasoning_tokens,cached_tokens,total_tokens,usage_source,degraded,route_id,streamed,error_summary\n");
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
            event.source_id.clone().unwrap_or_default(),
            event.client_source.clone(),
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

async fn usage_event_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
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

#[allow(clippy::result_large_err)]
fn ops_repository(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<ops::OpsRepository, Response<Body>> {
    if !state.admin_auth.authorized(headers) {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        ));
    }
    let Some(database) = &state.db else {
        return Err(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        ));
    };
    Ok(ops::OpsRepository::from_database(database))
}

fn ops_error_response(error: ops::OpsError) -> Response<Body> {
    let status = match &error {
        ops::OpsError::NotFound(_) => StatusCode::NOT_FOUND,
        ops::OpsError::Conflict(_) => StatusCode::CONFLICT,
        ops::OpsError::Validation(_) | ops::OpsError::Json(_) | ops::OpsError::Snapshot(_) => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        ops::OpsError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let code = match &error {
        ops::OpsError::NotFound(_) => "not_found",
        ops::OpsError::Conflict(_) => "operation_conflict",
        ops::OpsError::Validation(_) | ops::OpsError::Json(_) => "invalid_operation",
        ops::OpsError::Snapshot(_) => "snapshot_verification_failed",
        ops::OpsError::Database(_) => "operation_failed",
    };
    let message = match error {
        ops::OpsError::Database(_) => "database operation failed".to_owned(),
        other => other.to_string(),
    };
    error_response(status, code, &message)
}

async fn list_retention_policies(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    match repository.list_retention_policies().await {
        Ok(data) => (
            StatusCode::OK,
            Json(json!({"version":"v1","timezone":"UTC","data":data})),
        )
            .into_response(),
        Err(error) => ops_error_response(error),
    }
}

async fn update_retention_policies(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    if body.len() > 1024 * 1024 {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "request body exceeds 1 MiB",
        );
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "request body is not valid JSON",
            )
        }
    };
    let actor = payload
        .get("requested_by")
        .and_then(Value::as_str)
        .unwrap_or("admin_api");
    let policies = if let Some(values) = payload.get("policies") {
        match serde_json::from_value::<Vec<ops::RetentionPolicyWrite>>(values.clone()) {
            Ok(values) => values,
            Err(_) => {
                return error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_operation",
                    "policies must be an array of retention policy objects",
                )
            }
        }
    } else if payload.get("policy_key").is_some() {
        match serde_json::from_value::<ops::RetentionPolicyWrite>(payload.clone()) {
            Ok(value) => vec![value],
            Err(_) => {
                return error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_operation",
                    "retention policy object is invalid",
                )
            }
        }
    } else {
        // Also accept a compact map: {"usage_events":{"retention_days":90}}
        let Some(object) = payload.as_object() else {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_operation",
                "retention policy payload is invalid",
            );
        };
        let mut values = Vec::new();
        for (key, value) in object {
            if key == "requested_by" {
                continue;
            }
            let Some(policy) = value.as_object() else {
                return error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_operation",
                    "retention policy map values must be objects",
                );
            };
            let mut policy = policy.clone();
            policy.insert("policy_key".into(), Value::String(key.clone()));
            match serde_json::from_value::<ops::RetentionPolicyWrite>(Value::Object(policy)) {
                Ok(value) => values.push(value),
                Err(_) => {
                    return error_response(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "invalid_operation",
                        "retention policy map value is invalid",
                    )
                }
            }
        }
        values
    };
    match repository.update_retention_policies(&policies, actor).await {
        Ok(data) => (
            StatusCode::OK,
            Json(json!({"version":"v1","timezone":"UTC","data":data})),
        )
            .into_response(),
        Err(error) => ops_error_response(error),
    }
}

async fn start_retention_cleanup(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    if body.len() > 1024 * 1024 {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "request body exceeds 1 MiB",
        );
    }
    let request = if body.is_empty() {
        ops::CleanupRequest::default()
    } else {
        match serde_json::from_slice::<ops::CleanupRequest>(&body) {
            Ok(request) => request,
            Err(_) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_json",
                    "cleanup request is invalid",
                )
            }
        }
    };
    match repository.start_cleanup(&request).await {
        Ok(run) => (
            if run.status == "running" {
                StatusCode::ACCEPTED
            } else {
                StatusCode::OK
            },
            Json(json!({"version":"v1","timezone":"UTC","operation_id":run.id,"data":run})),
        )
            .into_response(),
        Err(error) => ops_error_response(error),
    }
}

async fn list_retention_cleanups(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    let limit = match query.get("limit") {
        Some(value) => match value.parse::<i64>() {
            Ok(value) if (1..=500).contains(&value) => value,
            _ => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_operation",
                    "limit must be between 1 and 500",
                )
            }
        },
        None => 100,
    };
    match repository.list_cleanups(limit).await {
        Ok(data) => (
            StatusCode::OK,
            Json(json!({"version":"v1","timezone":"UTC","data":data})),
        )
            .into_response(),
        Err(error) => ops_error_response(error),
    }
}

async fn get_retention_cleanup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    match repository.get_cleanup(&id).await {
        Ok(Some(run)) => (
            StatusCode::OK,
            Json(json!({"version":"v1","timezone":"UTC","operation_id":run.id,"data":run})),
        )
            .into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "cleanup operation not found",
        ),
        Err(error) => ops_error_response(error),
    }
}

async fn cancel_retention_cleanup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    match repository.cancel_cleanup(&id, "admin_api").await {
        Ok(run) => (
            StatusCode::OK,
            Json(json!({"version":"v1","timezone":"UTC","operation_id":run.id,"data":run})),
        )
            .into_response(),
        Err(error) => ops_error_response(error),
    }
}

async fn retry_retention_cleanup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    match repository.retry_cleanup(&id, "admin_api").await {
        Ok(run) => (
            if run.status == "running" {
                StatusCode::ACCEPTED
            } else {
                StatusCode::OK
            },
            Json(json!({"version":"v1","timezone":"UTC","operation_id":run.id,"data":run})),
        )
            .into_response(),
        Err(error) => ops_error_response(error),
    }
}

async fn export_control_plane(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    let Some(control_plane) = state.control_plane.as_ref() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        );
    };
    match repository
        .export_control_plane(control_plane, "admin_api")
        .await
    {
        Ok(result) => {
            let payload = json!({
                "version":"v1",
                "timezone":"UTC",
                "backup_id":result.backup_id,
                "checksum":result.checksum,
                "data":result.export,
            });
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/json")
                .header(
                    CONTENT_DISPOSITION,
                    "attachment; filename=control-plane.json",
                )
                .body(Body::from(
                    serde_json::to_vec(&payload).expect("serializable control-plane export"),
                ))
                .expect("valid control-plane export response")
        }
        Err(error) => ops_error_response(error),
    }
}

async fn import_control_plane(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    if body.len() > 16 * 1024 * 1024 {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "control-plane export exceeds 16 MiB",
        );
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "control-plane export is not valid JSON",
            )
        }
    };
    let replace = payload
        .get("replace")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let requested_by = payload
        .get("requested_by")
        .and_then(Value::as_str)
        .unwrap_or("admin_api");
    let export_value = payload
        .get("data")
        .cloned()
        .unwrap_or_else(|| payload.clone());
    let export: ops::ControlPlaneExport = match serde_json::from_value(export_value) {
        Ok(export) => export,
        Err(_) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_operation",
                "control-plane export shape is invalid",
            )
        }
    };
    if let Some(expected) = payload.get("checksum").and_then(Value::as_str) {
        match ops::control_plane_export_checksum(&export) {
            Ok(actual) if actual == expected => {}
            Ok(_) => {
                return error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "checksum_mismatch",
                    "control-plane export checksum does not match",
                )
            }
            Err(_) => {
                return error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_operation",
                    "control-plane export checksum cannot be computed",
                )
            }
        }
    }
    let Some(control_plane) = state.control_plane.as_ref() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        );
    };
    match repository
        .restore_control_plane(control_plane, &export, replace, requested_by)
        .await
    {
        Ok(result) => {
            let revision = result.snapshot.revision;
            let generated_at = result.snapshot.generated_at;
            state.reload_snapshot(result.snapshot);
            (
                StatusCode::OK,
                Json(json!({
                    "version":"v1",
                    "timezone":"UTC",
                    "backup_id":result.backup_id,
                    "verified":result.verified,
                    "snapshot_revision":revision,
                    "snapshot_generated_at":generated_at,
                    "skipped_virtual_keys":result.skipped_virtual_keys,
                })),
            )
                .into_response()
        }
        Err(error) => ops_error_response(error),
    }
}

async fn list_audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    let limit = match query.get("limit") {
        Some(value) => match value.parse::<i64>() {
            Ok(value) if (1..=500).contains(&value) => value,
            _ => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_operation",
                    "limit must be between 1 and 500",
                )
            }
        },
        None => 100,
    };
    match repository
        .list_audit_logs(query.get("operation_id").map(String::as_str), limit)
        .await
    {
        Ok(data) => (
            StatusCode::OK,
            Json(json!({"version":"v1","timezone":"UTC","data":data})),
        )
            .into_response(),
        Err(error) => ops_error_response(error),
    }
}

async fn get_backup_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    match repository.get_backup_run(&id).await {
        Ok(Some(data)) => (
            StatusCode::OK,
            Json(json!({"version":"v1","timezone":"UTC","data":data})),
        )
            .into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "backup operation not found",
        ),
        Err(error) => ops_error_response(error),
    }
}

async fn list_backup_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    let limit = match query.get("limit") {
        Some(value) => match value.parse::<i64>() {
            Ok(value) if (1..=500).contains(&value) => value,
            _ => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_operation",
                    "limit must be between 1 and 500",
                )
            }
        },
        None => 100,
    };
    match repository.list_backup_runs(limit).await {
        Ok(data) => (
            StatusCode::OK,
            Json(json!({"version":"v1","timezone":"UTC","data":data})),
        )
            .into_response(),
        Err(error) => ops_error_response(error),
    }
}

async fn ops_schema_metadata(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    let repository = match ops_repository(&state, &headers) {
        Ok(repository) => repository,
        Err(response) => return response,
    };
    match repository.schema_metadata().await {
        Ok(data) => (
            StatusCode::OK,
            Json(json!({"version":"v1","timezone":"UTC","data":data})),
        )
            .into_response(),
        Err(error) => ops_error_response(error),
    }
}

#[allow(clippy::result_large_err)]
fn admin_control_plane<'a>(
    state: &'a AppState,
    headers: &HeaderMap,
) -> Result<&'a control_plane::ControlPlane, Response<Body>> {
    if !state.admin_auth.authorized(headers) {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        ));
    }
    state.control_plane.as_ref().ok_or_else(|| {
        error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        )
    })
}

fn control_plane_error(error: control_plane::ControlPlaneError) -> Response<Body> {
    let status = match &error {
        control_plane::ControlPlaneError::NotFound(_) => StatusCode::NOT_FOUND,
        control_plane::ControlPlaneError::Conflict(_) => StatusCode::CONFLICT,
        control_plane::ControlPlaneError::Validation(_)
        | control_plane::ControlPlaneError::Json(_) => StatusCode::UNPROCESSABLE_ENTITY,
        control_plane::ControlPlaneError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error_response(status, error.code(), &error.message())
}

fn admin_result<T: serde::Serialize>(
    result: Result<T, control_plane::ControlPlaneError>,
) -> Response<Body> {
    match result {
        Ok(record) => (StatusCode::OK, Json(json!({"data": record}))).into_response(),
        Err(error) => control_plane_error(error),
    }
}

fn mutation_result<T: serde::Serialize>(
    state: &AppState,
    status: StatusCode,
    result: Result<control_plane::Mutation<T>, control_plane::ControlPlaneError>,
) -> Response<Body> {
    match result {
        Ok(mutation) => {
            let revision = mutation.snapshot.revision;
            let generated_at = mutation.snapshot.generated_at;
            state.reload_snapshot(mutation.snapshot);
            (
                status,
                Json(json!({"data": mutation.record, "snapshot_revision": revision, "snapshot_generated_at": generated_at})),
            )
                .into_response()
        }
        Err(error) => control_plane_error(error),
    }
}

fn delete_result(
    state: &AppState,
    result: Result<control_plane::RuntimeSnapshot, control_plane::ControlPlaneError>,
) -> Response<Body> {
    match result {
        Ok(snapshot) => {
            state.reload_snapshot(snapshot);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => control_plane_error(error),
    }
}

#[allow(clippy::result_large_err)]
fn json_payload<T>(payload: Result<Json<T>, JsonRejection>) -> Result<T, Response<Body>> {
    payload.map(|Json(value)| value).map_err(|error| {
        error_response(StatusCode::BAD_REQUEST, "invalid_json", &error.body_text())
    })
}

async fn list_sources(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(control_plane.list_sources().await)
}

async fn get_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(control_plane.get_source(&id).await)
}

async fn create_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<control_plane::SourceCreateWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::CREATED,
        control_plane.create_source_from_request(&input).await,
    )
}

async fn update_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    payload: Result<Json<control_plane::SourceWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane.update_source(&id, &input).await,
    )
}

async fn set_source_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    payload: Result<Json<control_plane::EnabledWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane.set_source_enabled(&id, input.enabled).await,
    )
}

async fn delete_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    delete_result(&state, control_plane.delete_source(&id).await)
}

async fn list_accounts(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(control_plane.list_accounts().await)
}

async fn get_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(control_plane.get_account(&id).await)
}

async fn create_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<control_plane::AccountWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::CREATED,
        control_plane.create_account(&input).await,
    )
}

async fn update_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    payload: Result<Json<control_plane::AccountWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane.update_account(&id, &input).await,
    )
}

async fn set_account_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    payload: Result<Json<control_plane::EnabledWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane.set_account_enabled(&id, input.enabled).await,
    )
}

async fn delete_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    delete_result(&state, control_plane.delete_account(&id).await)
}

async fn list_logical_models(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(control_plane.list_logical_models().await)
}

async fn get_logical_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(control_plane.get_logical_model(&id).await)
}

async fn create_logical_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<control_plane::LogicalModelWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::CREATED,
        control_plane.create_logical_model(&input).await,
    )
}

async fn update_logical_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    payload: Result<Json<control_plane::LogicalModelWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane.update_logical_model(&id, &input).await,
    )
}

async fn set_logical_model_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    payload: Result<Json<control_plane::EnabledWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane
            .set_logical_model_enabled(&id, input.enabled)
            .await,
    )
}

async fn delete_logical_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    delete_result(&state, control_plane.delete_logical_model(&id).await)
}

async fn list_model_bindings(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(control_plane.list_model_bindings().await)
}

async fn get_model_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(control_plane.get_model_binding(id).await)
}

async fn create_model_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<control_plane::ModelBindingWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::CREATED,
        control_plane.create_model_binding(&input).await,
    )
}

async fn update_model_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    payload: Result<Json<control_plane::ModelBindingWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane.update_model_binding(id, &input).await,
    )
}

async fn set_model_binding_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    payload: Result<Json<control_plane::EnabledWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane
            .set_model_binding_enabled(id, input.enabled)
            .await,
    )
}

async fn delete_model_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    delete_result(&state, control_plane.delete_model_binding(id).await)
}

async fn list_routes(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(control_plane.list_routes().await)
}

async fn get_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(control_plane.get_route(&id).await)
}

async fn create_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<control_plane::RouteWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::CREATED,
        control_plane.create_route(&input).await,
    )
}

async fn update_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    payload: Result<Json<control_plane::RouteWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane.update_route(&id, &input).await,
    )
}

async fn set_route_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    payload: Result<Json<control_plane::EnabledWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let input = match json_payload(payload) {
        Ok(input) => input,
        Err(response) => return response,
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane.set_route_enabled(&id, input.enabled).await,
    )
}

async fn delete_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    delete_result(&state, control_plane.delete_route(&id).await)
}

async fn admin_health(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let health_map = state.health.all_health().await;
    let live = state.snapshot();
    let mut data = Vec::new();
    for account in &live.config.accounts {
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
    let live = state.snapshot();
    admin_capabilities_response(state.admin_auth.authorized(&headers), &live)
}

fn admin_capabilities_response(authorized: bool, live: &LiveConfig) -> Response<Body> {
    if !authorized {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    match capabilities::CapabilityMatrixResponse::from_runtime_snapshot(
        &live.config,
        &live.resolver,
        &live.models,
        live.revision,
        live.generated_at,
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            error.code(),
            &error.to_string(),
        ),
    }
}

#[cfg(test)]
mod admin_capabilities_api_tests {
    use super::*;
    use axum::body::to_bytes;

    fn empty_runtime() -> LiveConfig {
        let config = Arc::new(GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: Vec::new(),
            accounts: Vec::new(),
            routes: Vec::new(),
        });
        LiveConfig {
            resolver: RouteResolver::from_runtime(config.clone(), Vec::new()),
            config,
            models: Arc::new(Vec::new()),
            revision: 7,
            generated_at: chrono::Utc::now(),
        }
    }

    async fn response_json(response: Response<Body>) -> Value {
        serde_json::from_slice(
            &to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("read capabilities response"),
        )
        .expect("capabilities JSON response")
    }

    #[tokio::test]
    async fn capability_matrix_preserves_admin_authorization() {
        let response = admin_capabilities_response(false, &empty_runtime());
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = response_json(response).await;
        assert_eq!(body["error"]["code"], "unauthorized");
        assert!(body.get("data").is_none());
    }

    #[tokio::test]
    async fn authorized_capability_matrix_uses_runtime_snapshot_contract() {
        let response = admin_capabilities_response(true, &empty_runtime());
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["version"], "v1");
        assert_eq!(body["fact_source"], "runtime_snapshot");
        assert_eq!(body["snapshot_revision"], 7);
        assert_eq!(body["data"], json!([]));
    }
}

async fn reload_config(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    reload_config_inner(&state).await
}

async fn reload_config_inner(state: &AppState) -> Response<Body> {
    let Some(control_plane) = &state.control_plane else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        );
    };
    match control_plane.load_snapshot().await {
        Ok(snapshot) => {
            let revision = snapshot.revision;
            let generated_at = snapshot.generated_at;
            state.reload_snapshot(snapshot);
            (
                StatusCode::OK,
                Json(json!({"status":"reloaded", "snapshot_revision":revision, "snapshot_generated_at":generated_at})),
            )
                .into_response()
        }
        Err(error) => control_plane_error(error),
    }
}

async fn proxy(
    state: AppState,
    headers: HeaderMap,
    body: Bytes,
    protocol: Protocol,
) -> Response<Body> {
    let started = Instant::now();
    let stream_config = stream_contract::StreamConfig::from_env();
    let request_id = Uuid::new_v4().to_string();
    let live = state.snapshot();
    let config = live.config;
    let resolver = live.resolver;
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
    let Some(provider) = config.provider(&route.source_id) else {
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
        let Some(candidate) =
            select_fallback_candidate(&config, &state.health, &route, model, protocol).await
        else {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                if account.enabled {
                    "account_cooling_down"
                } else {
                    "account_disabled"
                },
                "primary account is unavailable and no fallback succeeded",
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
                if is_retryable(status) {
                    state.health.mark_failure(&candidate.account.id).await;
                } else {
                    state.health.mark_success(&candidate.account.id).await;
                }
                (response, status.as_u16() as i32, status.is_success())
            }
            Err(error) => {
                state.health.mark_failure(&candidate.account.id).await;
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
        let is_streamed = payload
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
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
                );
            }
            if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await {
                tracing::warn!(%error, "failed to persist usage event");
            }
        }
        return response;
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
            state.health.mark_success(&account.id).await;
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
                &stream_config,
                started,
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
            );
        }
        if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await {
            tracing::warn!(%error, "failed to persist usage event");
        }
    }
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

fn wrap_stream_usage(
    response: Response<Body>,
    database: db::Database,
    mut event: db::UsageEvent,
    request_body: Bytes,
    mut attempts: Vec<db::UsageAttempt>,
    request_started: Instant,
) -> Response<Body> {
    let (parts, body) = response.into_parts();
    let body = usage::observe_stream_body(body, request_started, move |observation| {
        event.latency_ms = request_started.elapsed().as_millis() as i64;
        finalize_stream_usage(&mut event, &mut attempts, &request_body, observation);
        tokio::spawn(async move {
            if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await {
                tracing::warn!(%error, "failed to persist streaming usage event");
            }
        });
    });
    Response::from_parts(parts, body)
}

fn finalize_stream_usage(
    event: &mut db::UsageEvent,
    attempts: &mut [db::UsageAttempt],
    request_body: &[u8],
    observation: usage::StreamObservation,
) {
    event.ttft_ms = observation.ttft_ms;
    let termination = if observation.failed && !observation.termination.is_failure() {
        stream_contract::StreamTermination::UpstreamError
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
                stream_contract::StreamTermination::UpstreamError => "upstream stream error",
                stream_contract::StreamTermination::EmptyStream => {
                    "upstream stream ended without an event"
                }
                stream_contract::StreamTermination::ClientCancelled => "client disconnected",
                stream_contract::StreamTermination::ConnectionTimeout => {
                    "upstream connection timeout"
                }
                stream_contract::StreamTermination::FirstEventTimeout => "first event timeout",
                stream_contract::StreamTermination::IdleTimeout => "upstream idle timeout",
                stream_contract::StreamTermination::TotalTimeout => "stream total timeout",
                stream_contract::StreamTermination::Completed => "",
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
async fn forward_account(
    config: &GatewayConfig,
    http: &transport::SourceHttpClient,
    route: &ResolvedRoute,
    provider: &config::ProviderConfig,
    account: &config::AccountConfig,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream_contract::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = config.credential_for(account);
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
    stream_config: &stream_contract::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    http.validate_base_url(&provider.base_url)?;
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
    adapter
        .oneshot(request)
        .await
        .map_err(|_| transport::TransportError::Request)
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
async fn try_fallback(
    config: &GatewayConfig,
    health: &health::HealthRegistry,
    http: &transport::SourceHttpClient,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    first: Response<Body>,
    stream_config: &stream_contract::StreamConfig,
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
            if is_retryable(response.status()) {
                health.mark_failure(&candidate.account.id).await;
            } else {
                health.mark_success(&candidate.account.id).await;
            }
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
            health.mark_failure(&candidate.account.id).await;
            (first, attempts)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn try_fallback_error(
    config: &GatewayConfig,
    health: &health::HealthRegistry,
    http: &transport::SourceHttpClient,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    first_error: transport::TransportError,
    stream_config: &stream_contract::StreamConfig,
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
            if is_retryable(response.status()) {
                health.mark_failure(&candidate.account.id).await;
            } else {
                health.mark_success(&candidate.account.id).await;
            }
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
            health.mark_failure(&candidate.account.id).await;
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
    config: &GatewayConfig,
    http: &transport::SourceHttpClient,
    provider: &config::ProviderConfig,
    account: &config::AccountConfig,
    protocol: Protocol,
    mode: &str,
    adapter: Option<&str>,
    upstream_endpoint: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream_contract::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = config.credential_for(account);
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

async fn authorized_with_db(
    state: &AppState,
    headers: &HeaderMap,
    model: &str,
) -> Option<Option<i64>> {
    if let Ok(expected) = std::env::var("GATEWAY_API_KEY") {
        if supplied_key(headers)
            .is_some_and(|supplied| key_matches_digest(&key_digest(&expected), supplied))
        {
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
    if !state.admin_auth.authorized(&headers) {
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
            Json(json!({"error":{"code":"unknown_protocol","message":"unknown protocol"}})),
        );
    };
    match state.snapshot().resolver.resolve_detailed(protocol, &model) {
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
mod runtime_usage_tests;

#[cfg(test)]
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
struct EnvRestore {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl EnvRestore {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

#[cfg(test)]
impl Drop for EnvRestore {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var(self.name, value);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

#[cfg(test)]
mod admin_auth_tests {
    use super::*;
    use axum::{body::to_bytes, http::header};

    const ADMIN_API_ROUTES: &[(&str, &str)] = &[
        ("GET", "/admin/keys"),
        ("POST", "/admin/keys"),
        ("POST", "/admin/keys/1/revoke"),
        ("GET", "/admin/usage/summary"),
        ("GET", "/admin/usage/timeseries"),
        ("GET", "/admin/usage/breakdown"),
        ("GET", "/admin/usage/events"),
        ("GET", "/admin/usage/export"),
        ("GET", "/admin/usage/aggregate"),
        ("GET", "/admin/usage/events/request-id"),
        ("GET", "/admin/retention/policies"),
        ("PUT", "/admin/retention/policies"),
        ("GET", "/admin/retention"),
        ("PUT", "/admin/retention"),
        ("POST", "/admin/retention/cleanup"),
        ("GET", "/admin/retention/cleanup"),
        ("POST", "/admin/retention/runs"),
        ("GET", "/admin/retention/runs"),
        ("GET", "/admin/retention/cleanup/operation-id"),
        ("GET", "/admin/retention/runs/operation-id"),
        ("POST", "/admin/retention/cleanup/operation-id/cancel"),
        ("POST", "/admin/retention/cleanup/operation-id/retry"),
        ("POST", "/admin/retention/runs/operation-id/cancel"),
        ("POST", "/admin/retention/runs/operation-id/retry"),
        ("GET", "/admin/control-plane/export"),
        ("POST", "/admin/control-plane/import"),
        ("GET", "/admin/backup/export"),
        ("POST", "/admin/backup/import"),
        ("GET", "/admin/audit"),
        ("GET", "/admin/backups/backup-id"),
        ("GET", "/admin/backups"),
        ("GET", "/admin/backup/backup-id"),
        ("GET", "/admin/ops/schema"),
        ("GET", "/admin/schema"),
        ("GET", "/admin/sources"),
        ("POST", "/admin/sources"),
        ("GET", "/admin/sources/source-id"),
        ("PUT", "/admin/sources/source-id"),
        ("DELETE", "/admin/sources/source-id"),
        ("PUT", "/admin/sources/source-id/enabled"),
        ("GET", "/admin/accounts"),
        ("POST", "/admin/accounts"),
        ("GET", "/admin/accounts/account-id"),
        ("PUT", "/admin/accounts/account-id"),
        ("DELETE", "/admin/accounts/account-id"),
        ("PUT", "/admin/accounts/account-id/enabled"),
        ("GET", "/admin/logical-models"),
        ("POST", "/admin/logical-models"),
        ("GET", "/admin/logical-models/model-id"),
        ("PUT", "/admin/logical-models/model-id"),
        ("DELETE", "/admin/logical-models/model-id"),
        ("PUT", "/admin/logical-models/model-id/enabled"),
        ("GET", "/admin/model-bindings"),
        ("POST", "/admin/model-bindings"),
        ("GET", "/admin/model-bindings/1"),
        ("PUT", "/admin/model-bindings/1"),
        ("DELETE", "/admin/model-bindings/1"),
        ("PUT", "/admin/model-bindings/1/enabled"),
        ("GET", "/admin/routes"),
        ("POST", "/admin/routes"),
        ("GET", "/admin/routes/route-id"),
        ("PUT", "/admin/routes/route-id"),
        ("DELETE", "/admin/routes/route-id"),
        ("PUT", "/admin/routes/route-id/enabled"),
        ("POST", "/admin/config/reload"),
        ("GET", "/admin/capabilities"),
        ("GET", "/admin/health"),
        ("GET", "/admin/routes/openai_responses/model-id"),
        ("GET", "/admin/provider-presets"),
        ("GET", "/admin/sources/source-id/preset-diff"),
        ("POST", "/admin/sources/source-id/connection-tests"),
        ("POST", "/admin/sources/source-id/discoveries"),
        ("GET", "/admin/sources/source-id/discoveries/latest"),
        ("GET", "/admin/sources/source-id/models"),
        ("PATCH", "/admin/sources/source-id/models"),
        ("POST", "/admin/sources/source-id/models/confirm"),
    ];

    fn state(admin_auth: AdminAuth) -> AppState {
        let config = Arc::new(GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: Vec::new(),
            accounts: Vec::new(),
            routes: Vec::new(),
        });
        AppState {
            live: Arc::new(std::sync::RwLock::new(LiveConfig::legacy(config))),
            http: transport::test_client().expect("admin auth HTTP client"),
            db: None,
            control_plane: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
            admin_auth,
        }
    }

    fn request(method: &str, uri: &str, key: Option<&str>) -> Request<Body> {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(CONTENT_TYPE, "application/json");
        if let Some(key) = key {
            request = request.header(header::AUTHORIZATION, format!("Bearer {key}"));
        }
        request.body(Body::from("{}")).expect("admin request")
    }

    async fn assert_unauthorized(response: Response<Body>) {
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read unauthorized response");
        let body: Value = serde_json::from_slice(&body).expect("unauthorized JSON");
        assert_eq!(body["error"]["code"], "unauthorized");
        let serialized = body.to_string();
        for secret in [TEST_ADMIN_KEY, "data-plane-only", "GATEWAY_ADMIN_KEY"] {
            assert!(!serialized.contains(secret));
        }
    }

    #[test]
    fn admin_auth_is_fail_closed_and_accepts_both_supported_headers() {
        let unconfigured = AdminAuth::from_key(None);
        assert!(!unconfigured.is_configured());
        assert!(!unconfigured.authorized(&HeaderMap::new()));

        let auth = AdminAuth::test();
        let mut authorization = HeaderMap::new();
        authorization.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {TEST_ADMIN_KEY}")).unwrap(),
        );
        assert!(auth.authorized(&authorization));

        let mut x_api_key = HeaderMap::new();
        x_api_key.insert("x-api-key", HeaderValue::from_static(TEST_ADMIN_KEY));
        assert!(auth.authorized(&x_api_key));

        let mut wrong = HeaderMap::new();
        wrong.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer data-plane-only"),
        );
        assert!(!auth.authorized(&wrong));
    }

    #[tokio::test]
    async fn every_admin_api_route_rejects_missing_and_data_plane_keys_before_parsing() {
        let app = application(state(AdminAuth::test()));
        for (method, uri) in ADMIN_API_ROUTES {
            let response = app
                .clone()
                .oneshot(request(method, uri, None))
                .await
                .unwrap_or_else(|error| panic!("{method} {uri}: {error}"));
            assert_unauthorized(response).await;

            let response = app
                .clone()
                .oneshot(request(method, uri, Some("data-plane-only")))
                .await
                .unwrap_or_else(|error| panic!("{method} {uri}: {error}"));
            assert_unauthorized(response).await;

            let response = app
                .clone()
                .oneshot(request(method, uri, Some(TEST_ADMIN_KEY)))
                .await
                .unwrap_or_else(|error| panic!("{method} {uri}: {error}"));
            assert_ne!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {uri}"
            );
        }
    }

    #[test]
    fn admin_key_does_not_match_the_data_plane_key() {
        let mut admin_headers = HeaderMap::new();
        admin_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {TEST_ADMIN_KEY}")).unwrap(),
        );
        assert!(!key_matches_digest(
            &key_digest("data-plane-only"),
            supplied_key(&admin_headers).unwrap()
        ));

        let mut data_headers = HeaderMap::new();
        data_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer data-plane-only"),
        );
        assert!(key_matches_digest(
            &key_digest("data-plane-only"),
            supplied_key(&data_headers).unwrap()
        ));
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
        let live = LiveConfig::legacy(config);
        AppState {
            live: Arc::new(std::sync::RwLock::new(live)),
            http: transport::test_client().expect("audit HTTP client"),
            db: None,
            control_plane: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(30)),
            admin_auth: AdminAuth::test(),
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
            logs.lines()
                .filter(|line| {
                    line.contains("route has degraded features due to adapter conversion")
                        && line.contains("route_id=degraded-fallback-route")
                })
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
        Request::builder()
            .uri(uri)
            .header("authorization", format!("Bearer {TEST_ADMIN_KEY}"))
            .body(Body::empty())
            .expect("admin request")
    }

    fn usage_test_state(database: db::Database) -> AppState {
        let config = Arc::new(GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![],
            accounts: vec![],
            routes: vec![],
        });
        let live = LiveConfig::legacy(config);
        AppState {
            live: Arc::new(std::sync::RwLock::new(live)),
            http: transport::test_client().expect("HTTP client"),
            db: Some(database),
            control_plane: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
            admin_auth: AdminAuth::test(),
        }
    }

    #[test]
    fn usage_query_validates_utc_boundaries_and_dimensions() {
        let query = HashMap::from([
            ("from".into(), "2026-01-01T08:00:00+08:00".into()),
            ("to".into(), "2026-01-02T00:00:00Z".into()),
            ("logical_model".into(), "logical-a".into()),
            ("upstream_model".into(), "upstream-a".into()),
            ("source_id".into(), "source-a".into()),
            ("client_source".into(), "cli-a".into()),
            ("status".into(), "failure".into()),
            ("breakdown".into(), "client_source".into()),
        ]);
        let parsed = parse_usage_query(&query).expect("valid usage query");
        assert_eq!(
            parsed.filter.from.unwrap().to_rfc3339(),
            "2026-01-01T00:00:00+00:00"
        );
        assert_eq!(parsed.filter.success, Some(false));
        assert_eq!(parsed.filter.source_id.as_deref(), Some("source-a"));
        assert_eq!(parsed.filter.client_source.as_deref(), Some("cli-a"));
        assert_eq!(parsed.breakdown, "client_source");

        assert_eq!(
            parse_usage_query(&HashMap::from([("source".into(), "legacy".into())])).unwrap_err(),
            "source is no longer supported; use source_id or client_source"
        );

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
        assert!(header.contains("provider_id,source_id,client_source,account_id"));
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
            source_id: "source-api".into(),
            client_source: "api-test".into(),
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
                source_id: "source-primary".into(),
                account_id: "account-api".into(),
                upstream_model_id: Some("upstream-api".into()),
                status_code: 429,
                success: false,
                latency_ms: 10,
            },
            db::UsageAttempt {
                attempt_no: 1,
                provider_id: "provider-api".into(),
                source_id: "source-api".into(),
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
                "/admin/usage/summary?logical_model={logical_model}&source_id=source-api&client_source=api-test"
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

        let breakdown = app
            .clone()
            .oneshot(admin_request(&format!(
                "/admin/usage/breakdown?logical_model={logical_model}&breakdown=source_id"
            )))
            .await
            .expect("Source breakdown response");
        let breakdown: Value = serde_json::from_slice(
            &axum::body::to_bytes(breakdown.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(breakdown["dimension"], "source_id");
        assert_eq!(breakdown["data"][0]["key"], "source-api");

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
        assert_eq!(events["data"][0]["source_id"], "source-api");
        assert_eq!(events["data"][0]["client_source"], "api-test");
        assert!(events["data"][0].get("prompt").is_none());

        let detail = app
            .clone()
            .oneshot(admin_request(&format!(
                "/admin/usage/events/{}",
                event.request_id
            )))
            .await
            .expect("event detail response");
        let detail: Value = serde_json::from_slice(
            &axum::body::to_bytes(detail.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(detail["data"]["source_id"], "source-api");
        assert_eq!(detail["data"]["client_source"], "api-test");
        assert_eq!(detail["attempts"][0]["source_id"], "source-primary");
        assert_eq!(detail["attempts"][1]["source_id"], "source-api");

        let export = app
            .clone()
            .oneshot(admin_request(&format!(
                "/admin/usage/export?logical_model={logical_model}&source_id=source-api&client_source=api-test&format=csv"
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
        let export = String::from_utf8_lossy(&export);
        assert!(export.contains(&event.request_id));
        assert!(export.contains("provider_id,source_id,client_source,account_id"));

        let json_export = app
            .oneshot(admin_request(&format!(
                "/admin/usage/export?logical_model={logical_model}&source_id=source-api&client_source=api-test&format=json"
            )))
            .await
            .expect("JSON export response");
        assert_eq!(json_export.status(), StatusCode::OK);
        let json_export: Value = serde_json::from_slice(
            &axum::body::to_bytes(json_export.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(json_export["data"][0]["source_id"], "source-api");
        assert_eq!(json_export["data"][0]["client_source"], "api-test");
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
        let live = LiveConfig::legacy(config);
        AppState {
            live: Arc::new(std::sync::RwLock::new(live)),
            http: transport::test_client().expect("http client"),
            db: None,
            control_plane: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
            admin_auth: AdminAuth::test(),
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

#[cfg(test)]
mod ops_api_tests {
    use super::*;
    use axum::body::to_bytes;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn isolated_database() -> (db::Database, sqlx::PgPool, sqlx::PgPool, String) {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for the ops API test");
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect ops API test admin database");
        let schema = format!("ops_api_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create ops API schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse TEST_DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect ops API schema");
        let database = db::Database::from_test_pool(pool.clone())
            .await
            .expect("migrate ops API schema");
        (database, pool, admin, schema)
    }

    fn admin_request(method: &str, uri: &str, body: Body) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {TEST_ADMIN_KEY}"))
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .expect("ops API request")
    }

    async fn response_json(response: Response<Body>) -> Value {
        serde_json::from_slice(
            &to_bytes(response.into_body(), 16 * 1024 * 1024)
                .await
                .expect("ops API response body"),
        )
        .expect("ops API JSON response")
    }

    fn db_state(database: db::Database, control_plane: control_plane::ControlPlane) -> AppState {
        let config = Arc::new(GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: Vec::new(),
            accounts: Vec::new(),
            routes: Vec::new(),
        });
        AppState {
            live: Arc::new(std::sync::RwLock::new(LiveConfig::legacy(config))),
            http: transport::test_client().expect("ops API HTTP client"),
            db: Some(database),
            control_plane: Some(control_plane),
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
            admin_auth: AdminAuth::test(),
        }
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL; run with the PostgreSQL regression suite"]
    async fn postgres_ops_api_exposes_progress_versions_and_verified_restore() {
        let (database, pool, admin, schema) = isolated_database().await;
        let control_plane = control_plane::ControlPlane::new(&database, "127.0.0.1:0");
        let app = application(db_state(database.clone(), control_plane));

        let policies = app
            .clone()
            .oneshot(admin_request(
                "GET",
                "/admin/retention/policies",
                Body::empty(),
            ))
            .await
            .expect("retention policies response");
        assert_eq!(policies.status(), StatusCode::OK);
        let policies = response_json(policies).await;
        assert_eq!(policies["version"], "v1");
        assert_eq!(policies["timezone"], "UTC");
        assert_eq!(policies["data"].as_array().unwrap().len(), 4);

        let dry_run = app
            .clone()
            .oneshot(admin_request(
                "POST",
                "/admin/retention/cleanup",
                Body::from(r#"{"dry_run":true,"operation_id":"api-dry-run"}"#),
            ))
            .await
            .expect("dry-run response");
        assert_eq!(dry_run.status(), StatusCode::OK);
        let dry_run = response_json(dry_run).await;
        assert_eq!(dry_run["data"]["dry_run"], true);

        let schema_response = app
            .clone()
            .oneshot(admin_request("GET", "/admin/ops/schema", Body::empty()))
            .await
            .expect("schema response");
        let schema_response = response_json(schema_response).await;
        assert_eq!(schema_response["data"]["schema_version"], 11);
        assert_eq!(schema_response["data"]["migration_version"], 11);

        let export_response = app
            .clone()
            .oneshot(admin_request(
                "GET",
                "/admin/control-plane/export",
                Body::empty(),
            ))
            .await
            .expect("export response");
        assert_eq!(export_response.status(), StatusCode::OK);
        let export = response_json(export_response).await;
        assert_eq!(export["data"]["timezone"], "UTC");
        assert!(export["data"].get("credential_ciphertext").is_none());

        let import_payload = json!({"data": export["data"].clone(), "checksum": export["checksum"].clone(), "replace": true});
        let import_response = app
            .clone()
            .oneshot(admin_request(
                "POST",
                "/admin/control-plane/import",
                Body::from(serde_json::to_vec(&import_payload).unwrap()),
            ))
            .await
            .expect("import response");
        assert_eq!(import_response.status(), StatusCode::OK);
        let import = response_json(import_response).await;
        assert_eq!(import["verified"], true);

        let audit = app
            .clone()
            .oneshot(admin_request(
                "GET",
                "/admin/audit?operation_id=api-dry-run",
                Body::empty(),
            ))
            .await
            .expect("audit response");
        assert_eq!(audit.status(), StatusCode::OK);
        let audit = response_json(audit).await;
        assert!(!audit["data"].as_array().unwrap().is_empty());

        drop(app);
        drop(database);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop ops API schema");
        admin.close().await;
    }
}

#[cfg(test)]
mod stream_contract_e2e_tests {
    use super::*;
    use axum::{body::to_bytes, extract::Request, http::header, routing::any, Router};
    use futures_util::stream;
    use std::{collections::HashMap, time::Duration};

    async fn spawn_native_upstream() -> String {
        let app = Router::new().route(
            "/{*path}",
            any(|request: Request| async move {
                let path = request.uri().path().to_owned();
                let payload = if path.ends_with("/chat/completions") {
                    "data: {\"id\":\"chat-1\",\"choices\":[]}\n\ndata: [DONE]\n\n"
                } else if path.ends_with("/messages") {
                    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
                } else {
                    "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n"
                };
                let chunks = stream::once(async move {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    Ok::<Bytes, std::io::Error>(Bytes::from_static(payload.as_bytes()))
                });
                Response::builder()
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .body(Body::from_stream(chunks))
                    .expect("native SSE response")
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind native e2e upstream");
        let address = listener.local_addr().expect("native e2e address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve native e2e upstream")
        });
        format!("http://{address}")
    }

    fn native_provider(base_url: &str) -> config::ProviderConfig {
        config::ProviderConfig {
            id: "native-e2e".into(),
            name: "Native E2E".into(),
            base_url: base_url.into(),
            models: vec!["m".into()],
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

    fn native_state(base_url: &str) -> AppState {
        let config = Arc::new(GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![native_provider(base_url)],
            accounts: vec![config::AccountConfig {
                id: "native-account".into(),
                provider_id: "native-e2e".into(),
                display_name: "Native E2E".into(),
                credential_env: None,
                credential: Some("test-secret".into()),
                enabled: true,
                weight: 100,
                protocol_capabilities: HashMap::new(),
                capabilities: None,
                model_overrides: HashMap::new(),
                model_map: HashMap::new(),
            }],
            routes: vec![config::RouteConfig {
                id: "native-e2e-route".into(),
                model: "m".into(),
                provider_id: "native-e2e".into(),
                protocols: vec![
                    Protocol::OpenAiChatCompletions,
                    Protocol::OpenAiResponses,
                    Protocol::AnthropicMessages,
                ],
                primary_account_id: "native-account".into(),
                fallback_accounts: vec![],
                strategy: "primary_then_weighted_fallback".into(),
                mode: "native".into(),
                adapter: None,
                allow_lossy_conversion: false,
            }],
        });
        AppState {
            live: Arc::new(std::sync::RwLock::new(LiveConfig::legacy(config))),
            http: transport::test_client().expect("native e2e HTTP client"),
            db: None,
            control_plane: None,
            health: health::HealthRegistry::new(Duration::from_secs(1)),
            admin_auth: AdminAuth::test(),
        }
    }

    #[tokio::test]
    async fn native_three_protocol_streams_share_the_sse_contract() {
        let _environment_lock = ENV_LOCK.lock().await;
        let _heartbeat = EnvRestore::set("GATEWAY_SSE_HEARTBEAT_INTERVAL_MS", "5");
        let _connection = EnvRestore::set("GATEWAY_SSE_CONNECTION_TIMEOUT_MS", "500");
        let _first = EnvRestore::set("GATEWAY_SSE_FIRST_EVENT_TIMEOUT_MS", "200");
        let _idle = EnvRestore::set("GATEWAY_SSE_IDLE_TIMEOUT_MS", "200");
        let _total = EnvRestore::set("GATEWAY_SSE_TOTAL_TIMEOUT_MS", "1000");
        let base_url = spawn_native_upstream().await;
        let state = native_state(&base_url);
        for protocol in [
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ] {
            let mut headers = HeaderMap::new();
            if let Ok(key) = std::env::var("GATEWAY_API_KEY") {
                headers.insert(
                    header::AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {key}"))
                        .expect("native e2e auth header"),
                );
            }
            let response = proxy(
                state.clone(),
                headers,
                Bytes::from(
                    serde_json::to_vec(&json!({
                        "model": "m",
                        "stream": true,
                        "input": "hello"
                    }))
                    .expect("native e2e body"),
                ),
                protocol,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let body = String::from_utf8_lossy(
                &to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .expect("native e2e body bytes"),
            )
            .into_owned();
            assert!(
                body.contains(": gateway-heartbeat"),
                "heartbeat missing for {protocol}: {body}"
            );
            assert!(!body.contains(": gateway-heartbeat\\ndata:"));
            match protocol {
                Protocol::OpenAiChatCompletions => assert!(body.contains("[DONE]")),
                Protocol::OpenAiResponses => assert!(body.contains("response.completed")),
                Protocol::AnthropicMessages => assert!(body.contains("message_stop")),
            }
        }
    }
}
