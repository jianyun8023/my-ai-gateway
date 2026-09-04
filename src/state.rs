use std::sync::Arc;

use crate::domain::protocol::Protocol;
use crate::{
    control_plane,
    domain::{
        catalog::PublishedModel,
        config::{self, GatewayConfig},
        routing::RouteResolver,
    },
    http,
    infra::{db, health, observability, secrets},
};
use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, Response, StatusCode},
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
    pub(crate) models: Arc<Vec<PublishedModel>>,
    pub(crate) revision: i64,
    pub(crate) generated_at: chrono::DateTime<chrono::Utc>,
}

impl LiveConfig {
    #[cfg(any(test, feature = "test-support"))]
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
            .map(|id| PublishedModel {
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
    pub(crate) http: http::SourceHttpClient,
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

pub(crate) async fn authorized_with_db(
    state: &AppState,
    headers: &HeaderMap,
    model: Option<&str>,
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

/// Build a protocol-aware JSON error envelope for the data-plane proxy
/// (`/v1/chat/completions`, `/v1/responses`, `/v1/messages`).
///
/// The Anthropic Messages envelope follows the public Anthropic API contract
/// (`{"type":"error","error":{"type":<sdk_standard>,"code":<internal>,
/// "message":<...>},"request_id":<uuid>}`).  The OpenAI envelopes keep the
/// existing `error.code/type/message` shape for backward compatibility and
/// only attach the optional top-level `request_id` plus the `x-request-id`
/// response header so OpenAI clients ignore unknown fields safely.
///
/// `x-request-id` is always set on the response so SDKs that read it from
/// headers (Anthropic SDK) get a stable correlation id.
pub(crate) fn data_plane_error_response(
    protocol: Protocol,
    status: StatusCode,
    gateway_code: &str,
    message: &str,
    request_id: &str,
) -> Response<Body> {
    let body = match protocol {
        Protocol::AnthropicMessages => {
            let standard_type = anthropic_standard_error_type(gateway_code);
            json!({
                "type": "error",
                "error": {
                    "type": standard_type,
                    "code": gateway_code,
                    "message": message,
                },
                "request_id": request_id,
            })
        }
        Protocol::OpenAiChatCompletions | Protocol::OpenAiResponses => {
            // The existing `error.{code,type,message}` shape is preserved so
            // OpenAI SDKs and clients keep working.  `request_id` is added
            // at the top level; OpenAI clients ignore unknown fields.
            json!({
                "error": {
                    "code": gateway_code,
                    "type": gateway_code,
                    "message": message,
                },
                "request_id": request_id,
            })
        }
    };
    let mut response = (status, Json(body)).into_response();
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

/// Map a gateway internal error code to an Anthropic SDK standard error
/// `type`.  See https://docs.anthropic.com/en/api/errors for the full enum.
fn anthropic_standard_error_type(gateway_code: &str) -> &'static str {
    match gateway_code {
        "invalid_json"
        | "missing_required_parameter"
        | "unsupported_protocol"
        | "lossy_conversion_not_allowed" => "invalid_request_error",
        "unauthorized" => "authentication_error",
        "route_not_found" => "not_found_error",
        "account_cooling_down" => "overloaded_error",
        "upstream_request_failed" => "api_error",
        "provider_not_found"
        | "account_not_found"
        | "fallback_account_not_found"
        | "fallback_provider_mismatch"
        | "account_provider_mismatch"
        | "adapter_missing"
        | "adapter_unknown"
        | "adapter_direction_mismatch"
        | "adapter_capability_mismatch"
        | "adapter_source_unsupported"
        | "invalid_route_mode"
        | "endpoint_missing"
        | "route_unavailable"
        | "account_disabled" => "api_error",
        _ => "api_error",
    }
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

    pub(crate) fn unset(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        std::env::remove_var(name);
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
