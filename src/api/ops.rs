use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        HeaderMap, Response, StatusCode,
    },
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};

use crate::ops;

use super::state::{error_response, ops_error_response, ops_repository, AppState};

pub(crate) async fn list_retention_policies(
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

pub(crate) async fn update_retention_policies(
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

pub(crate) async fn start_retention_cleanup(
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

pub(crate) async fn list_retention_cleanups(
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

pub(crate) async fn get_retention_cleanup(
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

pub(crate) async fn cancel_retention_cleanup(
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

pub(crate) async fn retry_retention_cleanup(
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

pub(crate) async fn export_control_plane(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response<Body> {
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

pub(crate) async fn import_control_plane(
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

pub(crate) async fn list_audit_logs(
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

pub(crate) async fn get_backup_run(
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

pub(crate) async fn list_backup_runs(
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

pub(crate) async fn ops_schema_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response<Body> {
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
