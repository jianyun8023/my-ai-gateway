use axum::{
    body::Body,
    extract::rejection::JsonRejection,
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::json;

use crate::{
    control_plane,
    http::response::error_response,
    infra::{db, health, ops},
    state::AppState,
};

#[allow(clippy::result_large_err)]
pub(crate) fn ops_repository(
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

pub(crate) fn ops_error_response(error: ops::OpsError) -> Response<Body> {
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

pub(crate) fn virtual_key_error_response(error: db::VirtualKeyError) -> Response<Body> {
    match error {
        db::VirtualKeyError::NotFound => error_response(
            StatusCode::NOT_FOUND,
            "key_not_found",
            "virtual key not found",
        ),
        db::VirtualKeyError::Conflict(message) => {
            error_response(StatusCode::CONFLICT, "key_conflict", &message)
        }
        db::VirtualKeyError::Validation(message) => {
            error_response(StatusCode::BAD_REQUEST, "key_validation_failed", &message)
        }
        db::VirtualKeyError::Database(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "key_operation_failed",
            &error.to_string(),
        ),
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn admin_control_plane<'a>(
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

pub(crate) fn control_plane_error(error: control_plane::ControlPlaneError) -> Response<Body> {
    let status = match &error {
        control_plane::ControlPlaneError::NotFound(_) => StatusCode::NOT_FOUND,
        control_plane::ControlPlaneError::Conflict(_) => StatusCode::CONFLICT,
        control_plane::ControlPlaneError::Validation(_)
        | control_plane::ControlPlaneError::Json(_)
        | control_plane::ControlPlaneError::Credential(_)
        | control_plane::ControlPlaneError::NoCiphertext => StatusCode::UNPROCESSABLE_ENTITY,
        control_plane::ControlPlaneError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error_response(status, error.code(), &error.message())
}

pub(crate) fn admin_result<T: serde::Serialize>(
    result: Result<T, control_plane::ControlPlaneError>,
) -> Response<Body> {
    match result {
        Ok(record) => (StatusCode::OK, Json(json!({"data": record}))).into_response(),
        Err(error) => control_plane_error(error),
    }
}

pub(crate) fn mutation_result<T: serde::Serialize>(
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

pub(crate) fn delete_result(
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
pub(crate) fn json_payload<T>(
    payload: Result<Json<T>, JsonRejection>,
) -> Result<T, Response<Body>> {
    payload.map(|Json(value)| value).map_err(|error| {
        error_response(StatusCode::BAD_REQUEST, "invalid_json", &error.body_text())
    })
}

pub(crate) fn probe_error_response(error: health::ProbeError) -> Response<Body> {
    let status = match error.code() {
        "not_found" => StatusCode::NOT_FOUND,
        "database_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
        "database_error" => StatusCode::INTERNAL_SERVER_ERROR,
        "source_url_blocked"
        | "invalid_source_url"
        | "invalid_provider_preset"
        | "invalid_header_template"
        | "credential_unavailable"
        | "protocol_unsupported" => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::BAD_GATEWAY,
    };
    error_response(status, error.code(), &error.message())
}
