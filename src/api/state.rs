use std::sync::Arc;

use crate::{
    config::{self, GatewayConfig},
    control_plane, db, health, observability, ops,
    routing::RouteResolver,
    secrets, transport,
};
use axum::{
    body::Body,
    extract::rejection::JsonRejection,
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use metrics_exporter_prometheus::PrometheusHandle;
use serde_json::json;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub(crate) struct LiveConfig {
    pub(crate) config: Arc<GatewayConfig>,
    pub(crate) resolver: RouteResolver,
    pub(crate) models: Arc<Vec<control_plane::PublishedModel>>,
    pub(crate) revision: i64,
    pub(crate) generated_at: chrono::DateTime<chrono::Utc>,
}

impl LiveConfig {
    #[cfg(test)]
    pub(crate) fn legacy(config: Arc<GatewayConfig>) -> Self {
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

    pub(crate) fn from_snapshot(snapshot: control_plane::RuntimeSnapshot) -> Self {
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
pub(crate) struct AppState {
    pub(crate) live: Arc<std::sync::RwLock<LiveConfig>>,
    pub(crate) http: transport::SourceHttpClient,
    pub(crate) db: Option<db::Database>,
    pub(crate) control_plane: Option<control_plane::ControlPlane>,
    pub(crate) health: health::HealthRegistry,
    pub(crate) admin_auth: AdminAuth,
    pub(crate) secrets: secrets::SecretResolver,
    pub(crate) prometheus_handle: PrometheusHandle,
}

#[derive(Clone)]
pub(crate) struct AdminAuth {
    key_digest: Option<[u8; 32]>,
}

impl AdminAuth {
    pub(crate) fn from_env() -> Self {
        Self::from_key(
            std::env::var("GATEWAY_ADMIN_KEY")
                .ok()
                .filter(|key| !key.is_empty())
                .as_deref(),
        )
    }

    pub(crate) fn from_key(key: Option<&str>) -> Self {
        Self {
            key_digest: key.map(key_digest),
        }
    }

    pub(crate) fn is_configured(&self) -> bool {
        self.key_digest.is_some()
    }

    pub(crate) fn authorized(&self, headers: &HeaderMap) -> bool {
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

pub(crate) fn key_digest(key: &str) -> [u8; 32] {
    Sha256::digest(key.as_bytes()).into()
}

pub(crate) fn key_matches_digest(expected: &[u8; 32], supplied: &str) -> bool {
    bool::from(expected.ct_eq(&key_digest(supplied)))
}

#[cfg(test)]
pub(crate) const TEST_ADMIN_KEY: &str = "test-admin-key";

impl AppState {
    pub(crate) fn snapshot(&self) -> LiveConfig {
        self.live.read().unwrap().clone()
    }

    pub(crate) fn reload_snapshot(&self, snapshot: control_plane::RuntimeSnapshot) {
        let candidate = LiveConfig::from_snapshot(snapshot);
        let mut current = self.live.write().unwrap();
        if candidate.revision >= current.revision {
            let revision = candidate.revision;
            *current = candidate;
            observability::set_snapshot_revision(revision);
        }
    }
}

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
        | control_plane::ControlPlaneError::Json(_) => StatusCode::UNPROCESSABLE_ENTITY,
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

pub(crate) async fn authorized_with_db(
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

pub(crate) fn supplied_key(headers: &HeaderMap) -> Option<&str> {
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

pub(crate) fn resolve_credential(
    secrets: &secrets::SecretResolver,
    account: &config::AccountConfig,
) -> Option<String> {
    match secrets.resolve_account(
        &account.provider_id,
        &account.id,
        account.credential_env.as_deref(),
        account.credential_ciphertext.as_deref(),
        account.credential.as_deref(),
    ) {
        Ok(lease) => Some(lease.as_str().to_owned()),
        Err(secrets::SecretResolverError::CredentialUnavailable) => None,
        Err(err) => {
            tracing::warn!(
                account_id = %account.id,
                error = %err,
                "credential resolution failed"
            );
            None
        }
    }
}

pub(crate) fn error_response(status: StatusCode, code: &str, message: &str) -> Response<Body> {
    (
        status,
        Json(json!({"error":{"code":code,"type":code,"message":message}})),
    )
        .into_response()
}

#[cfg(test)]
pub(crate) static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
pub(crate) struct EnvRestore {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl EnvRestore {
    pub(crate) fn set(name: &'static str, value: &str) -> Self {
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
