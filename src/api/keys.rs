use std::time::Duration;

use axum::{
    body::Body,
    extract::{rejection::JsonRejection, Path, State},
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::infra::{db, secrets};

use super::helpers::virtual_key_error_response;
use crate::{http::response::error_response, state::AppState};

fn encrypted_virtual_key_material(
    state: &AppState,
) -> Result<(db::VirtualKeyMaterial, String), Box<Response<Body>>> {
    if !state.secrets.has_master_key() {
        return Err(Box::new(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "master_key_unavailable",
            "credential master key is required to create recoverable Virtual Keys",
        )));
    }
    let material = db::Database::generate_virtual_key_material();
    let aad = secrets::SecretResolver::virtual_key_aad(&material.prefix);
    match state.secrets.encrypt(&material.raw, aad.as_bytes()) {
        Ok(ciphertext) => Ok((material, ciphertext)),
        Err(error) => Err(Box::new(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            error.code(),
            error.public_message(),
        ))),
    }
}

#[derive(Deserialize)]
pub(crate) struct CreateKeyRequest {
    name: String,
    #[serde(default)]
    allowed_models: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct EncryptCredentialRequest {
    source_id: String,
    account_id: String,
    plaintext: String,
}

pub(crate) async fn encrypt_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<EncryptCredentialRequest>,
) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or invalid admin key",
        );
    }
    if !state.secrets.has_master_key() {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "master_key_unavailable",
            "credential master key is not configured",
        );
    }
    match state.secrets.encrypt_for_account(
        &request.source_id,
        &request.account_id,
        &request.plaintext,
    ) {
        Ok(ciphertext) => Json(json!({
            "data": {
                "ciphertext": ciphertext,
                "key_version": state.secrets.active_key_version(),
            }
        }))
        .into_response(),
        Err(err) => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            err.code(),
            err.public_message(),
        ),
    }
}

pub(crate) async fn rotate_account_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or invalid admin key",
        );
    }
    let Some(control_plane) = &state.control_plane else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "database is not configured",
        );
    };
    match control_plane
        .rotate_account_credential(&account_id, &state.secrets)
        .await
    {
        Ok(snapshot) => {
            state.reload_snapshot(snapshot).await;
            Json(json!({
                "data": {
                    "account_id": account_id,
                    "key_version": state.secrets.active_key_version(),
                }
            }))
            .into_response()
        }
        Err(crate::control_plane::ControlPlaneError::Database(error)) => {
            tracing::warn!(%error, "account credential rotation failed");
            state.events.database_failed("credential.rotation").await;
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "failed to rotate account credential",
            )
        }
        Err(error) => super::helpers::control_plane_error(error),
    }
}

pub(crate) async fn create_key(
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
    let (material, ciphertext) = match encrypted_virtual_key_material(&state) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match database
        .create_virtual_key_with_material(
            &request.name,
            &request.allowed_models,
            &[db::VIRTUAL_KEY_INVOKE_SCOPE.to_owned()],
            None,
            None,
            "created",
            &material,
            Some(&ciphertext),
        )
        .await
    {
        Ok((id, key)) => (StatusCode::CREATED, Json(json!({"id":id,"key":key,"name":request.name,"allowed_models":request.allowed_models}))).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "key_create_failed", &error.to_string()),
    }
}

pub(crate) async fn reveal_key(
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
    let row = match database.get_virtual_key_ciphertext(id).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "key_not_found",
                "virtual key not found",
            );
        }
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "key_get_failed",
                "failed to fetch virtual key",
            );
        }
    };
    let Some(ciphertext) = row.1 else {
        return error_response(
            StatusCode::CONFLICT,
            "key_not_recoverable",
            "this Virtual Key predates recoverable storage; rotate it to obtain a viewable key",
        );
    };
    match state.secrets.resolve_virtual_key(&row.0, &ciphertext) {
        Ok(key) => Json(json!({"data":{"id":id,"key":key.as_str()}})).into_response(),
        Err(error) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            error.code(),
            error.public_message(),
        ),
    }
}

pub(crate) async fn list_keys(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
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

pub(crate) async fn get_key(
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
    match database.get_virtual_key(id).await {
        Ok(Some(key)) => (StatusCode::OK, Json(json!({"data": key}))).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "key_not_found",
            "virtual key not found",
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "key_get_failed",
            &error.to_string(),
        ),
    }
}

#[derive(Deserialize, Default)]
pub(crate) struct RotateKeyRequest {
    #[serde(default)]
    overlap_secs: u64,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    name: Option<String>,
    allowed_models: Option<Vec<String>>,
    scopes: Option<Vec<String>>,
    key_group: Option<Option<String>>,
}

pub(crate) async fn rotate_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    payload: Result<Json<RotateKeyRequest>, JsonRejection>,
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
    let request = match payload {
        Ok(Json(request)) => request,
        Err(error) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid_json", &error.to_string());
        }
    };
    if let Some(scopes) = &request.scopes {
        if let Err(message) = db::validate_virtual_key_scopes(scopes) {
            return error_response(StatusCode::BAD_REQUEST, "key_validation_failed", &message);
        }
    }
    let options = db::VirtualKeyRotationOptions {
        overlap: Duration::from_secs(request.overlap_secs),
        expires_at: request.expires_at,
        name: request.name,
        allowed_models: request.allowed_models,
        scopes: request.scopes,
        key_group: request.key_group,
    };
    let (material, ciphertext) = match encrypted_virtual_key_material(&state) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    match database
        .rotate_virtual_key_with_material(id, &options, &material, Some(&ciphertext))
        .await
    {
        Ok(rotation) => (StatusCode::OK, Json(json!(rotation))).into_response(),
        Err(error) => virtual_key_error_response(error),
    }
}

pub(crate) async fn revoke_key(
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
