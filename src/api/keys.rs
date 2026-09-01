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

use crate::{db, secrets};

use super::state::{error_response, virtual_key_error_response, AppState};

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
    let aad = secrets::SecretResolver::account_aad(&request.source_id, &request.account_id);
    match state.secrets.encrypt(&request.plaintext, aad.as_bytes()) {
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
    let Some(database) = &state.db else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "database is not configured",
        );
    };
    let row = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT source_id, credential_ciphertext FROM accounts WHERE id=$1",
    )
    .bind(&account_id)
    .fetch_optional(database.pool())
    .await;
    match row {
        Ok(Some((source_id, Some(ciphertext)))) => {
            match state
                .secrets
                .rotate_for_account(&source_id, &account_id, &ciphertext)
            {
                Ok(rotated) => {
                    let _ = sqlx::query(
                        "UPDATE accounts SET credential_ciphertext=$2, updated_at=NOW() WHERE id=$1",
                    )
                    .bind(&account_id)
                    .bind(&rotated)
                    .execute(database.pool())
                    .await;
                    Json(json!({
                        "data": {
                            "account_id": account_id,
                            "key_version": state.secrets.active_key_version(),
                        }
                    }))
                    .into_response()
                }
                Err(err) => error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    err.code(),
                    err.public_message(),
                ),
            }
        }
        Ok(Some((_, None))) => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "no_ciphertext",
            "account does not have an encrypted credential",
        ),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "not_found", "account not found"),
        Err(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            "failed to fetch account",
        ),
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
    match database.create_virtual_key(&request.name, &request.allowed_models).await {
        Ok((id, key)) => (StatusCode::CREATED, Json(json!({"id":id,"key":key,"name":request.name,"allowed_models":request.allowed_models}))).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "key_create_failed", &error.to_string()),
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
    match database.rotate_virtual_key(id, &options).await {
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
