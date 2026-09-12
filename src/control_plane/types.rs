use super::snapshot::RuntimeSnapshot;
use crate::domain::catalog::CatalogStatus;
use crate::domain::protocol::Protocol;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SourceWrite {
    pub(crate) id: String,
    pub(crate) display_name: String,
    #[serde(default = "default_custom_preset")]
    pub(crate) provider_preset_id: String,
    #[serde(default = "default_preset_version")]
    pub(crate) provider_preset_version: i32,
    pub(crate) base_url: String,
    #[serde(default)]
    pub(crate) endpoints: HashMap<Protocol, String>,
    #[serde(default)]
    pub(crate) auth_config: Value,
    #[serde(default)]
    pub(crate) protocol_capabilities: Value,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
}

/// POST contract shared with the Provider discovery workflow. Omitted values
/// are filled from the selected immutable ProviderPreset snapshot.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SourceCreateWrite {
    pub(crate) id: String,
    pub(crate) display_name: String,
    pub(crate) provider_preset_id: String,
    #[serde(default)]
    pub(crate) provider_preset_version: Option<i32>,
    #[serde(default)]
    pub(crate) base_url: Option<String>,
    #[serde(default)]
    pub(crate) endpoints: Option<HashMap<Protocol, String>>,
    #[serde(default)]
    pub(crate) endpoint_overrides: HashMap<Protocol, String>,
    #[serde(default)]
    pub(crate) auth_config: Option<Value>,
    #[serde(default)]
    pub(crate) protocol_capabilities: Option<Value>,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub(crate) struct SourceView {
    pub(crate) id: String,
    pub(crate) display_name: String,
    pub(crate) provider_preset_id: String,
    pub(crate) provider_preset_version: i32,
    pub(crate) provider_preset_snapshot: Value,
    pub(crate) base_url: String,
    pub(crate) endpoints: Value,
    pub(crate) auth_config: Value,
    pub(crate) protocol_capabilities: Value,
    pub(crate) enabled: bool,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AccountWrite {
    pub(crate) id: String,
    pub(crate) source_id: String,
    pub(crate) display_name: String,
    #[serde(default)]
    pub(crate) credential_env: Option<String>,
    #[serde(default)]
    pub(crate) credential_ciphertext: Option<String>,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default = "default_weight")]
    pub(crate) weight: i32,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub(crate) struct AccountView {
    pub(crate) id: String,
    pub(crate) source_id: String,
    pub(crate) display_name: String,
    pub(crate) credential_env: Option<String>,
    pub(crate) credential_configured: bool,
    pub(crate) enabled: bool,
    pub(crate) weight: i32,
    pub(crate) health_status: String,
    pub(crate) health_source: String,
    pub(crate) health_updated_at: Option<DateTime<Utc>>,
    pub(crate) consecutive_failures: i32,
    pub(crate) cooldown_until: Option<DateTime<Utc>>,
    pub(crate) last_error: Option<String>,
    pub(crate) last_success_at: Option<DateTime<Utc>>,
    pub(crate) last_probe_at: Option<DateTime<Utc>>,
    pub(crate) last_probe_status: Option<String>,
    pub(crate) last_probe_error: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct LogicalModelWrite {
    pub(crate) id: String,
    pub(crate) public_name: String,
    pub(crate) display_name: String,
    #[serde(default)]
    pub(crate) status: CatalogStatus,
    #[serde(default)]
    pub(crate) metadata: Value,
    #[serde(default)]
    pub(crate) field_sources: Value,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub(crate) struct LogicalModelView {
    pub(crate) id: String,
    pub(crate) public_name: String,
    pub(crate) display_name: String,
    pub(crate) status: CatalogStatus,
    pub(crate) metadata: Value,
    pub(crate) field_sources: Value,
    pub(crate) enabled: bool,
    pub(crate) confirmed_at: Option<DateTime<Utc>>,
    pub(crate) unavailable_at: Option<DateTime<Utc>>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelRoutingWrite {
    pub(crate) public_name: String,
    pub(crate) display_name: String,
    pub(crate) enabled: bool,
    pub(crate) lines: Vec<ModelRoutingLineWrite>,
    pub(crate) request_timeout_ms: Option<i64>,
    pub(crate) max_retries: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelRoutingLineWrite {
    pub(crate) source_id: String,
    pub(crate) account_id: String,
    pub(crate) upstream_model_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ModelRoutingLineView {
    pub(crate) source_id: String,
    pub(crate) account_id: String,
    pub(crate) upstream_model_id: String,
    pub(crate) protocols: Vec<Protocol>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ModelRoutingView {
    pub(crate) logical_model: LogicalModelView,
    pub(crate) lines: Vec<ModelRoutingLineView>,
    pub(crate) protocols: Vec<Protocol>,
    pub(crate) strategy: String,
    pub(crate) request_timeout_ms: Option<i64>,
    pub(crate) max_retries: Option<i32>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ModelBindingWrite {
    pub(crate) logical_model_id: String,
    pub(crate) source_id: String,
    pub(crate) account_id: String,
    pub(crate) upstream_model_id: String,
    pub(crate) protocol: Protocol,
    #[serde(default)]
    pub(crate) status: CatalogStatus,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) priority: i32,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub(crate) struct ModelBindingView {
    pub(crate) id: i64,
    pub(crate) logical_model_id: String,
    pub(crate) source_id: String,
    pub(crate) account_id: String,
    pub(crate) upstream_model_id: String,
    pub(crate) protocol: Protocol,
    pub(crate) status: CatalogStatus,
    pub(crate) enabled: bool,
    pub(crate) priority: i32,
    pub(crate) confirmed_at: Option<DateTime<Utc>>,
    pub(crate) unavailable_at: Option<DateTime<Utc>>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RouteWrite {
    pub(crate) id: String,
    pub(crate) logical_model_id: String,
    pub(crate) protocols: Vec<Protocol>,
    #[serde(default = "default_strategy")]
    pub(crate) strategy: String,
    #[serde(default)]
    pub(crate) allow_lossy_conversion: bool,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub(crate) struct RouteView {
    pub(crate) id: String,
    pub(crate) logical_model_id: String,
    pub(crate) public_name: String,
    pub(crate) protocols: Value,
    pub(crate) strategy: String,
    pub(crate) allow_lossy_conversion: bool,
    pub(crate) enabled: bool,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct EnabledWrite {
    pub(crate) enabled: bool,
}

pub(crate) struct Mutation<T> {
    pub(crate) record: T,
    pub(crate) snapshot: RuntimeSnapshot,
}

fn default_true() -> bool {
    true
}

fn default_weight() -> i32 {
    100
}

fn default_strategy() -> String {
    "primary_then_weighted_fallback".to_owned()
}

fn default_custom_preset() -> String {
    "custom".to_owned()
}

fn default_preset_version() -> i32 {
    1
}
