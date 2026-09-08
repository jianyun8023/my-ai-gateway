use std::{net::SocketAddr, sync::Arc};

use serde_json::{json, Value};

use crate::{
    auth::AdminAuth,
    control_plane,
    domain::config::GatewayConfig,
    http::client as source_http_client,
    infra::{db, events::SystemEvent, health, observability, ops, secrets},
    source_url,
    state::{AppState, LiveConfig},
};

pub(crate) async fn run() -> Result<(), Box<dyn std::error::Error>> {
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
    let events = control_plane.event_repository();
    let snapshot_result = match bootstrap {
        Some(config) => match control_plane
            .initialize_from_config(&config, force_import)
            .await
        {
            Ok(Some(snapshot)) => Ok(snapshot),
            Ok(None) => control_plane.load_snapshot().await,
            Err(error) => Err(error),
        },
        None => control_plane.load_snapshot().await,
    };
    let snapshot = match snapshot_result {
        Ok(snapshot) => snapshot,
        Err(error) => {
            if matches!(error, control_plane::ControlPlaneError::Database(_)) {
                events.database_failed("runtime.snapshot.startup").await;
            }
            events
                .record(
                    SystemEvent::new(
                        "configuration",
                        "runtime.snapshot_startup_failed",
                        "error",
                        "runtime_snapshot",
                        "Initial runtime snapshot could not be loaded",
                    )
                    .subject_id("candidate")
                    .details(json!({"error_code": error.code()})),
                )
                .await;
            return Err(error.into());
        }
    };
    events.database_recovered("runtime.snapshot.startup").await;
    events
        .record(
            SystemEvent::new(
                "configuration",
                "runtime.snapshot_built",
                "info",
                "runtime_snapshot",
                "Initial runtime snapshot built",
            )
            .subject_id(snapshot.revision.to_string())
            .details(json!({
                "snapshot_revision": snapshot.revision,
                "snapshot_generated_at": snapshot.generated_at,
            })),
        )
        .await;
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
    if let Err(error) = health.restore().await {
        events.database_failed("health.restore").await;
        return Err(error.into());
    }
    events.database_recovered("health.restore").await;
    let secrets = match secrets::SecretResolver::from_env() {
        Ok(secrets) => secrets,
        Err(error) => {
            tracing::warn!(
                code = error.code(),
                "credential master key unavailable; encrypted credentials will fail at request time"
            );
            events
                .record(
                    SystemEvent::new(
                        "security",
                        "credential.master_key_unavailable",
                        "error",
                        "credential_store",
                        "Credential master key is unavailable",
                    )
                    .subject_id("gateway")
                    .details(json!({"error_code": error.code()})),
                )
                .await;
            secrets::SecretResolver::empty()
        }
    };
    let prometheus_handle = observability::prometheus_handle();
    observability::spawn_upkeep(prometheus_handle.clone());
    observability::set_snapshot_revision(live.revision);
    let state = AppState {
        live: Arc::new(std::sync::RwLock::new(live)),
        http: source_http_client(source_url_policy.clone())?,
        db: Some(database.clone()),
        control_plane: Some(control_plane),
        events: events.clone(),
        health,
        admin_auth,
        secrets,
        prometheus_handle,
    };
    spawn_health_probe_loop(state.clone());
    let app = crate::app::application(state.clone());
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            events
                .record(
                    SystemEvent::new(
                        "lifecycle",
                        "gateway.start_failed",
                        "error",
                        "gateway",
                        "Gateway listener failed to start",
                    )
                    .subject_id("process")
                    .details(json!({"error_code": "listener_bind_failed"})),
                )
                .await;
            return Err(error.into());
        }
    };
    events
        .record(
            SystemEvent::new(
                "configuration",
                "runtime.snapshot_switched",
                "info",
                "runtime_snapshot",
                "Initial runtime snapshot switched",
            )
            .subject_id(state.snapshot().revision.to_string())
            .details(json!({
                "snapshot_revision": state.snapshot().revision,
                "snapshot_generated_at": state.snapshot().generated_at,
                "previous_revision": null,
            })),
        )
        .await;
    events
        .record(
            SystemEvent::new(
                "lifecycle",
                "gateway.started",
                "info",
                "gateway",
                "Gateway started",
            )
            .subject_id("process")
            .details(json!({
                "listen_addr": addr.to_string(),
                "snapshot_revision": state.snapshot().revision,
            })),
        )
        .await;
    tracing::info!(%addr, "AI gateway listening");
    match axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        Ok(()) => {
            events
                .record(
                    SystemEvent::new(
                        "lifecycle",
                        "gateway.stopped",
                        "info",
                        "gateway",
                        "Gateway stopped gracefully",
                    )
                    .subject_id("process"),
                )
                .await;
            Ok(())
        }
        Err(error) => {
            events
                .record(
                    SystemEvent::new(
                        "lifecycle",
                        "gateway.serve_failed",
                        "error",
                        "gateway",
                        "Gateway server stopped unexpectedly",
                    )
                    .subject_id("process")
                    .details(json!({"error_code": "server_io_failed"})),
                )
                .await;
            Err(error.into())
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
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
        Ok(targets) => {
            state
                .events
                .database_recovered("health.probe_targets")
                .await;
            targets
        }
        Err(error) => {
            tracing::warn!(%error, "failed to enumerate periodic health probes");
            state.events.database_failed("health.probe_targets").await;
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
        // Keep probing at the configured cadence while the account is cooling
        // down. A successful half-open probe can restore a transiently limited
        // account before a long exponential cooldown expires.
        if !due {
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
            Ok(outcome) => {
                state.events.database_recovered("health.probe").await;
                tracing::info!(
                    account_id = %outcome.account_id,
                    %protocol,
                    status = %outcome.connection_test.status,
                    "periodic account health probe completed"
                )
            }
            Err(error) => {
                if error.code() == "database_error" {
                    state.events.database_failed("health.probe").await;
                }
                tracing::warn!(
                    account_id = %account_id,
                    %protocol,
                    code = error.code(),
                    "periodic account health probe failed"
                )
            }
        }
    }
}
