use axum::{
    body::Body,
    extract::{rejection::JsonRejection, Path, State},
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{control_plane, domain::protocol::Protocol, infra::health};

use super::helpers::{control_plane_error, json_payload, probe_error_response};
use crate::{http::response::error_response, state::AppState};

pub(crate) async fn admin_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let health_map = state.health.all_health().await;
    let live = state.snapshot();
    let accounts = if let Some(control_plane) = &state.control_plane {
        match control_plane.list_accounts().await {
            Ok(accounts) => accounts
                .into_iter()
                .map(|account| {
                    (
                        account.id,
                        account.source_id,
                        account.display_name,
                        account.enabled,
                    )
                })
                .collect::<Vec<_>>(),
            Err(error) => return control_plane_error(error),
        }
    } else if let Some(database) = &state.db {
        match database.health_accounts_metadata().await {
            Ok(accounts) => accounts
                .into_iter()
                .map(|account| {
                    (
                        account.id,
                        account.source_id,
                        account.display_name,
                        account.enabled,
                    )
                })
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "failed to list accounts for health API");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "failed to list accounts for health API",
                );
            }
        }
    } else {
        live.config
            .accounts
            .iter()
            .map(|account| {
                (
                    account.id.clone(),
                    account.provider_id.clone(),
                    account.display_name.clone(),
                    account.enabled,
                )
            })
            .collect::<Vec<_>>()
    };
    let database_configured = state.health.database().is_some();
    let mut data = Vec::new();
    for (account_id, source_id, display_name, enabled) in accounts {
        let health =
            health_map
                .get(&account_id)
                .cloned()
                .unwrap_or_else(|| health::AccountHealth {
                    available: enabled && !database_configured,
                    source_enabled: true,
                    consecutive_failures: 0,
                    cooldown_remaining_ms: 0,
                    status: if enabled { "unknown" } else { "disabled" }.into(),
                    source: "unknown".into(),
                    stale: true,
                    updated_at: None,
                    cooldown_until: None,
                    last_error: None,
                    last_success_at: None,
                    last_probe_at: None,
                    last_probe_status: None,
                    last_probe_error: None,
                });
        data.push(json!({
            "account_id": account_id,
            "provider_id": source_id.clone(),
            "source_id": source_id.clone(),
            "source": health.source.clone(),
            "display_name": display_name,
            "enabled": enabled,
            "health_status": health.status.clone(),
            "health_source": health.source.clone(),
            "health_updated_at": health.updated_at.clone(),
            "updated_at": health.updated_at.clone(),
            "stale": health.stale,
            "cooldown_until": health.cooldown_until,
            "consecutive_failures": health.consecutive_failures,
            "health": health,
        }));
    }
    (
        StatusCode::OK,
        Json(json!({
            "fact_source": if database_configured { "postgresql" } else { "memory" },
            "stale_after_secs": state.health.config().stale_after.as_secs(),
            "data": data
        })),
    )
        .into_response()
}

pub(crate) async fn admin_account_health(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let health = state.health.get_health(&account_id).await;
    let metadata = if let Some(control_plane) = &state.control_plane {
        match control_plane.get_account(&account_id).await {
            Ok(account) => json!({
                "account_id": account.id,
                "source_id": account.source_id,
                "display_name": account.display_name,
                "enabled": account.enabled,
            }),
            Err(control_plane::ControlPlaneError::NotFound(_)) => {
                return error_response(StatusCode::NOT_FOUND, "not_found", "account not found")
            }
            Err(error) => return control_plane_error(error),
        }
    } else if let Some(database) = &state.db {
        match database.health_account_metadata(&account_id).await {
            Ok(Some(account)) => json!({
                "account_id": account.id,
                "source_id": account.source_id,
                "display_name": account.display_name,
                "enabled": account.enabled,
            }),
            Ok(None) => {
                return error_response(StatusCode::NOT_FOUND, "not_found", "account not found")
            }
            Err(error) => {
                tracing::warn!(%error, "failed to read account for health API");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "failed to read account for health API",
                );
            }
        }
    } else {
        let live = state.snapshot();
        let Some(account) = live.config.account(&account_id) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "account not found");
        };
        json!({
            "account_id": account.id,
            "source_id": account.provider_id,
            "display_name": account.display_name,
            "enabled": account.enabled,
        })
    };
    let mut data = metadata;
    if let Some(object) = data.as_object_mut() {
        object.insert(
            "health".into(),
            serde_json::to_value(&health).unwrap_or(Value::Null),
        );
        object.insert("stale".into(), json!(health.stale));
        object.insert("source".into(), json!(health.source.clone()));
        object.insert("updated_at".into(), json!(health.updated_at.clone()));
    }
    (StatusCode::OK, Json(json!({"data": data}))).into_response()
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct HealthProbeRequest {
    account_id: Option<String>,
    protocol: Option<Protocol>,
    model: Option<String>,
    #[serde(default = "default_probe_actor")]
    requested_by: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct HealthProbesRequest {
    account_ids: Option<Vec<String>>,
    protocol: Option<Protocol>,
    model: Option<String>,
    #[serde(default = "default_probe_actor")]
    requested_by: String,
}

fn default_probe_actor() -> String {
    "admin_health_probe".into()
}

pub(crate) async fn admin_health_probe(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<HealthProbeRequest>, JsonRejection>,
) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let request = match json_payload(payload) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(account_id) = request
        .account_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "account_required",
            "account_id is required for a health probe",
        );
    };
    let protocol = match request.protocol {
        Some(protocol) => protocol,
        None => match state.health.database() {
            Some(database) => match database.health_probe_protocol(account_id).await {
                Ok(Some(protocol)) => protocol,
                Ok(None) => Protocol::OpenAiChatCompletions,
                Err(error) => {
                    tracing::warn!(%error, "failed to determine health probe protocol");
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "database_error",
                        "failed to determine health probe protocol",
                    );
                }
            },
            None => Protocol::OpenAiChatCompletions,
        },
    };
    match state
        .health
        .probe_account(
            &state.http,
            account_id,
            protocol,
            request.model.as_deref(),
            &request.requested_by,
        )
        .await
    {
        Ok(outcome) => (StatusCode::OK, Json(json!({"data": outcome}))).into_response(),
        Err(error) => probe_error_response(error),
    }
}

pub(crate) async fn admin_account_probe(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
    payload: Result<Json<HealthProbeRequest>, JsonRejection>,
) -> Response<Body> {
    let payload = match payload {
        Ok(Json(mut request)) => {
            request.account_id = Some(account_id);
            Ok(Json(request))
        }
        Err(error) => Err(error),
    };
    admin_health_probe(State(state), headers, payload).await
}

pub(crate) async fn admin_health_probes(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<HealthProbesRequest>, JsonRejection>,
) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let request = match json_payload(payload) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(database) = state.health.database() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "health probes require PostgreSQL",
        );
    };
    let requested_protocol = request.protocol;
    let account_ids = match request.account_ids {
        Some(ids) if !ids.is_empty() => ids,
        _ => match database.health_probe_targets().await {
            Ok(targets) => targets.into_iter().map(|target| target.0).collect(),
            Err(error) => {
                tracing::warn!(%error, "failed to list health probe targets");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "failed to list health probe targets",
                );
            }
        },
    };
    let mut outcomes = Vec::with_capacity(account_ids.len());
    let mut errors = Vec::new();
    for account_id in account_ids {
        let protocol = match requested_protocol {
            Some(protocol) => protocol,
            None => database
                .health_probe_protocol(&account_id)
                .await
                .ok()
                .flatten()
                .unwrap_or(Protocol::OpenAiChatCompletions),
        };
        match state
            .health
            .probe_account(
                &state.http,
                &account_id,
                protocol,
                request.model.as_deref(),
                &request.requested_by,
            )
            .await
        {
            Ok(outcome) => outcomes.push(outcome),
            Err(error) => errors.push(json!({
                "account_id": account_id,
                "code": error.code(),
                "message": error.message()
            })),
        }
    }
    (
        StatusCode::OK,
        Json(json!({"data": outcomes, "errors": errors})),
    )
        .into_response()
}
