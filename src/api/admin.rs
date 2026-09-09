use axum::{
    body::Body,
    extract::{rejection::JsonRejection, Path, State},
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use std::{collections::BTreeMap, str::FromStr};

use crate::{
    control_plane,
    domain::{
        capabilities,
        catalog::{
            CapabilitySupport, CatalogStatus, MetadataSource, SourceModelCapabilityInput,
            SourceProtocolMode,
        },
        protocol::Protocol,
    },
};

use super::helpers::{
    admin_control_plane, admin_result, control_plane_error, delete_result, json_payload,
    mutation_result, record_snapshot_build_failure,
};
use crate::{
    http::response::error_response,
    infra::events::SystemEvent,
    state::{AppState, LiveConfig},
};

pub(crate) async fn list_sources(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(&state, control_plane.list_sources().await).await
}

pub(crate) async fn get_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(&state, control_plane.get_source(&id).await).await
}

pub(crate) async fn create_source(
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
    .await
}

pub(crate) async fn update_source(
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
    .await
}

pub(crate) async fn set_source_enabled(
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
    .await
}

pub(crate) async fn delete_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    delete_result(&state, control_plane.delete_source(&id).await).await
}

pub(crate) async fn list_accounts(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(&state, control_plane.list_accounts().await).await
}

pub(crate) async fn get_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(&state, control_plane.get_account(&id).await).await
}

pub(crate) async fn create_account(
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
    .await
}

pub(crate) async fn update_account(
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
    .await
}

pub(crate) async fn set_account_enabled(
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
    .await
}

pub(crate) async fn delete_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    delete_result(&state, control_plane.delete_account(&id).await).await
}

pub(crate) async fn list_logical_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(&state, control_plane.list_logical_models().await).await
}

pub(crate) async fn get_logical_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(&state, control_plane.get_logical_model(&id).await).await
}

pub(crate) async fn create_logical_model(
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
    .await
}

pub(crate) async fn update_logical_model(
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
    .await
}

pub(crate) async fn set_logical_model_enabled(
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
    .await
}

pub(crate) async fn delete_logical_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    delete_result(&state, control_plane.delete_logical_model(&id).await).await
}

pub(crate) async fn list_model_bindings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(&state, control_plane.list_model_bindings().await).await
}

pub(crate) async fn get_model_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(&state, control_plane.get_model_binding(id).await).await
}

pub(crate) async fn create_model_binding(
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
    .await
}

#[derive(Debug, Deserialize)]
pub(crate) struct SourceModelCapabilityWrite {
    pub(crate) status: CatalogStatus,
    pub(crate) mode: SourceProtocolMode,
    #[serde(default)]
    pub(crate) source_protocol: Option<Protocol>,
    #[serde(default)]
    pub(crate) adapter: Option<String>,
    #[serde(default)]
    pub(crate) feature_capabilities: BTreeMap<String, CapabilitySupport>,
}

pub(crate) async fn list_source_model_capabilities(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((source_id, upstream_model_id)): Path<(String, String)>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(
        &state,
        control_plane
            .list_source_model_capabilities(&source_id, &upstream_model_id)
            .await,
    )
    .await
}

pub(crate) async fn upsert_source_model_capability(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((source_id, upstream_model_id, protocol)): Path<(String, String, String)>,
    payload: Result<Json<SourceModelCapabilityWrite>, JsonRejection>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    let protocol = match Protocol::from_str(&protocol) {
        Ok(protocol) => protocol,
        Err(_) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                &format!("unknown protocol '{protocol}'"),
            )
        }
    };
    let write = match json_payload(payload) {
        Ok(write) => write,
        Err(response) => return response,
    };
    let input = SourceModelCapabilityInput {
        source_id,
        upstream_model_id,
        protocol,
        status: write.status,
        mode: write.mode,
        source_protocol: write.source_protocol,
        adapter: write.adapter,
        feature_capabilities: write.feature_capabilities,
        field_source: MetadataSource::User,
        observed_at: Utc::now(),
    };
    mutation_result(
        &state,
        StatusCode::OK,
        control_plane.upsert_source_model_capability(&input).await,
    )
    .await
}

pub(crate) async fn update_model_binding(
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
    .await
}

pub(crate) async fn set_model_binding_enabled(
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
    .await
}

pub(crate) async fn delete_model_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    delete_result(&state, control_plane.delete_model_binding(id).await).await
}

pub(crate) async fn list_routes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(&state, control_plane.list_routes().await).await
}

pub(crate) async fn get_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    admin_result(&state, control_plane.get_route(&id).await).await
}

pub(crate) async fn create_route(
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
    .await
}

pub(crate) async fn update_route(
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
    .await
}

pub(crate) async fn set_route_enabled(
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
    .await
}

pub(crate) async fn delete_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let control_plane = match admin_control_plane(&state, &headers) {
        Ok(control_plane) => control_plane,
        Err(response) => return response,
    };
    delete_result(&state, control_plane.delete_route(&id).await).await
}

pub(crate) async fn admin_capabilities(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response<Body> {
    let live = state.snapshot();
    admin_capabilities_response(state.admin_auth.authorized(&headers), &live)
}

pub(crate) fn admin_capabilities_response(authorized: bool, live: &LiveConfig) -> Response<Body> {
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

pub(crate) async fn reload_config(
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
    reload_config_inner(&state).await
}

pub(crate) async fn reload_config_inner(state: &AppState) -> Response<Body> {
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
            let switched = state.reload_snapshot(snapshot).await;
            let mut event = SystemEvent::new(
                "configuration",
                "configuration.reload_succeeded",
                "info",
                "runtime_snapshot",
                "Runtime configuration reloaded",
            )
            .subject_id(revision.to_string())
            .details(json!({
                "snapshot_revision": revision,
                "snapshot_generated_at": generated_at,
                "switched": switched,
            }));
            if let Some(context) = crate::infra::audit::current_context() {
                event = event.correlation_id(context.request_id);
            }
            state.events.record(event).await;
            (
                StatusCode::OK,
                Json(json!({"status":"reloaded", "snapshot_revision":revision, "snapshot_generated_at":generated_at})),
            )
                .into_response()
        }
        Err(error) => {
            record_snapshot_build_failure(state, &error).await;
            let database_failure =
                matches!(error, crate::control_plane::ControlPlaneError::Database(_));
            let mut event = SystemEvent::new(
                "configuration",
                "configuration.reload_failed",
                "error",
                "runtime_snapshot",
                "Runtime configuration reload failed",
            )
            .subject_id("candidate")
            .details(json!({"error_code": error.code()}));
            if let Some(context) = crate::infra::audit::current_context() {
                event = event.correlation_id(context.request_id);
            }
            if database_failure {
                state.events.record_during_database_incident(event).await;
            } else {
                state.events.record(event).await;
            }
            control_plane_error(error)
        }
    }
}

pub(crate) async fn resolve_route(
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
