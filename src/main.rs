mod api;
mod control_plane;
mod domain;
pub(crate) mod http;
mod infra;
mod proxy;
pub(crate) mod source_url;
mod state;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, Response, StatusCode},
    middleware::{self, Next},
    routing::{get, post, put},
    Router,
};
use serde_json::{json, Value};
use tower_http::{services::ServeDir, trace::TraceLayer};

use domain::config::GatewayConfig;
use http::client as source_http_client;
use infra::{audit, db, health, observability, ops, secrets};
#[cfg(test)]
use proxy::transport;
use state::{error_response, AdminAuth, AppState, LiveConfig};

#[cfg(test)]
use api::{
    admin::admin_capabilities_response,
    health_admin::admin_health,
    proxy::{models, responses},
    usage::{csv_field, parse_usage_query, usage_events_csv},
};
#[cfg(test)]
use axum::{
    body::Bytes,
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue},
};
#[cfg(test)]
use domain::{config, protocol::Protocol};
#[cfg(test)]
use proxy::service::proxy as proxy_fn;
#[cfg(test)]
use proxy::usage;
#[cfg(test)]
use state::{key_digest, key_matches_digest, supplied_key, EnvRestore, ENV_LOCK, TEST_ADMIN_KEY};
#[cfg(test)]
use tower::ServiceExt;
#[cfg(test)]
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command_line = std::env::args().skip(1).collect::<Vec<_>>();
    if command_line.first().is_some_and(|value| value == "ops") {
        return run_ops_cli(&command_line[1..]).await;
    }
    let _otel_provider = observability::init_tracing();
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
        database.pool().clone(),
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
    let catalog =
        control_plane::model_catalog::ModelCatalogRepository::new(database.pool().clone());
    control_plane::model_catalog::install_builtin_presets(&catalog).await?;
    let control_plane = control_plane::ControlPlane::with_url_policy(
        database.pool().clone(),
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
    let health = health::HealthRegistry::with_database_config(
        database.clone(),
        health::HealthConfig::from_env(),
    );
    health.restore().await?;
    let secrets = secrets::SecretResolver::from_env().unwrap_or_else(|err| {
        tracing::warn!(
            ?err,
            "credential master key unavailable; encrypted credentials will fail at request time"
        );
        secrets::SecretResolver::empty()
    });
    let prometheus_handle = observability::prometheus_handle();
    observability::spawn_upkeep(prometheus_handle.clone());
    observability::set_snapshot_revision(live.revision);
    let state = AppState {
        live: Arc::new(std::sync::RwLock::new(live)),
        http: source_http_client(source_url_policy.clone())?,
        db: Some(database.clone()),
        control_plane: Some(control_plane),
        health,
        admin_auth,
        secrets,
        prometheus_handle,
    };
    spawn_health_probe_loop(state.clone());
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
            let control_plane = control_plane::ControlPlane::with_url_policy(
                database.pool().clone(),
                listen_addr,
                policy,
            );
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
            let control_plane = control_plane::ControlPlane::with_url_policy(
                database.pool().clone(),
                listen_addr,
                policy,
            );
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

fn spawn_health_probe_loop(state: AppState) {
    let enabled = std::env::var("GATEWAY_HEALTH_PROBE_ENABLED")
        .ok()
        .map(|value| !matches!(value.as_str(), "0" | "false" | "FALSE" | "no"))
        .unwrap_or(true);
    if !enabled || state.health.database().is_none() {
        return;
    }
    let interval = state.health.config().probe_interval;
    let on_startup = std::env::var("GATEWAY_HEALTH_PROBE_ON_STARTUP")
        .ok()
        .is_some_and(|value| !matches!(value.as_str(), "0" | "false" | "FALSE" | "no"));
    tokio::spawn(async move {
        if on_startup {
            run_health_probes_once(&state).await;
        }
        loop {
            tokio::time::sleep(interval).await;
            run_health_probes_once(&state).await;
        }
    });
}

async fn run_health_probes_once(state: &AppState) {
    let Some(database) = state.health.database() else {
        return;
    };
    let targets = match database.health_probe_targets().await {
        Ok(targets) => targets,
        Err(error) => {
            tracing::warn!(%error, "failed to enumerate periodic health probes");
            return;
        }
    };
    let now = state.health.now();
    for (account_id, _source_id, protocol) in targets {
        let current = state.health.get_health(&account_id).await;
        let due = current.last_probe_at.is_none_or(|last| {
            now.signed_duration_since(last)
                .to_std()
                .map(|elapsed| elapsed >= state.health.config().probe_interval)
                .unwrap_or(true)
        });
        if !due || (current.cooldown_remaining_ms > 0 && !current.stale) {
            continue;
        }
        match state
            .health
            .probe_account(
                &state.http,
                &account_id,
                protocol,
                None,
                "periodic_health_probe",
            )
            .await
        {
            Ok(outcome) => tracing::info!(
                account_id = %outcome.account_id,
                %protocol,
                status = %outcome.connection_test.status,
                "periodic account health probe completed"
            ),
            Err(error) => tracing::warn!(
                account_id = %account_id,
                %protocol,
                code = error.code(),
                "periodic account health probe failed"
            ),
        }
    }
}

fn application(state: AppState) -> Router {
    use api::{admin, discovery, health_admin, keys, ops, proxy, usage};

    let discovery_router = discovery::auxiliary_router_with_health(
        state.db.clone(),
        state.http.clone(),
        state.admin_auth.clone(),
        state.health.clone(),
    );
    let admin_api = Router::new()
        .route("/admin/keys", get(keys::list_keys).post(keys::create_key))
        .route(
            "/admin/keys/{id}",
            get(keys::get_key).delete(keys::revoke_key),
        )
        .route("/admin/keys/{id}/rotate", post(keys::rotate_key))
        .route("/admin/keys/{id}/value", get(keys::reveal_key))
        .route("/admin/keys/{id}/revoke", post(keys::revoke_key))
        .route("/admin/usage/summary", get(usage::usage_summary))
        .route("/admin/usage/timeseries", get(usage::usage_timeseries))
        .route("/admin/usage/breakdown", get(usage::usage_breakdown))
        .route("/admin/usage/events", get(usage::usage_events))
        .route("/admin/usage/export", get(usage::usage_export))
        .route("/admin/usage/aggregate", get(usage::usage_aggregate))
        .route(
            "/admin/usage/events/{request_id}",
            get(usage::usage_event_detail),
        )
        .route(
            "/admin/retention/policies",
            get(ops::list_retention_policies).put(ops::update_retention_policies),
        )
        .route(
            "/admin/retention",
            get(ops::list_retention_policies).put(ops::update_retention_policies),
        )
        .route(
            "/admin/retention/cleanup",
            get(ops::list_retention_cleanups).post(ops::start_retention_cleanup),
        )
        .route(
            "/admin/retention/runs",
            get(ops::list_retention_cleanups).post(ops::start_retention_cleanup),
        )
        .route(
            "/admin/retention/cleanup/{id}",
            get(ops::get_retention_cleanup),
        )
        .route(
            "/admin/retention/runs/{id}",
            get(ops::get_retention_cleanup),
        )
        .route(
            "/admin/retention/cleanup/{id}/cancel",
            post(ops::cancel_retention_cleanup),
        )
        .route(
            "/admin/retention/cleanup/{id}/retry",
            post(ops::retry_retention_cleanup),
        )
        .route(
            "/admin/retention/runs/{id}/cancel",
            post(ops::cancel_retention_cleanup),
        )
        .route(
            "/admin/retention/runs/{id}/retry",
            post(ops::retry_retention_cleanup),
        )
        .route(
            "/admin/control-plane/export",
            get(ops::export_control_plane),
        )
        .route(
            "/admin/control-plane/import",
            post(ops::import_control_plane),
        )
        .route("/admin/backup/export", get(ops::export_control_plane))
        .route("/admin/backup/import", post(ops::import_control_plane))
        .route("/admin/audit", get(ops::list_audit_logs))
        .route("/admin/backups/{id}", get(ops::get_backup_run))
        .route("/admin/backups", get(ops::list_backup_runs))
        .route("/admin/backup/{id}", get(ops::get_backup_run))
        .route("/admin/ops/schema", get(ops::ops_schema_metadata))
        .route("/admin/schema", get(ops::ops_schema_metadata))
        .route(
            "/admin/sources",
            get(admin::list_sources).post(admin::create_source),
        )
        .route(
            "/admin/sources/{id}",
            get(admin::get_source)
                .put(admin::update_source)
                .delete(admin::delete_source),
        )
        .route(
            "/admin/sources/{id}/enabled",
            put(admin::set_source_enabled),
        )
        .route(
            "/admin/accounts",
            get(admin::list_accounts).post(admin::create_account),
        )
        .route(
            "/admin/accounts/{id}",
            get(admin::get_account)
                .put(admin::update_account)
                .delete(admin::delete_account),
        )
        .route(
            "/admin/accounts/{id}/enabled",
            put(admin::set_account_enabled),
        )
        .route("/admin/credentials/encrypt", post(keys::encrypt_credential))
        .route(
            "/admin/accounts/{id}/credentials/rotate",
            post(keys::rotate_account_credential),
        )
        .route(
            "/admin/logical-models",
            get(admin::list_logical_models).post(admin::create_logical_model),
        )
        .route(
            "/admin/logical-models/{id}",
            get(admin::get_logical_model)
                .put(admin::update_logical_model)
                .delete(admin::delete_logical_model),
        )
        .route(
            "/admin/logical-models/{id}/enabled",
            put(admin::set_logical_model_enabled),
        )
        .route(
            "/admin/model-bindings",
            get(admin::list_model_bindings).post(admin::create_model_binding),
        )
        .route(
            "/admin/model-bindings/{id}",
            get(admin::get_model_binding)
                .put(admin::update_model_binding)
                .delete(admin::delete_model_binding),
        )
        .route(
            "/admin/model-bindings/{id}/enabled",
            put(admin::set_model_binding_enabled),
        )
        .route(
            "/admin/routes",
            get(admin::list_routes).post(admin::create_route),
        )
        .route(
            "/admin/routes/{id}",
            get(admin::get_route)
                .put(admin::update_route)
                .delete(admin::delete_route),
        )
        .route("/admin/routes/{id}/enabled", put(admin::set_route_enabled))
        .route("/admin/config/reload", post(admin::reload_config))
        .route("/admin/capabilities", get(admin::admin_capabilities))
        .route("/admin/health", get(health_admin::admin_health))
        .route(
            "/admin/health/probe",
            post(health_admin::admin_health_probe),
        )
        .route(
            "/admin/health/probes",
            post(health_admin::admin_health_probes),
        )
        .route(
            "/admin/health/{id}",
            get(health_admin::admin_account_health),
        )
        .route(
            "/admin/health/{id}/probe",
            post(health_admin::admin_account_probe),
        )
        .route(
            "/admin/accounts/{id}/probe",
            post(health_admin::admin_account_probe),
        )
        .route(
            "/admin/routes/{protocol}/{model}",
            get(admin::resolve_route),
        )
        .with_state(state.clone())
        .merge(discovery_router)
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            audit_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.admin_auth.clone(),
            require_admin_auth,
        ));
    Router::new()
        .route("/healthz", get(proxy::healthz))
        .route("/metrics", get(proxy::metrics_handler))
        .route("/v1/models", get(proxy::models))
        .route("/v1/chat/completions", post(proxy::chat_completions))
        .route("/v1/responses", post(proxy::responses))
        .route("/v1/messages", post(proxy::messages))
        .nest_service("/admin", ServeDir::new("web/dist"))
        .with_state(state)
        .merge(admin_api)
        .layer(TraceLayer::new_for_http())
}

async fn audit_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    let method = request.method().clone();
    if !should_audit_admin_request(&method, request.uri().path()) {
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 4 * 1024 * 1024)
        .await
        .unwrap_or_default();
    let payload: Option<Value> = serde_json::from_slice(&bytes).ok();
    let context =
        audit::context_from_request(&method, parts.uri.path(), &parts.headers, payload.as_ref());
    let pool = state.db.as_ref().map(|db| db.pool().clone());
    let request = Request::from_parts(parts, Body::from(bytes));
    audit::scope(context.clone(), async move {
        let response = next.run(request).await;
        if !context.was_recorded() {
            if let Some(pool) = &pool {
                let status_str = response.status().as_u16().to_string();
                if response.status().is_success() {
                    let _ = audit::append_current_success_pool(pool).await;
                } else {
                    let _ = audit::append_current_failure_pool(pool, &status_str, None).await;
                }
            }
        }
        response
    })
    .await
}

fn should_audit_admin_request(method: &Method, path: &str) -> bool {
    match *method {
        Method::HEAD | Method::OPTIONS => false,
        Method::GET => {
            let segments = path.trim_matches('/').split('/').collect::<Vec<_>>();
            matches!(segments.as_slice(), ["admin", "keys", _, "value"])
        }
        _ => true,
    }
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

// All handler functions have been extracted to api/ submodules:
// - api::proxy       — data-plane endpoints and proxy pipeline
// - api::keys        — virtual key management
// - api::usage       — usage query/export endpoints
// - api::ops         — retention, backup, audit, schema
// - api::admin       — control-plane CRUD, capabilities, config reload
// - api::health_admin — health management endpoints
// - api::discovery   — provider preset and model discovery

#[cfg(test)]
mod admin_auth_tests {
    use super::*;
    use axum::{body::to_bytes, http::header};

    const ADMIN_API_ROUTES: &[(&str, &str)] = &[
        ("GET", "/admin/keys"),
        ("POST", "/admin/keys"),
        ("GET", "/admin/keys/1"),
        ("DELETE", "/admin/keys/1"),
        ("POST", "/admin/keys/1/rotate"),
        ("GET", "/admin/keys/1/value"),
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
        ("POST", "/admin/credentials/encrypt"),
        ("POST", "/admin/accounts/account-id/credentials/rotate"),
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
        ("GET", "/admin/health/account-id"),
        ("POST", "/admin/health/probe"),
        ("POST", "/admin/health/probes"),
        ("POST", "/admin/health/account-id/probe"),
        ("POST", "/admin/accounts/account-id/probe"),
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

    #[test]
    fn audit_includes_virtual_key_reveal_but_skips_ordinary_reads() {
        assert!(should_audit_admin_request(
            &Method::GET,
            "/admin/keys/1/value"
        ));
        assert!(!should_audit_admin_request(&Method::GET, "/admin/keys/1"));
        assert!(!should_audit_admin_request(
            &Method::GET,
            "/admin/usage/summary"
        ));
        assert!(should_audit_admin_request(&Method::POST, "/admin/keys"));
    }

    fn state(admin_auth: AdminAuth) -> AppState {
        let config = Arc::new(GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: Vec::new(),
            accounts: Vec::new(),
            routes: Vec::new(),
        });
        AppState {
            live: Arc::new(std::sync::RwLock::new(LiveConfig::legacy(config))),
            http: http::test_client().expect("admin auth HTTP client"),
            db: None,
            control_plane: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
            admin_auth,
            secrets: secrets::SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
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
mod health_api_tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::Duration;

    fn health_state() -> AppState {
        let provider = config::ProviderConfig {
            id: "health-provider".into(),
            name: "Health Provider".into(),
            base_url: "https://health.example".into(),
            models: vec!["health-model".into()],
            native_protocols: vec![Protocol::OpenAiChatCompletions],
            endpoints: HashMap::from([(
                Protocol::OpenAiChatCompletions,
                "/v1/chat/completions".into(),
            )]),
            capabilities: config::Capabilities::native(),
            protocol_capabilities: HashMap::new(),
            model_overrides: HashMap::new(),
        };
        let account = config::AccountConfig {
            id: "health-account".into(),
            provider_id: "health-provider".into(),
            display_name: "Health Account".into(),
            credential_env: None,
            credential_ciphertext: None,
            credential: None,
            enabled: true,
            weight: 100,
            protocol_capabilities: HashMap::new(),
            capabilities: None,
            model_overrides: HashMap::new(),
            model_map: HashMap::new(),
        };
        let config = Arc::new(GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![provider],
            accounts: vec![account],
            routes: vec![],
        });
        AppState {
            live: Arc::new(std::sync::RwLock::new(LiveConfig::legacy(config))),
            http: http::test_client().expect("health API HTTP client"),
            db: None,
            control_plane: None,
            health: health::HealthRegistry::new(Duration::from_secs(1)),
            admin_auth: AdminAuth::test(),
            secrets: secrets::SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
        }
    }

    #[tokio::test]
    async fn health_api_exposes_transition_source_timestamp_and_stale() {
        let state = health_state();
        state.health.mark_failure("health-account").await;
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-admin-key"),
        );
        let response = admin_health(State(state), headers).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("health API body");
        let body: Value = serde_json::from_slice(&body).expect("health API JSON");
        let account = &body["data"][0];
        assert_eq!(account["health"]["source"], "passive");
        assert!(account["health"]["updated_at"].is_string());
        assert!(account["health"].get("stale").is_some());
        assert_eq!(account["health_source"], "passive");
        assert!(account.get("health_updated_at").is_some());
    }

    #[tokio::test]
    async fn health_api_is_reached_through_admin_http_route() {
        let state = health_state();
        let response = application(state)
            .oneshot(
                Request::builder()
                    .uri("/admin/health")
                    .header(axum::http::header::AUTHORIZATION, "Bearer test-admin-key")
                    .body(Body::empty())
                    .expect("health HTTP request"),
            )
            .await
            .expect("health HTTP response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read health HTTP body");
        let body: Value = serde_json::from_slice(&body).expect("health HTTP JSON");
        assert_eq!(body["fact_source"], "memory");
        assert_eq!(body["data"].as_array().unwrap().len(), 1);
    }
}

#[cfg(test)]
mod audit_closeout_tests {
    use super::*;
    use axum::{body::to_bytes, extract::Request, http::header, Router};
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
            credential_ciphertext: None,
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
            http: http::test_client().expect("audit HTTP client"),
            db: None,
            control_plane: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(30)),
            admin_auth: AdminAuth::test(),
            secrets: secrets::SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
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

    fn empty_routes_config() -> GatewayConfig {
        GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![provider("audit-provider", "https://unused.invalid".into())],
            accounts: vec![account("audit-primary", "audit-provider")],
            routes: vec![],
        }
    }

    fn anthropic_unauthorized_request() -> Request<Body> {
        // Configure `GATEWAY_API_KEY=correct` and send a wrong bearer so
        // `authorized_with_db` rejects without falling through to the DB.
        Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header(CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer wrong-key")
            .body(Body::from(r#"{"model":"audit-model","messages":[]}"#))
            .expect("anthropic unauthorized request")
    }

    fn openai_chat_unauthorized_request() -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header(CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer wrong-key")
            .body(Body::from(r#"{"model":"audit-model","messages":[]}"#))
            .expect("openai chat unauthorized request")
    }

    fn openai_responses_unauthorized_request() -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header(CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer wrong-key")
            .body(Body::from(r#"{"model":"audit-model","input":"hello"}"#))
            .expect("openai responses unauthorized request")
    }

    async fn assert_data_plane_error_envelope(
        response: Response<Body>,
        expected_status: StatusCode,
        expected_code: &str,
        body_asserts: impl FnOnce(&Value),
    ) {
        assert_eq!(response.status(), expected_status);
        // Capture the request_id header before the response body is consumed.
        let header_request_id = response
            .headers()
            .get("x-request-id")
            .expect("x-request-id header missing on data-plane error")
            .to_str()
            .expect("x-request-id header must be ASCII")
            .to_owned();
        let body = json_body(response).await;
        let body_request_id = body["request_id"]
            .as_str()
            .expect("top-level request_id must be present on data-plane errors")
            .to_owned();
        assert!(
            Uuid::parse_str(&body_request_id).is_ok(),
            "request_id must be a UUID: {body_request_id}"
        );
        assert_eq!(body_request_id, header_request_id);
        // Either the legacy `error.code` (Anthropic/OpenAI envelopes) or the
        // top-level `code` (admin envelope, not used here) is acceptable.
        let envelope_code = body["error"]["code"].as_str().unwrap_or_default();
        assert_eq!(envelope_code, expected_code);
        body_asserts(&body);
    }

    #[tokio::test]
    async fn data_plane_anthropic_messages_401_uses_sdk_standard_envelope() {
        let _environment_lock = ENV_LOCK.lock().await;
        // Set the gateway key and send a wrong bearer so the data-plane auth
        // layer rejects without falling through to the DB.
        let _key = EnvRestore::set("GATEWAY_API_KEY", "audit-correct-key");
        let response = application(state(empty_routes_config()))
            .oneshot(anthropic_unauthorized_request())
            .await
            .expect("anthropic unauthorized response");
        assert_data_plane_error_envelope(
            response,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            |body| {
                assert_eq!(body["type"], "error");
                assert_eq!(body["error"]["type"], "authentication_error");
                assert!(body["error"]["message"].is_string());
            },
        )
        .await;
    }

    #[tokio::test]
    async fn data_plane_anthropic_messages_404_uses_sdk_standard_envelope() {
        let _environment_lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", "audit-anthropic-404");
        let response = application(state(empty_routes_config()))
            .oneshot(proxy_request("/v1/messages"))
            .await
            .expect("anthropic not-found response");
        assert_data_plane_error_envelope(
            response,
            StatusCode::NOT_FOUND,
            "route_not_found",
            |body| {
                assert_eq!(body["type"], "error");
                assert_eq!(body["error"]["type"], "not_found_error");
                assert!(body["error"]["message"].is_string());
            },
        )
        .await;
    }

    #[tokio::test]
    async fn data_plane_openai_chat_401_keeps_envelope_and_attaches_request_id() {
        let _environment_lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", "audit-correct-key");
        let response = application(state(empty_routes_config()))
            .oneshot(openai_chat_unauthorized_request())
            .await
            .expect("openai chat unauthorized response");
        assert_data_plane_error_envelope(
            response,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            |body| {
                // OpenAI envelopes stay as-is for client compatibility; only
                // the optional top-level request_id and x-request-id header
                // are added on top.
                assert_eq!(body["error"]["type"], "unauthorized");
                assert!(body["error"]["message"].is_string());
                assert!(body["type"].is_null(), "no top-level type for OpenAI");
            },
        )
        .await;
    }

    #[tokio::test]
    async fn data_plane_openai_responses_401_keeps_envelope_and_attaches_request_id() {
        let _environment_lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", "audit-correct-key");
        let response = application(state(empty_routes_config()))
            .oneshot(openai_responses_unauthorized_request())
            .await
            .expect("openai responses unauthorized response");
        assert_data_plane_error_envelope(
            response,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            |body| {
                assert_eq!(body["error"]["type"], "unauthorized");
                assert!(body["error"]["message"].is_string());
                assert!(body["type"].is_null(), "no top-level type for OpenAI");
            },
        )
        .await;
    }

    fn models_request(authorization: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("GET")
            .uri("/v1/models")
            .header(CONTENT_TYPE, "application/json");
        if let Some(key) = authorization {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
        }
        builder.body(Body::empty()).expect("models request")
    }

    #[tokio::test]
    async fn data_plane_models_rejects_unauthenticated_requests() {
        let _environment_lock = ENV_LOCK.lock().await;
        // Configure the static key but send no Authorization header so the
        // gateway fails closed the same way the three POST endpoints do.
        // (Without `GATEWAY_API_KEY` and without a DB the proxy is
        // fail-open by design — see `authorized_with_db` — so we cannot
        // exercise the 401 path with no env wiring at all.)
        let _key = EnvRestore::set("GATEWAY_API_KEY", "audit-models-correct");
        let response = application(state(empty_routes_config()))
            .oneshot(models_request(None))
            .await
            .expect("models unauthorized response");
        assert_data_plane_error_envelope(
            response,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            |body| {
                assert_eq!(body["error"]["type"], "unauthorized");
                assert!(body["error"]["message"].is_string());
            },
        )
        .await;
    }

    #[tokio::test]
    async fn data_plane_models_rejects_invalid_bearer() {
        let _environment_lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", "audit-models-correct");
        let response = application(state(empty_routes_config()))
            .oneshot(models_request(Some("wrong-key")))
            .await
            .expect("models wrong-bearer response");
        assert_data_plane_error_envelope(
            response,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            |body| {
                assert_eq!(body["error"]["type"], "unauthorized");
                assert!(body["error"]["message"].is_string());
            },
        )
        .await;
    }

    #[tokio::test]
    async fn data_plane_models_accepts_valid_bearer() {
        let _environment_lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", "audit-models-correct");
        let response = application(state(empty_routes_config()))
            .oneshot(models_request(Some("audit-models-correct")))
            .await
            .expect("models authorized response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("x-request-id"),
            None,
            "200 models response must not carry an error envelope's x-request-id"
        );
        let body: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), 64 * 1024)
                .await
                .expect("models authorized body"),
        )
        .expect("models authorized JSON");
        assert_eq!(body["object"], "list");
        assert!(body["data"].is_array());
        // Assert the OpenAI catalogue shape, not the contents.  Production
        // deployments with at least one healthy Binding will see entries;
        // the fixture here may have one or zero depending on the runtime
        // snapshot, so the data length is intentionally not asserted.
        assert!(
            body["data"]
                .as_array()
                .unwrap()
                .iter()
                .all(|model| model.get("id").is_some()
                    && model["object"] == "model"
                    && model["owned_by"] == "gateway"),
            "every model entry must follow the OpenAI catalogue shape"
        );
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
            http: http::test_client().expect("HTTP client"),
            db: Some(database),
            control_plane: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
            admin_auth: AdminAuth::test(),
            secrets: secrets::SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
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
            fallback_reason: None,
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
    use crate::usage::UsageReport;
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
                credential_ciphertext: None,
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
            http: http::test_client().expect("http client"),
            db: None,
            control_plane: None,
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
            admin_auth: AdminAuth::test(),
            secrets: secrets::SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
        }
    }

    async fn invoke_responses_with_usage(
        state: AppState,
        body: &str,
    ) -> (StatusCode, String, Option<UsageReport>) {
        let response =
            responses(State(state), HeaderMap::new(), Bytes::from(body.to_owned())).await;
        let status = response.status();
        let usage = transport::usage_from_response(&response);
        let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .expect("response body");
        (status, String::from_utf8_lossy(&bytes).into_owned(), usage)
    }

    async fn invoke_responses(state: AppState, body: &str) -> (StatusCode, String) {
        let (status, body, _) = invoke_responses_with_usage(state, body).await;
        (status, body)
    }

    #[tokio::test]
    async fn embedded_kimi_adapter_non_stream_preserves_thinking_and_web_search() {
        let _environment_lock = ENV_LOCK.lock().await;
        // Force the "no auth configured" path so other tests that set
        // `GATEWAY_API_KEY` cannot make this request return 401.
        let _unset = EnvRestore::unset("GATEWAY_API_KEY");
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
        let (status, body, usage) = invoke_responses_with_usage(
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
        let usage = usage.expect("embedded adapter should attach a usage report");
        assert_eq!(usage.source, "upstream");
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 4);
        assert_eq!(usage.reasoning_tokens, 1);
        assert_eq!(usage.cached_tokens, 2);
        assert_eq!(usage.total_tokens, 16);
        assert!(recorded
            .lock()
            .expect("recorded body mutex")
            .contains("messages"));
    }

    #[tokio::test]
    async fn embedded_kimi_adapter_stream_translates_sse_events() {
        let _environment_lock = ENV_LOCK.lock().await;
        let _unset = EnvRestore::unset("GATEWAY_API_KEY");
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
            http: http::test_client().expect("ops API HTTP client"),
            db: Some(database),
            control_plane: Some(control_plane),
            health: health::HealthRegistry::new(std::time::Duration::from_secs(1)),
            admin_auth: AdminAuth::test(),
            secrets: secrets::SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
        }
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL; run with the PostgreSQL regression suite"]
    async fn postgres_ops_api_exposes_progress_versions_and_verified_restore() {
        let (database, pool, admin, schema) = isolated_database().await;
        let control_plane =
            control_plane::ControlPlane::new(database.pool().clone(), "127.0.0.1:0");
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
        assert!(schema_response["data"]["schema_version"].as_i64().unwrap() >= 12);
        assert!(
            schema_response["data"]["migration_version"]
                .as_i64()
                .unwrap()
                >= 12
        );

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
                credential_ciphertext: None,
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
            http: http::test_client().expect("native e2e HTTP client"),
            db: None,
            control_plane: None,
            health: health::HealthRegistry::new(Duration::from_secs(1)),
            admin_auth: AdminAuth::test(),
            secrets: secrets::SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
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
            let response = proxy_fn(
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

#[cfg(test)]
mod multi_turn_tool_tests {
    //! End-to-end coverage for multi-turn tool use across the three data-plane
    //! protocols.
    //!
    //! Each test drives two independent logical requests through the gateway
    //! against a mock upstream that emits a tool call on the first turn and
    //! a final assistant message on the second.  The tests assert:
    //!
    //! 1. The first-turn response carries the tool-call payload
    //!    (`tool_calls` / `function_call` / `tool_use`) back to the client.
    //! 2. The second-turn request body reaching the mock upstream still
    //!    references the call id emitted on the first turn, which is the
    //!    openai-compatible contract for preserving conversation context
    //!    across rounds.
    //! 3. The second-turn response carries the final assistant text.

    use super::*;
    use crate::{
        api::proxy::{chat_completions, messages},
        domain::config,
        http,
        infra::{health, observability, secrets},
    };
    use axum::{
        body::{to_bytes, Body},
        extract::Request,
        http::{header, Response, StatusCode},
        Router,
    };
    use serde_json::{json, Value};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tower::ServiceExt;

    /// Body of an inbound request captured at the mock upstream, kept small
    /// to keep multi-turn assertions readable.
    #[derive(Clone, Debug)]
    struct RecordedTurn {
        body: Value,
    }

    fn json_response(status: StatusCode, payload: Value) -> Response<Body> {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&payload).expect("encode json"),
            ))
            .expect("build mock upstream response")
    }

    /// Spin up a mock upstream that, on the first request, returns the
    /// `first_turn` response, and on every subsequent request returns
    /// `subsequent_turn`.  Both bodies are recorded so tests can assert the
    /// second-turn body carries the call id from the first turn.
    async fn spawn_round_trip_upstream(
        first_turn: Response<Body>,
        subsequent_turn: Response<Body>,
    ) -> (String, Arc<Mutex<Vec<RecordedTurn>>>) {
        let recorded: Arc<Mutex<Vec<RecordedTurn>>> = Arc::new(Mutex::new(Vec::new()));
        // Bodies are not Clone; preserve status / headers per turn and
        // reconstruct the response inside the closure on each request.
        let first_status = first_turn.status();
        let subsequent_status = subsequent_turn.status();
        let first_bytes = to_bytes(first_turn.into_body(), 1024 * 1024)
            .await
            .expect("read first-turn body");
        let subsequent_bytes = to_bytes(subsequent_turn.into_body(), 1024 * 1024)
            .await
            .expect("read subsequent-turn body");
        let first_bytes = Arc::new(first_bytes);
        let subsequent_bytes = Arc::new(subsequent_bytes);
        let app = Router::new().fallback({
            let recorded = recorded.clone();
            let first_bytes = first_bytes.clone();
            let subsequent_bytes = subsequent_bytes.clone();
            move |request: Request| {
                let recorded = recorded.clone();
                let first_bytes = first_bytes.clone();
                let subsequent_bytes = subsequent_bytes.clone();
                async move {
                    let body_bytes = to_bytes(request.into_body(), 4 * 1024 * 1024)
                        .await
                        .expect("read mock upstream body");
                    let body: Value =
                        serde_json::from_slice(&body_bytes).expect("decode mock upstream body");
                    let mut guard = recorded.lock().expect("recorded lock");
                    let is_first = guard.is_empty();
                    guard.push(RecordedTurn { body });
                    let (status, payload) = if is_first {
                        (first_status, Bytes::clone(&first_bytes))
                    } else {
                        (subsequent_status, Bytes::clone(&subsequent_bytes))
                    };
                    drop(guard);
                    Response::builder()
                        .status(status)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(payload))
                        .expect("build mock upstream response")
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind round-trip upstream");
        let address = listener.local_addr().expect("round-trip upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve round-trip upstream")
        });
        (format!("http://{address}"), recorded)
    }

    fn round_trip_state(base_url: String) -> AppState {
        let config = Arc::new(config::GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![config::ProviderConfig {
                id: "round-trip".into(),
                name: "Round trip provider".into(),
                base_url,
                models: vec!["k3".into()],
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
            }],
            accounts: vec![config::AccountConfig {
                id: "round-trip-account".into(),
                provider_id: "round-trip".into(),
                display_name: "Round trip account".into(),
                credential_env: None,
                credential_ciphertext: None,
                credential: Some("round-trip-key".into()),
                enabled: true,
                weight: 100,
                protocol_capabilities: HashMap::new(),
                capabilities: None,
                model_overrides: HashMap::new(),
                model_map: HashMap::new(),
            }],
            routes: vec![config::RouteConfig {
                id: "round-trip-route".into(),
                model: "k3".into(),
                provider_id: "round-trip".into(),
                protocols: vec![
                    Protocol::OpenAiChatCompletions,
                    Protocol::OpenAiResponses,
                    Protocol::AnthropicMessages,
                ],
                primary_account_id: "round-trip-account".into(),
                fallback_accounts: vec![],
                strategy: "primary_then_weighted_fallback".into(),
                mode: "native".into(),
                adapter: None,
                allow_lossy_conversion: false,
            }],
        });
        AppState {
            live: Arc::new(std::sync::RwLock::new(LiveConfig::legacy(config))),
            http: http::test_client().expect("round-trip HTTP client"),
            db: None,
            control_plane: None,
            health: health::HealthRegistry::new(Duration::from_secs(1)),
            admin_auth: AdminAuth::test(),
            secrets: secrets::SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
        }
    }

    /// Async JSON body parser used inside `#[tokio::test]` bodies.
    async fn json_body_async(response: Response<Body>) -> Value {
        let body_bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("round-trip response body");
        serde_json::from_slice(&body_bytes).expect("round-trip response JSON")
    }

    #[tokio::test]
    async fn chat_completions_multi_turn_tool_use_preserves_context() {
        // Anchor the gateway data-plane key so concurrent tests that mutate
        // `GATEWAY_API_KEY` cannot flip this test into a 401.
        let _environment_lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", "round-trip-key");
        let first_turn = json_response(
            StatusCode::OK,
            json!({
                "id": "chatcmpl-tool-1",
                "object": "chat.completion",
                "model": "k3",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_TOKYO_1",
                            "type": "function",
                            "function": {
                                "name": "lookup_weather",
                                "arguments": "{\"city\":\"Tokyo\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17}
            }),
        );
        let subsequent_turn = json_response(
            StatusCode::OK,
            json!({
                "id": "chatcmpl-final-2",
                "object": "chat.completion",
                "model": "k3",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Tokyo is sunny, 23°C."
                    },
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 38, "completion_tokens": 7, "total_tokens": 45}
            }),
        );
        let (base_url, recorded) = spawn_round_trip_upstream(first_turn, subsequent_turn).await;
        let state = round_trip_state(base_url);

        let first_request = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer round-trip-key")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "model": "k3",
                    "messages": [
                        {"role": "user", "content": "What's the weather in Tokyo?"}
                    ],
                    "tools": [{
                        "type": "function",
                        "function": {
                            "name": "lookup_weather",
                            "description": "Return the current weather for a city.",
                            "parameters": {
                                "type": "object",
                                "properties": {"city": {"type": "string"}},
                                "required": ["city"]
                            }
                        }
                    }]
                }))
                .expect("encode chat turn 1"),
            ))
            .expect("build chat turn 1 request");
        let first_response = axum::Router::new()
            .route("/{*path}", axum::routing::any(chat_completions))
            .with_state(state.clone())
            .oneshot(first_request)
            .await
            .expect("chat turn 1 response");
        assert_eq!(first_response.status(), StatusCode::OK);
        let first_body = json_body_async(first_response).await;
        let first_tool_calls = first_body["choices"][0]["message"]["tool_calls"]
            .as_array()
            .expect("tool_calls array");
        assert_eq!(first_tool_calls.len(), 1);
        let call_id = first_tool_calls[0]["id"].as_str().expect("tool call id");
        assert_eq!(call_id, "call_TOKYO_1");
        let tool_args: Value = serde_json::from_str(
            first_tool_calls[0]["function"]["arguments"]
                .as_str()
                .expect("tool arguments string"),
        )
        .expect("decode tool arguments");
        assert_eq!(tool_args["city"], "Tokyo");

        // The client "executes" the tool locally and feeds the result back
        // alongside the original messages.  This mirrors the
        // chatFunctionRoundTrip shape used by scripts/live-provider-smoke.mjs.
        let second_request = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer round-trip-key")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "model": "k3",
                    "messages": [
                        {"role": "user", "content": "What's the weather in Tokyo?"},
                        {
                            "role": "assistant",
                            "content": null,
                            "tool_calls": first_tool_calls.clone()
                        },
                        {
                            "role": "tool",
                            "tool_call_id": call_id,
                            "content": "Tokyo is sunny, 23°C."
                        }
                    ]
                }))
                .expect("encode chat turn 2"),
            ))
            .expect("build chat turn 2 request");
        let second_response = axum::Router::new()
            .route("/{*path}", axum::routing::any(chat_completions))
            .with_state(state.clone())
            .oneshot(second_request)
            .await
            .expect("chat turn 2 response");
        assert_eq!(second_response.status(), StatusCode::OK);
        let second_body = json_body_async(second_response).await;
        assert_eq!(
            second_body["choices"][0]["message"]["content"],
            "Tokyo is sunny, 23°C."
        );
        assert_eq!(second_body["choices"][0]["finish_reason"], "stop");

        // Verify the second-turn request that reached the upstream carries
        // the assistant tool_calls block plus the tool message so the
        // upstream could see the full conversation context.
        let captured = recorded.lock().expect("recorded lock").clone();
        assert_eq!(captured.len(), 2, "upstream must see both logical turns");
        let messages = captured[1].body["messages"]
            .as_array()
            .expect("second-turn messages array");
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| m["role"].as_str().expect("message role"))
            .collect();
        assert_eq!(roles, vec!["user", "assistant", "tool"]);
        assert_eq!(
            messages[2]["tool_call_id"].as_str(),
            Some("call_TOKYO_1"),
            "second turn must reference the first-turn tool call id"
        );
        assert_eq!(messages[2]["content"], "Tokyo is sunny, 23°C.");
        assert!(
            !messages[1]["tool_calls"].is_null(),
            "second turn must include the assistant tool_calls block"
        );
    }

    #[tokio::test]
    async fn responses_multi_turn_tool_use_preserves_context() {
        let _environment_lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", "round-trip-key");
        let first_turn = json_response(
            StatusCode::OK,
            json!({
                "id": "resp_tool_1",
                "object": "response",
                "status": "completed",
                "model": "k3",
                "output": [{
                    "type": "function_call",
                    "id": "fc_TOKYO_1",
                    "call_id": "fc_TOKYO_1",
                    "name": "lookup_weather",
                    "arguments": "{\"city\":\"Tokyo\"}"
                }],
                "usage": {
                    "input_tokens": 12,
                    "output_tokens": 5,
                    "total_tokens": 17
                }
            }),
        );
        let subsequent_turn = json_response(
            StatusCode::OK,
            json!({
                "id": "resp_final_2",
                "object": "response",
                "status": "completed",
                "model": "k3",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "Tokyo is sunny, 23°C."}]
                }],
                "usage": {
                    "input_tokens": 30,
                    "output_tokens": 7,
                    "total_tokens": 37
                }
            }),
        );
        let (base_url, recorded) = spawn_round_trip_upstream(first_turn, subsequent_turn).await;
        let state = round_trip_state(base_url);

        let first_request = Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer round-trip-key")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "model": "k3",
                    "input": "What's the weather in Tokyo?",
                    "tools": [{
                        "type": "function",
                        "name": "lookup_weather",
                        "description": "Return the current weather for a city.",
                        "parameters": {
                            "type": "object",
                            "properties": {"city": {"type": "string"}},
                            "required": ["city"]
                        }
                    }]
                }))
                .expect("encode responses turn 1"),
            ))
            .expect("build responses turn 1 request");
        let first_response = axum::Router::new()
            .route("/{*path}", axum::routing::any(responses))
            .with_state(state.clone())
            .oneshot(first_request)
            .await
            .expect("responses turn 1 response");
        assert_eq!(first_response.status(), StatusCode::OK);
        let first_body = json_body_async(first_response).await;
        let first_output = first_body["output"].as_array().expect("output array");
        assert_eq!(first_output[0]["type"], "function_call");
        assert_eq!(first_output[0]["call_id"], "fc_TOKYO_1");

        let second_request = Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer round-trip-key")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "model": "k3",
                    "input": [
                        {"role": "user", "content": "What's the weather in Tokyo?"},
                        {
                            "type": "function_call",
                            "call_id": "fc_TOKYO_1",
                            "name": "lookup_weather",
                            "arguments": "{\"city\":\"Tokyo\"}"
                        },
                        {
                            "type": "function_call_output",
                            "call_id": "fc_TOKYO_1",
                            "output": "Tokyo is sunny, 23°C."
                        }
                    ]
                }))
                .expect("encode responses turn 2"),
            ))
            .expect("build responses turn 2 request");
        let second_response = axum::Router::new()
            .route("/{*path}", axum::routing::any(responses))
            .with_state(state.clone())
            .oneshot(second_request)
            .await
            .expect("responses turn 2 response");
        assert_eq!(second_response.status(), StatusCode::OK);
        let second_body = json_body_async(second_response).await;
        assert_eq!(
            second_body["output"][0]["content"][0]["text"],
            "Tokyo is sunny, 23°C."
        );

        let captured = recorded.lock().expect("recorded lock").clone();
        assert_eq!(captured.len(), 2);
        let input = captured[1].body["input"]
            .as_array()
            .expect("second-turn input array");
        let types: Vec<&str> = input
            .iter()
            .map(|item| {
                item["type"]
                    .as_str()
                    .unwrap_or(item["role"].as_str().unwrap_or("?"))
            })
            .collect();
        assert_eq!(types, vec!["user", "function_call", "function_call_output"]);
        assert_eq!(input[2]["call_id"], "fc_TOKYO_1");
        assert_eq!(input[2]["output"], "Tokyo is sunny, 23°C.");
    }

    #[tokio::test]
    async fn anthropic_messages_multi_turn_tool_use_preserves_context() {
        let _environment_lock = ENV_LOCK.lock().await;
        let _key = EnvRestore::set("GATEWAY_API_KEY", "round-trip-key");
        let first_turn = json_response(
            StatusCode::OK,
            json!({
                "id": "msg_tool_1",
                "type": "message",
                "role": "assistant",
                "model": "k3",
                "stop_reason": "tool_use",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_TOKYO_1",
                    "name": "lookup_weather",
                    "input": {"city": "Tokyo"}
                }],
                "usage": {"input_tokens": 12, "output_tokens": 5}
            }),
        );
        let subsequent_turn = json_response(
            StatusCode::OK,
            json!({
                "id": "msg_final_2",
                "type": "message",
                "role": "assistant",
                "model": "k3",
                "stop_reason": "end_turn",
                "content": [{"type": "text", "text": "Tokyo is sunny, 23°C."}],
                "usage": {"input_tokens": 32, "output_tokens": 6}
            }),
        );
        let (base_url, recorded) = spawn_round_trip_upstream(first_turn, subsequent_turn).await;
        let state = round_trip_state(base_url);

        let first_request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-api-key", "round-trip-key")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "model": "k3",
                    "max_tokens": 256,
                    "messages": [
                        {"role": "user", "content": "What's the weather in Tokyo?"}
                    ],
                    "tools": [{
                        "name": "lookup_weather",
                        "description": "Return the current weather for a city.",
                        "input_schema": {
                            "type": "object",
                            "properties": {"city": {"type": "string"}},
                            "required": ["city"]
                        }
                    }]
                }))
                .expect("encode anthropic turn 1"),
            ))
            .expect("build anthropic turn 1 request");
        let first_response = axum::Router::new()
            .route("/{*path}", axum::routing::any(messages))
            .with_state(state.clone())
            .oneshot(first_request)
            .await
            .expect("anthropic turn 1 response");
        assert_eq!(first_response.status(), StatusCode::OK);
        let first_body = json_body_async(first_response).await;
        let first_content = first_body["content"].as_array().expect("content array");
        assert_eq!(first_content[0]["type"], "tool_use");
        assert_eq!(first_content[0]["id"], "toolu_TOKYO_1");
        let first_tool_input = first_content[0]["input"].clone();
        let first_tool_name = first_content[0]["name"].as_str().expect("tool name");

        let second_request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-api-key", "round-trip-key")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "model": "k3",
                    "max_tokens": 256,
                    "messages": [
                        {"role": "user", "content": "What's the weather in Tokyo?"},
                        {
                            "role": "assistant",
                            "content": [{
                                "type": "tool_use",
                                "id": "toolu_TOKYO_1",
                                "name": first_tool_name,
                                "input": first_tool_input
                            }]
                        },
                        {
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": "toolu_TOKYO_1",
                                "content": "Tokyo is sunny, 23°C."
                            }]
                        }
                    ]
                }))
                .expect("encode anthropic turn 2"),
            ))
            .expect("build anthropic turn 2 request");
        let second_response = axum::Router::new()
            .route("/{*path}", axum::routing::any(messages))
            .with_state(state.clone())
            .oneshot(second_request)
            .await
            .expect("anthropic turn 2 response");
        assert_eq!(second_response.status(), StatusCode::OK);
        let second_body = json_body_async(second_response).await;
        assert_eq!(second_body["content"][0]["text"], "Tokyo is sunny, 23°C.");
        assert_eq!(second_body["stop_reason"], "end_turn");

        let captured = recorded.lock().expect("recorded lock").clone();
        assert_eq!(captured.len(), 2);
        let messages = captured[1].body["messages"]
            .as_array()
            .expect("second-turn messages array");
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| m["role"].as_str().expect("message role"))
            .collect();
        assert_eq!(roles, vec!["user", "assistant", "user"]);
        let tool_result = &messages[2]["content"][0];
        assert_eq!(tool_result["type"], "tool_result");
        assert_eq!(tool_result["tool_use_id"], "toolu_TOKYO_1");
        assert_eq!(tool_result["content"], "Tokyo is sunny, 23°C.");
    }
}
