use crate::domain::{config::adapter_definition, protocol::Protocol};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use std::{collections::BTreeMap, error::Error, fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "catalog_status", rename_all = "snake_case")]
pub enum CatalogStatus {
    #[default]
    Pending,
    Confirmed,
    Unavailable,
}

impl CatalogStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Pending, Self::Confirmed | Self::Unavailable)
                    | (Self::Confirmed, Self::Unavailable)
                    | (Self::Unavailable, Self::Pending)
            )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "catalog_availability", rename_all = "snake_case")]
pub enum CatalogAvailability {
    Unknown,
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "source_protocol_mode", rename_all = "snake_case")]
pub enum SourceProtocolMode {
    Unknown,
    Native,
    Adapter,
    Unsupported,
}

impl SourceProtocolMode {
    #[allow(dead_code)]
    pub fn is_routable(self) -> bool {
        matches!(self, Self::Native | Self::Adapter)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataSource {
    Upstream,
    Preset,
    User,
    Unknown,
}

impl fmt::Display for MetadataSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Upstream => "upstream",
            Self::Preset => "preset",
            Self::User => "user",
            Self::Unknown => "unknown",
        })
    }
}

impl FromStr for MetadataSource {
    type Err = CatalogError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "upstream" => Ok(Self::Upstream),
            "preset" => Ok(Self::Preset),
            "user" => Ok(Self::User),
            "unknown" => Ok(Self::Unknown),
            _ => Err(CatalogError::InvalidMetadata(format!(
                "unknown metadata source '{value}'"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataField {
    LogicalModelName,
    DisplayName,
    ContextWindow,
    MaxInputTokens,
    MaxOutputTokens,
    InputModalities,
    OutputModalities,
    Tools,
    Thinking,
    WebSearch,
    StructuredOutput,
    Streaming,
    Usage,
}

impl MetadataField {
    pub const ALL: [Self; 13] = [
        Self::LogicalModelName,
        Self::DisplayName,
        Self::ContextWindow,
        Self::MaxInputTokens,
        Self::MaxOutputTokens,
        Self::InputModalities,
        Self::OutputModalities,
        Self::Tools,
        Self::Thinking,
        Self::WebSearch,
        Self::StructuredOutput,
        Self::Streaming,
        Self::Usage,
    ];

    fn is_feature(self) -> bool {
        matches!(
            self,
            Self::Tools
                | Self::Thinking
                | Self::WebSearch
                | Self::StructuredOutput
                | Self::Streaming
                | Self::Usage
        )
    }

    fn unknown_value(self) -> Value {
        if self.is_feature() {
            Value::String("unknown".to_owned())
        } else {
            Value::Null
        }
    }
}

impl fmt::Display for MetadataField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_value(self).expect("metadata field serializes");
        f.write_str(value.as_str().expect("metadata field is a string"))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Supported,
    Unsupported,
    Unknown,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MetadataValues(pub BTreeMap<MetadataField, Value>);

impl MetadataValues {
    #[allow(dead_code)]
    pub fn from_fields(
        fields: impl IntoIterator<Item = (MetadataField, Value)>,
    ) -> Result<Self, CatalogError> {
        let values = Self(fields.into_iter().collect());
        values.validate()?;
        Ok(values)
    }

    pub fn validate(&self) -> Result<(), CatalogError> {
        for (field, value) in &self.0 {
            let valid = match field {
                MetadataField::LogicalModelName | MetadataField::DisplayName => {
                    value.is_null() || value.as_str().is_some_and(|value| !value.trim().is_empty())
                }
                MetadataField::ContextWindow
                | MetadataField::MaxInputTokens
                | MetadataField::MaxOutputTokens => {
                    value.is_null() || value.as_i64().is_some_and(|value| value > 0)
                }
                MetadataField::InputModalities | MetadataField::OutputModalities => {
                    value.is_null()
                        || value.as_array().is_some_and(|items| {
                            items.iter().all(|item| {
                                item.as_str().is_some_and(|value| !value.trim().is_empty())
                            })
                        })
                }
                field if field.is_feature() => value
                    .as_str()
                    .is_some_and(|value| matches!(value, "supported" | "unsupported" | "unknown")),
                _ => false,
            };
            if !valid {
                return Err(CatalogError::InvalidMetadata(format!(
                    "invalid value for metadata field '{field}'"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CatalogMetadata {
    pub values: MetadataValues,
    pub field_sources: BTreeMap<MetadataField, MetadataSource>,
}

impl CatalogMetadata {
    pub fn resolve(
        upstream: &MetadataValues,
        preset: Option<&MetadataValues>,
    ) -> Result<Self, CatalogError> {
        upstream.validate()?;
        if let Some(preset) = preset {
            preset.validate()?;
        }
        let mut result = Self::unknown();
        result.apply_values(upstream, MetadataSource::Upstream);
        if let Some(preset) = preset {
            result.apply_values(preset, MetadataSource::Preset);
        }
        Ok(result)
    }

    pub fn unknown() -> Self {
        let values = MetadataField::ALL
            .into_iter()
            .map(|field| (field, field.unknown_value()))
            .collect();
        let field_sources = MetadataField::ALL
            .into_iter()
            .map(|field| (field, MetadataSource::Unknown))
            .collect();
        Self {
            values: MetadataValues(values),
            field_sources,
        }
    }

    pub fn apply_user_overrides(&mut self, overrides: &MetadataValues) -> Result<(), CatalogError> {
        overrides.validate()?;
        self.apply_values(overrides, MetadataSource::User);
        Ok(())
    }

    pub fn refreshed_preserving_user_fields(
        &self,
        confirmed: bool,
        upstream: &MetadataValues,
        preset: Option<&MetadataValues>,
    ) -> Result<Self, CatalogError> {
        if confirmed {
            return Ok(self.clone());
        }
        let mut refreshed = Self::resolve(upstream, preset)?;
        for field in MetadataField::ALL {
            if self.field_sources.get(&field) == Some(&MetadataSource::User) {
                if let Some(value) = self.values.0.get(&field) {
                    refreshed.values.0.insert(field, value.clone());
                    refreshed.field_sources.insert(field, MetadataSource::User);
                }
            }
        }
        Ok(refreshed)
    }

    pub fn to_json(&self) -> Result<(Value, Value), CatalogError> {
        Ok((
            serde_json::to_value(&self.values)?,
            serde_json::to_value(&self.field_sources)?,
        ))
    }

    pub fn from_json(values: Value, field_sources: Value) -> Result<Self, CatalogError> {
        let metadata = Self {
            values: serde_json::from_value(values)?,
            field_sources: serde_json::from_value(field_sources)?,
        };
        metadata.values.validate()?;
        Ok(metadata)
    }

    fn apply_values(&mut self, values: &MetadataValues, source: MetadataSource) {
        for (field, value) in &values.0 {
            let is_unknown =
                value.is_null() || (field.is_feature() && value.as_str() == Some("unknown"));
            if is_unknown
                && source != MetadataSource::User
                && self.field_sources.get(field) != Some(&MetadataSource::Unknown)
            {
                continue;
            }
            self.values.0.insert(*field, value.clone());
            self.field_sources.insert(
                *field,
                if is_unknown && source != MetadataSource::User {
                    MetadataSource::Unknown
                } else {
                    source
                },
            );
        }
    }
}

#[derive(Debug)]
pub enum CatalogError {
    Database(sqlx::Error),
    Json(serde_json::Error),
    NotFound(String),
    InvalidMetadata(String),
    InvalidState(String),
    ImmutableVersionConflict(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::NotFound(message)
            | Self::InvalidMetadata(message)
            | Self::InvalidState(message)
            | Self::ImmutableVersionConflict(message) => f.write_str(message),
        }
    }
}

impl Error for CatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for CatalogError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

impl From<serde_json::Error> for CatalogError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelPresetRef {
    pub id: String,
    pub version: i32,
}

#[derive(Clone, Debug)]
pub struct ProviderPresetInput {
    pub id: String,
    pub version: i32,
    pub display_name: String,
    pub definition: Value,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct ProviderPresetRecord {
    pub id: String,
    pub version: i32,
    pub display_name: String,
    pub definition: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct SourceInput {
    pub id: String,
    pub display_name: String,
    pub provider_preset_id: String,
    pub provider_preset_version: i32,
    pub base_url: String,
    pub endpoints: Value,
    pub auth_config: Value,
    pub protocol_capabilities: Value,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct SourceRecord {
    pub id: String,
    pub display_name: String,
    pub provider_preset_id: String,
    pub provider_preset_version: i32,
    pub provider_preset_snapshot: Value,
    pub base_url: String,
    pub endpoints: Value,
    pub auth_config: Value,
    pub protocol_capabilities: Value,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct ModelPresetInput {
    pub id: String,
    pub version: i32,
    pub canonical_model_id: String,
    pub aliases: Vec<String>,
    pub metadata: CatalogMetadata,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct ModelPresetRecord {
    pub id: String,
    pub version: i32,
    pub canonical_model_id: String,
    pub aliases: Value,
    pub metadata: Value,
    pub field_sources: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ModelPresetRecord {
    pub fn catalog_metadata(&self) -> Result<CatalogMetadata, CatalogError> {
        CatalogMetadata::from_json(self.metadata.clone(), self.field_sources.clone())
    }
}

#[derive(Clone, Debug)]
pub struct SourceModelRefresh {
    pub source_id: String,
    pub upstream_model_id: String,
    pub raw_snapshot: Value,
    pub upstream_metadata: MetadataValues,
    pub matched_preset: Option<ModelPresetRef>,
    pub preset_metadata: Option<MetadataValues>,
    pub discovered_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct SourceModelRecord {
    pub source_id: String,
    pub upstream_model_id: String,
    pub confirmation_status: CatalogStatus,
    pub availability_status: CatalogAvailability,
    pub raw_snapshot: Value,
    pub metadata: Value,
    pub field_sources: Value,
    pub matched_model_preset_id: Option<String>,
    pub matched_model_preset_version: Option<i32>,
    pub first_discovered_at: DateTime<Utc>,
    pub last_discovered_at: DateTime<Utc>,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub unavailable_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SourceModelRecord {
    pub fn catalog_metadata(&self) -> Result<CatalogMetadata, CatalogError> {
        CatalogMetadata::from_json(self.metadata.clone(), self.field_sources.clone())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiscoveryDiffEntry {
    pub upstream_model_id: String,
    pub changed_fields: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DiscoveryDiff {
    pub added: Vec<DiscoveryDiffEntry>,
    pub changed: Vec<DiscoveryDiffEntry>,
    pub missing: Vec<DiscoveryDiffEntry>,
}

#[derive(Clone, Debug)]
pub struct DiscoveryApplyInput {
    pub source_id: String,
    pub account_id: Option<String>,
    pub raw_snapshot: Value,
    pub models: Vec<SourceModelRefresh>,
    pub http_status: i32,
    pub latency_ms: i64,
    pub requested_by: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct DiscoveryRunRecord {
    pub id: i64,
    pub source_id: String,
    pub account_id: Option<String>,
    pub provider_preset_id: String,
    pub provider_preset_version: i32,
    pub status: String,
    pub raw_snapshot: Option<Value>,
    pub diff: Value,
    pub discovered_model_count: i32,
    pub http_status: Option<i32>,
    pub latency_ms: i64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub requested_by: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

impl DiscoveryRunRecord {
    pub fn discovery_diff(&self) -> Result<DiscoveryDiff, CatalogError> {
        serde_json::from_value(self.diff.clone()).map_err(Into::into)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DiscoveryApplyResult {
    pub run: DiscoveryRunRecord,
    pub diff: DiscoveryDiff,
    pub models: Vec<SourceModelRecord>,
}

#[derive(Clone, Debug)]
pub struct DiscoveryFailureInput {
    pub source_id: String,
    pub account_id: Option<String>,
    pub status: String,
    pub http_status: Option<i32>,
    pub latency_ms: i64,
    pub error_code: String,
    pub error_message: String,
    pub requested_by: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct ConnectionTestInput {
    pub source_id: String,
    pub account_id: Option<String>,
    pub protocol: Protocol,
    pub upstream_protocol: Protocol,
    pub mode: SourceProtocolMode,
    pub status: String,
    pub http_status: Option<i32>,
    pub latency_ms: i64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub requested_by: String,
    pub tested_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct ConnectionTestRecord {
    pub id: i64,
    pub source_id: String,
    pub account_id: Option<String>,
    pub protocol: Protocol,
    pub upstream_protocol: Protocol,
    pub mode: SourceProtocolMode,
    pub status: String,
    pub http_status: Option<i32>,
    pub latency_ms: i64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub requested_by: String,
    pub tested_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct AccountCredentialRef {
    pub id: String,
    pub source_id: String,
    pub credential_env: Option<String>,
    pub has_credential_ciphertext: bool,
}

#[derive(Clone, Debug)]
pub struct SourceModelConfirmation {
    pub upstream_model_id: String,
    pub user_overrides: MetadataValues,
}

#[derive(Clone, Debug)]
pub struct LogicalModelInput {
    pub id: String,
    pub public_name: String,
    pub display_name: String,
    pub status: CatalogStatus,
    pub model_preset: Option<ModelPresetRef>,
    pub metadata: CatalogMetadata,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct LogicalModelRecord {
    pub id: String,
    pub public_name: String,
    pub display_name: String,
    pub status: CatalogStatus,
    pub model_preset_id: Option<String>,
    pub model_preset_version: Option<i32>,
    pub metadata: Value,
    pub field_sources: Value,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub unavailable_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct SourceModelCapabilityInput {
    pub source_id: String,
    pub upstream_model_id: String,
    pub protocol: Protocol,
    pub status: CatalogStatus,
    pub mode: SourceProtocolMode,
    pub source_protocol: Option<Protocol>,
    pub adapter: Option<String>,
    pub feature_capabilities: BTreeMap<String, CapabilitySupport>,
    pub field_source: MetadataSource,
    pub observed_at: DateTime<Utc>,
}

impl SourceModelCapabilityInput {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.status == CatalogStatus::Confirmed && self.mode == SourceProtocolMode::Unknown {
            return Err(CatalogError::InvalidState(
                "unknown protocol capability cannot be confirmed".to_owned(),
            ));
        }
        match self.mode {
            SourceProtocolMode::Adapter => {
                let source_protocol = self.source_protocol.ok_or_else(|| {
                    CatalogError::InvalidState(
                        "adapter capability requires source_protocol".to_owned(),
                    )
                })?;
                let adapter = self
                    .adapter
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        CatalogError::InvalidState("adapter capability requires adapter".to_owned())
                    })?;
                let definition = adapter_definition(adapter).ok_or_else(|| {
                    CatalogError::InvalidState(format!("unknown adapter '{adapter}'"))
                })?;
                if definition.from_protocol != self.protocol
                    || definition.to_protocol != source_protocol
                {
                    return Err(CatalogError::InvalidState(format!(
                        "adapter '{adapter}' does not implement {} -> {source_protocol}",
                        self.protocol
                    )));
                }
            }
            SourceProtocolMode::Native
            | SourceProtocolMode::Unsupported
            | SourceProtocolMode::Unknown => {
                if self.source_protocol.is_some() || self.adapter.is_some() {
                    return Err(CatalogError::InvalidState(
                        "only adapter capability may set source_protocol and adapter".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct SourceModelCapabilityRecord {
    pub source_id: String,
    pub upstream_model_id: String,
    pub protocol: Protocol,
    pub status: CatalogStatus,
    pub mode: SourceProtocolMode,
    pub source_protocol: Option<Protocol>,
    pub adapter: Option<String>,
    pub feature_capabilities: Value,
    pub field_source: String,
    pub observed_at: DateTime<Utc>,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub unavailable_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

impl SourceModelCapabilityRecord {
    #[allow(dead_code)]
    pub fn is_routable(&self) -> bool {
        self.status == CatalogStatus::Confirmed && self.mode.is_routable()
    }
}

#[derive(Clone, Debug)]
pub struct ModelBindingInput {
    pub logical_model_id: String,
    pub source_id: String,
    pub account_id: String,
    pub upstream_model_id: String,
    pub protocol: Protocol,
    pub priority: i32,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct ModelBindingRecord {
    pub id: i64,
    pub logical_model_id: String,
    pub source_id: String,
    pub account_id: String,
    pub upstream_model_id: String,
    pub protocol: Protocol,
    pub status: CatalogStatus,
    pub priority: i32,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub unavailable_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct RoutableBindingRecord {
    pub binding_id: i64,
    pub logical_model_id: String,
    pub public_name: String,
    pub source_id: String,
    pub account_id: String,
    pub upstream_model_id: String,
    pub protocol: Protocol,
    pub mode: SourceProtocolMode,
    pub source_protocol: Option<Protocol>,
    pub adapter: Option<String>,
    pub feature_capabilities: Value,
    pub priority: i32,
}

#[derive(Clone)]
pub struct ModelCatalogRepository {
    pool: PgPool,
}

#[allow(dead_code)]
impl ModelCatalogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list_provider_presets(&self) -> Result<Vec<ProviderPresetRecord>, CatalogError> {
        sqlx::query_as::<_, ProviderPresetRecord>(
            "SELECT id,version,display_name,definition,created_at FROM provider_presets ORDER BY id,version DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn get_provider_preset(
        &self,
        id: &str,
        version: i32,
    ) -> Result<ProviderPresetRecord, CatalogError> {
        sqlx::query_as::<_, ProviderPresetRecord>("SELECT id,version,display_name,definition,created_at FROM provider_presets WHERE id=$1 AND version=$2")
            .bind(id)
            .bind(version)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| CatalogError::NotFound(format!("provider preset {id} version {version} not found")))
    }

    pub async fn latest_provider_preset(
        &self,
        id: &str,
    ) -> Result<ProviderPresetRecord, CatalogError> {
        sqlx::query_as::<_, ProviderPresetRecord>("SELECT id,version,display_name,definition,created_at FROM provider_presets WHERE id=$1 ORDER BY version DESC LIMIT 1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| CatalogError::NotFound(format!("provider preset {id} not found")))
    }

    pub async fn insert_provider_preset(
        &self,
        input: &ProviderPresetInput,
    ) -> Result<ProviderPresetRecord, CatalogError> {
        if input.version <= 0 || !input.definition.is_object() {
            return Err(CatalogError::InvalidState(
                "provider preset version must be positive and definition must be an object"
                    .to_owned(),
            ));
        }
        sqlx::query("INSERT INTO provider_presets (id,version,display_name,definition) VALUES ($1,$2,$3,$4) ON CONFLICT (id,version) DO NOTHING")
            .bind(&input.id)
            .bind(input.version)
            .bind(&input.display_name)
            .bind(&input.definition)
            .execute(&self.pool)
            .await?;
        let record = sqlx::query_as::<_, ProviderPresetRecord>("SELECT id,version,display_name,definition,created_at FROM provider_presets WHERE id=$1 AND version=$2")
            .bind(&input.id)
            .bind(input.version)
            .fetch_one(&self.pool)
            .await?;
        if record.display_name != input.display_name || record.definition != input.definition {
            return Err(CatalogError::ImmutableVersionConflict(format!(
                "provider preset {} version {} already exists with different content",
                input.id, input.version
            )));
        }
        Ok(record)
    }

    pub async fn create_source(&self, input: &SourceInput) -> Result<SourceRecord, CatalogError> {
        for (name, value) in [
            ("endpoints", &input.endpoints),
            ("auth_config", &input.auth_config),
            ("protocol_capabilities", &input.protocol_capabilities),
        ] {
            if !value.is_object() {
                return Err(CatalogError::InvalidState(format!(
                    "source {name} must be an object"
                )));
            }
        }
        let snapshot: Value = sqlx::query_scalar(
            "SELECT definition FROM provider_presets WHERE id=$1 AND version=$2",
        )
        .bind(&input.provider_preset_id)
        .bind(input.provider_preset_version)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            CatalogError::NotFound(format!(
                "provider preset {} version {} not found",
                input.provider_preset_id, input.provider_preset_version
            ))
        })?;
        sqlx::query_as::<_, SourceRecord>("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled,created_at,updated_at")
            .bind(&input.id)
            .bind(&input.display_name)
            .bind(&input.provider_preset_id)
            .bind(input.provider_preset_version)
            .bind(snapshot)
            .bind(&input.base_url)
            .bind(&input.endpoints)
            .bind(&input.auth_config)
            .bind(&input.protocol_capabilities)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn get_source(&self, id: &str) -> Result<SourceRecord, CatalogError> {
        sqlx::query_as::<_, SourceRecord>("SELECT id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled,created_at,updated_at FROM sources WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| CatalogError::NotFound(format!("source {id} not found")))
    }

    pub async fn list_sources(&self) -> Result<Vec<SourceRecord>, CatalogError> {
        sqlx::query_as::<_, SourceRecord>("SELECT id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled,created_at,updated_at FROM sources ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn get_account_credential_ref(
        &self,
        source_id: &str,
        account_id: &str,
    ) -> Result<AccountCredentialRef, CatalogError> {
        sqlx::query_as::<_, AccountCredentialRef>("SELECT id,source_id,credential_env,(credential_ciphertext IS NOT NULL) AS has_credential_ciphertext FROM accounts WHERE id=$1 AND source_id=$2 AND enabled")
            .bind(account_id)
            .bind(source_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| CatalogError::NotFound(format!("enabled account {account_id} for source {source_id} not found")))
    }

    pub async fn insert_model_preset(
        &self,
        input: &ModelPresetInput,
    ) -> Result<ModelPresetRecord, CatalogError> {
        if input.version <= 0 {
            return Err(CatalogError::InvalidState(
                "model preset version must be positive".to_owned(),
            ));
        }
        if input
            .metadata
            .field_sources
            .values()
            .any(|source| !matches!(source, MetadataSource::Preset | MetadataSource::Unknown))
        {
            return Err(CatalogError::InvalidMetadata(
                "model preset fields may only use preset or unknown sources".to_owned(),
            ));
        }
        let (metadata, field_sources) = input.metadata.to_json()?;
        let aliases = serde_json::to_value(&input.aliases)?;
        sqlx::query("INSERT INTO model_presets (id,version,canonical_model_id,aliases,metadata,field_sources) VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (id,version) DO NOTHING")
            .bind(&input.id)
            .bind(input.version)
            .bind(&input.canonical_model_id)
            .bind(&aliases)
            .bind(&metadata)
            .bind(&field_sources)
            .execute(&self.pool)
            .await?;
        let record = sqlx::query_as::<_, ModelPresetRecord>("SELECT id,version,canonical_model_id,aliases,metadata,field_sources,created_at,updated_at FROM model_presets WHERE id=$1 AND version=$2")
            .bind(&input.id)
            .bind(input.version)
            .fetch_one(&self.pool)
            .await?;
        if record.canonical_model_id != input.canonical_model_id
            || record.aliases != aliases
            || record.metadata != metadata
            || record.field_sources != field_sources
        {
            return Err(CatalogError::ImmutableVersionConflict(format!(
                "model preset {} version {} already exists with different content",
                input.id, input.version
            )));
        }
        Ok(record)
    }

    pub async fn match_model_preset(
        &self,
        upstream_model_id: &str,
    ) -> Result<Option<ModelPresetRecord>, CatalogError> {
        sqlx::query_as::<_, ModelPresetRecord>("SELECT id,version,canonical_model_id,aliases,metadata,field_sources,created_at,updated_at FROM model_presets WHERE canonical_model_id=$1 OR aliases ? $1 ORDER BY version DESC,id LIMIT 1")
            .bind(upstream_model_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn refresh_source_model(
        &self,
        refresh: &SourceModelRefresh,
    ) -> Result<SourceModelRecord, CatalogError> {
        validate_source_model_refresh(refresh)?;
        let mut tx = self.pool.begin().await?;
        let record = refresh_source_model_tx(&mut tx, refresh).await?;
        tx.commit().await?;
        Ok(record)
    }

    pub async fn list_source_models(
        &self,
        source_id: &str,
        confirmation_status: Option<CatalogStatus>,
        availability_status: Option<CatalogAvailability>,
    ) -> Result<Vec<SourceModelRecord>, CatalogError> {
        sqlx::query_as::<_, SourceModelRecord>("SELECT source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,matched_model_preset_id,matched_model_preset_version,first_discovered_at,last_discovered_at,confirmed_at,unavailable_at,created_at,updated_at FROM source_models WHERE source_id=$1 AND ($2::catalog_status IS NULL OR confirmation_status=$2) AND ($3::catalog_availability IS NULL OR availability_status=$3) ORDER BY upstream_model_id")
            .bind(source_id)
            .bind(confirmation_status)
            .bind(availability_status)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn update_source_model_user_overrides(
        &self,
        source_id: &str,
        upstream_model_id: &str,
        user_overrides: &MetadataValues,
    ) -> Result<SourceModelRecord, CatalogError> {
        let mut tx = self.pool.begin().await?;
        let existing = fetch_source_model_for_update(&mut tx, source_id, upstream_model_id).await?;
        let mut metadata = existing.catalog_metadata()?;
        metadata.apply_user_overrides(user_overrides)?;
        let (values, sources) = metadata.to_json()?;
        sqlx::query("UPDATE source_models SET metadata=$3,field_sources=$4,updated_at=NOW() WHERE source_id=$1 AND upstream_model_id=$2")
            .bind(source_id)
            .bind(upstream_model_id)
            .bind(values)
            .bind(sources)
            .execute(&mut *tx)
            .await?;
        let record = fetch_source_model(&mut tx, source_id, upstream_model_id).await?;
        tx.commit().await?;
        Ok(record)
    }

    pub async fn confirm_source_model(
        &self,
        source_id: &str,
        upstream_model_id: &str,
        user_overrides: &MetadataValues,
    ) -> Result<SourceModelRecord, CatalogError> {
        self.confirm_source_models(
            source_id,
            &[SourceModelConfirmation {
                upstream_model_id: upstream_model_id.to_owned(),
                user_overrides: user_overrides.clone(),
            }],
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| CatalogError::InvalidState("confirmation set cannot be empty".into()))
    }

    pub async fn confirm_source_models(
        &self,
        source_id: &str,
        confirmations: &[SourceModelConfirmation],
    ) -> Result<Vec<SourceModelRecord>, CatalogError> {
        if confirmations.is_empty() {
            return Err(CatalogError::InvalidState(
                "at least one source model confirmation is required".into(),
            ));
        }
        let mut ordered = confirmations.iter().collect::<Vec<_>>();
        ordered.sort_by(|left, right| left.upstream_model_id.cmp(&right.upstream_model_id));
        if ordered
            .windows(2)
            .any(|items| items[0].upstream_model_id == items[1].upstream_model_id)
        {
            return Err(CatalogError::InvalidState(
                "source model confirmation IDs must be unique".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        lock_source(&mut tx, source_id).await?;
        let mut records = Vec::with_capacity(ordered.len());
        for confirmation in ordered {
            let existing =
                fetch_source_model_for_update(&mut tx, source_id, &confirmation.upstream_model_id)
                    .await?;
            if existing.availability_status != CatalogAvailability::Available {
                return Err(CatalogError::InvalidState(format!(
                    "only an available source model can be confirmed: {}",
                    confirmation.upstream_model_id
                )));
            }
            let mut metadata = existing.catalog_metadata()?;
            metadata.apply_user_overrides(&confirmation.user_overrides)?;
            let (values, sources) = metadata.to_json()?;
            sqlx::query("UPDATE source_models SET confirmation_status='confirmed',metadata=$3,field_sources=$4,confirmed_at=COALESCE(confirmed_at,NOW()),updated_at=NOW() WHERE source_id=$1 AND upstream_model_id=$2")
                .bind(source_id)
                .bind(&confirmation.upstream_model_id)
                .bind(values)
                .bind(sources)
                .execute(&mut *tx)
                .await?;
            records.push(
                fetch_source_model(&mut tx, source_id, &confirmation.upstream_model_id).await?,
            );
        }
        tx.commit().await?;
        Ok(records)
    }

    pub async fn mark_source_model_unavailable(
        &self,
        source_id: &str,
        upstream_model_id: &str,
        at: DateTime<Utc>,
    ) -> Result<SourceModelRecord, CatalogError> {
        sqlx::query_as::<_, SourceModelRecord>("UPDATE source_models SET availability_status='unavailable',unavailable_at=$3,updated_at=NOW() WHERE source_id=$1 AND upstream_model_id=$2 RETURNING source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,matched_model_preset_id,matched_model_preset_version,first_discovered_at,last_discovered_at,confirmed_at,unavailable_at,created_at,updated_at")
            .bind(source_id)
            .bind(upstream_model_id)
            .bind(at)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| CatalogError::NotFound(format!("source model {source_id}/{upstream_model_id} not found")))
    }

    pub async fn apply_discovery(
        &self,
        input: &DiscoveryApplyInput,
    ) -> Result<DiscoveryApplyResult, CatalogError> {
        if input.http_status < 200 || input.http_status > 299 {
            return Err(CatalogError::InvalidState(
                "successful discovery requires a 2xx HTTP status".into(),
            ));
        }
        if input.latency_ms < 0 || input.completed_at < input.started_at {
            return Err(CatalogError::InvalidState(
                "discovery timing metadata is invalid".into(),
            ));
        }
        if input.raw_snapshot.is_null() {
            return Err(CatalogError::InvalidMetadata(
                "discovery raw snapshot cannot be null".into(),
            ));
        }
        let mut discovered = input.models.iter().collect::<Vec<_>>();
        discovered.sort_by(|left, right| left.upstream_model_id.cmp(&right.upstream_model_id));
        if discovered
            .windows(2)
            .any(|items| items[0].upstream_model_id == items[1].upstream_model_id)
        {
            return Err(CatalogError::InvalidMetadata(
                "discovery result contains duplicate model IDs".into(),
            ));
        }
        for refresh in &discovered {
            validate_source_model_refresh(refresh)?;
            if refresh.source_id != input.source_id {
                return Err(CatalogError::InvalidState(
                    "discovery models must belong to the requested source".into(),
                ));
            }
        }

        let mut tx = self.pool.begin().await?;
        lock_source(&mut tx, &input.source_id).await?;
        let source: (String, i32) = sqlx::query_as(
            "SELECT provider_preset_id,provider_preset_version FROM sources WHERE id=$1 FOR UPDATE",
        )
        .bind(&input.source_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| CatalogError::NotFound(format!("source {} not found", input.source_id)))?;
        validate_account_for_source(&mut tx, &input.source_id, input.account_id.as_deref()).await?;
        let before = sqlx::query_as::<_, SourceModelRecord>("SELECT source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,matched_model_preset_id,matched_model_preset_version,first_discovered_at,last_discovered_at,confirmed_at,unavailable_at,created_at,updated_at FROM source_models WHERE source_id=$1 ORDER BY upstream_model_id FOR UPDATE")
            .bind(&input.source_id)
            .fetch_all(&mut *tx)
            .await?;
        let mut before_by_id = before
            .into_iter()
            .map(|record| (record.upstream_model_id.clone(), record))
            .collect::<BTreeMap<_, _>>();
        let mut diff = DiscoveryDiff::default();
        let mut records = Vec::new();
        for refresh in discovered {
            let previous = before_by_id.remove(&refresh.upstream_model_id);
            let current = refresh_source_model_tx(&mut tx, refresh).await?;
            match previous.as_ref() {
                None => diff.added.push(DiscoveryDiffEntry {
                    upstream_model_id: current.upstream_model_id.clone(),
                    changed_fields: vec!["source_model".into()],
                }),
                Some(previous) => {
                    let changed_fields = changed_source_model_fields(previous, &current);
                    if !changed_fields.is_empty() {
                        diff.changed.push(DiscoveryDiffEntry {
                            upstream_model_id: current.upstream_model_id.clone(),
                            changed_fields,
                        });
                    }
                }
            }
            records.push(current);
        }
        for (_, previous) in before_by_id {
            if previous.availability_status == CatalogAvailability::Unavailable {
                continue;
            }
            let current = sqlx::query_as::<_, SourceModelRecord>("UPDATE source_models SET availability_status='unavailable',unavailable_at=$3,updated_at=NOW() WHERE source_id=$1 AND upstream_model_id=$2 RETURNING source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,matched_model_preset_id,matched_model_preset_version,first_discovered_at,last_discovered_at,confirmed_at,unavailable_at,created_at,updated_at")
                .bind(&input.source_id)
                .bind(&previous.upstream_model_id)
                .bind(input.completed_at)
                .fetch_one(&mut *tx)
                .await?;
            diff.missing.push(DiscoveryDiffEntry {
                upstream_model_id: current.upstream_model_id.clone(),
                changed_fields: vec!["availability_status".into()],
            });
            records.push(current);
        }
        sort_discovery_diff(&mut diff);
        records.sort_by(|left, right| left.upstream_model_id.cmp(&right.upstream_model_id));
        let diff_value = serde_json::to_value(&diff)?;
        let discovered_model_count = i32::try_from(input.models.len()).map_err(|_| {
            CatalogError::InvalidState("discovery result contains too many models".into())
        })?;
        let run = sqlx::query_as::<_, DiscoveryRunRecord>("INSERT INTO source_discovery_runs (source_id,account_id,provider_preset_id,provider_preset_version,status,raw_snapshot,diff,discovered_model_count,http_status,latency_ms,requested_by,started_at,completed_at) VALUES ($1,$2,$3,$4,'succeeded',$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id,source_id,account_id,provider_preset_id,provider_preset_version,status,raw_snapshot,diff,discovered_model_count,http_status,latency_ms,error_code,error_message,requested_by,started_at,completed_at")
            .bind(&input.source_id)
            .bind(&input.account_id)
            .bind(&source.0)
            .bind(source.1)
            .bind(&input.raw_snapshot)
            .bind(&diff_value)
            .bind(discovered_model_count)
            .bind(input.http_status)
            .bind(input.latency_ms)
            .bind(&input.requested_by)
            .bind(input.started_at)
            .bind(input.completed_at)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(DiscoveryApplyResult {
            run,
            diff,
            models: records,
        })
    }

    pub async fn record_discovery_failure(
        &self,
        input: &DiscoveryFailureInput,
    ) -> Result<DiscoveryRunRecord, CatalogError> {
        if !matches!(input.status.as_str(), "failed" | "unsupported")
            || input.error_code.trim().is_empty()
            || input.error_message.trim().is_empty()
            || input.latency_ms < 0
            || input.completed_at < input.started_at
        {
            return Err(CatalogError::InvalidState(
                "invalid discovery failure audit record".into(),
            ));
        }
        let source = self.get_source(&input.source_id).await?;
        if let Some(account_id) = input.account_id.as_deref() {
            self.get_account_credential_ref(&input.source_id, account_id)
                .await?;
        }
        let empty_diff = serde_json::to_value(DiscoveryDiff::default())?;
        sqlx::query_as::<_, DiscoveryRunRecord>("INSERT INTO source_discovery_runs (source_id,account_id,provider_preset_id,provider_preset_version,status,diff,http_status,latency_ms,error_code,error_message,requested_by,started_at,completed_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING id,source_id,account_id,provider_preset_id,provider_preset_version,status,raw_snapshot,diff,discovered_model_count,http_status,latency_ms,error_code,error_message,requested_by,started_at,completed_at")
            .bind(&input.source_id)
            .bind(&input.account_id)
            .bind(&source.provider_preset_id)
            .bind(source.provider_preset_version)
            .bind(&input.status)
            .bind(empty_diff)
            .bind(input.http_status)
            .bind(input.latency_ms)
            .bind(&input.error_code)
            .bind(&input.error_message)
            .bind(&input.requested_by)
            .bind(input.started_at)
            .bind(input.completed_at)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn latest_discovery_run(
        &self,
        source_id: &str,
    ) -> Result<Option<DiscoveryRunRecord>, CatalogError> {
        sqlx::query_as::<_, DiscoveryRunRecord>("SELECT id,source_id,account_id,provider_preset_id,provider_preset_version,status,raw_snapshot,diff,discovered_model_count,http_status,latency_ms,error_code,error_message,requested_by,started_at,completed_at FROM source_discovery_runs WHERE source_id=$1 ORDER BY completed_at DESC,id DESC LIMIT 1")
            .bind(source_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn record_connection_test(
        &self,
        input: &ConnectionTestInput,
    ) -> Result<ConnectionTestRecord, CatalogError> {
        if !matches!(input.status.as_str(), "succeeded" | "failed")
            || input.latency_ms < 0
            || (input.status == "succeeded"
                && (!input
                    .http_status
                    .is_some_and(|status| (200..300).contains(&status))
                    || input.error_code.is_some()
                    || input.error_message.is_some()))
            || (input.status == "failed"
                && (input.error_code.as_deref().is_none_or(str::is_empty)
                    || input.error_message.as_deref().is_none_or(str::is_empty)))
        {
            return Err(CatalogError::InvalidState(
                "invalid connection test audit record".into(),
            ));
        }
        self.get_source(&input.source_id).await?;
        if let Some(account_id) = input.account_id.as_deref() {
            self.get_account_credential_ref(&input.source_id, account_id)
                .await?;
        }
        sqlx::query_as::<_, ConnectionTestRecord>("INSERT INTO source_connection_tests (source_id,account_id,protocol,upstream_protocol,mode,status,http_status,latency_ms,error_code,error_message,requested_by,tested_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id,source_id,account_id,protocol,upstream_protocol,mode,status,http_status,latency_ms,error_code,error_message,requested_by,tested_at")
            .bind(&input.source_id)
            .bind(&input.account_id)
            .bind(input.protocol)
            .bind(input.upstream_protocol)
            .bind(input.mode)
            .bind(&input.status)
            .bind(input.http_status)
            .bind(input.latency_ms)
            .bind(&input.error_code)
            .bind(&input.error_message)
            .bind(&input.requested_by)
            .bind(input.tested_at)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn create_logical_model(
        &self,
        input: &LogicalModelInput,
    ) -> Result<LogicalModelRecord, CatalogError> {
        let (metadata, field_sources) = input.metadata.to_json()?;
        let (preset_id, preset_version) = preset_parts(input.model_preset.as_ref());
        let confirmed_at = (input.status == CatalogStatus::Confirmed).then(Utc::now);
        let unavailable_at = (input.status == CatalogStatus::Unavailable).then(Utc::now);
        sqlx::query_as::<_, LogicalModelRecord>("INSERT INTO logical_models (id,public_name,display_name,status,model_preset_id,model_preset_version,metadata,field_sources,confirmed_at,unavailable_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id,public_name,display_name,status,model_preset_id,model_preset_version,metadata,field_sources,confirmed_at,unavailable_at,created_at,updated_at")
            .bind(&input.id)
            .bind(&input.public_name)
            .bind(&input.display_name)
            .bind(input.status)
            .bind(preset_id)
            .bind(preset_version)
            .bind(metadata)
            .bind(field_sources)
            .bind(confirmed_at)
            .bind(unavailable_at)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn transition_logical_model_status(
        &self,
        id: &str,
        next: CatalogStatus,
    ) -> Result<LogicalModelRecord, CatalogError> {
        let current: CatalogStatus =
            sqlx::query_scalar("SELECT status FROM logical_models WHERE id=$1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or_else(|| CatalogError::NotFound(format!("logical model {id} not found")))?;
        ensure_transition(current, next, "logical model")?;
        sqlx::query_as::<_, LogicalModelRecord>("UPDATE logical_models SET status=$2,confirmed_at=CASE WHEN $2='confirmed' THEN COALESCE(confirmed_at,NOW()) ELSE confirmed_at END,unavailable_at=CASE WHEN $2='unavailable' THEN NOW() ELSE NULL END,updated_at=NOW() WHERE id=$1 RETURNING id,public_name,display_name,status,model_preset_id,model_preset_version,metadata,field_sources,confirmed_at,unavailable_at,created_at,updated_at")
            .bind(id)
            .bind(next)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn upsert_source_model_capability(
        &self,
        input: &SourceModelCapabilityInput,
    ) -> Result<SourceModelCapabilityRecord, CatalogError> {
        input.validate()?;
        if let Some(current_status) = sqlx::query_scalar::<_, CatalogStatus>(
            "SELECT status FROM source_model_capabilities WHERE source_id=$1 AND upstream_model_id=$2 AND protocol=$3",
        )
        .bind(&input.source_id)
        .bind(&input.upstream_model_id)
        .bind(input.protocol)
        .fetch_optional(&self.pool)
        .await?
        {
            ensure_transition(current_status, input.status, "source model capability")?;
            if current_status == CatalogStatus::Confirmed
                && input.status == CatalogStatus::Confirmed
                && input.field_source != MetadataSource::User
            {
                return self
                    .get_source_model_capability(
                        &input.source_id,
                        &input.upstream_model_id,
                        input.protocol,
                    )
                    .await;
            }
        }
        if input.mode == SourceProtocolMode::Adapter {
            let source_protocol = input
                .source_protocol
                .expect("validated adapter source protocol");
            let source = sqlx::query_as::<_, (CatalogStatus, SourceProtocolMode)>("SELECT status,mode FROM source_model_capabilities WHERE source_id=$1 AND upstream_model_id=$2 AND protocol=$3")
                .bind(&input.source_id)
                .bind(&input.upstream_model_id)
                .bind(source_protocol)
                .fetch_optional(&self.pool)
                .await?;
            if source != Some((CatalogStatus::Confirmed, SourceProtocolMode::Native)) {
                return Err(CatalogError::InvalidState(format!(
                    "adapter source protocol {source_protocol} must be confirmed native"
                )));
            }
        }
        let features = serde_json::to_value(&input.feature_capabilities)?;
        let confirmed_at = (input.status == CatalogStatus::Confirmed).then(Utc::now);
        let unavailable_at = (input.status == CatalogStatus::Unavailable).then(Utc::now);
        sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,source_protocol,adapter,feature_capabilities,field_source,observed_at,confirmed_at,unavailable_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) ON CONFLICT (source_id,upstream_model_id,protocol) DO UPDATE SET status=EXCLUDED.status,mode=EXCLUDED.mode,source_protocol=EXCLUDED.source_protocol,adapter=EXCLUDED.adapter,feature_capabilities=EXCLUDED.feature_capabilities,field_source=EXCLUDED.field_source,observed_at=EXCLUDED.observed_at,confirmed_at=CASE WHEN EXCLUDED.status='confirmed' THEN COALESCE(source_model_capabilities.confirmed_at,EXCLUDED.confirmed_at) ELSE source_model_capabilities.confirmed_at END,unavailable_at=EXCLUDED.unavailable_at,updated_at=NOW() WHERE source_model_capabilities.status <> 'confirmed' OR EXCLUDED.status <> 'confirmed' OR EXCLUDED.field_source = 'user'")
            .bind(&input.source_id)
            .bind(&input.upstream_model_id)
            .bind(input.protocol)
            .bind(input.status)
            .bind(input.mode)
            .bind(input.source_protocol)
            .bind(&input.adapter)
            .bind(features)
            .bind(input.field_source.to_string())
            .bind(input.observed_at)
            .bind(confirmed_at)
            .bind(unavailable_at)
            .execute(&self.pool)
            .await?;
        self.get_source_model_capability(&input.source_id, &input.upstream_model_id, input.protocol)
            .await
    }

    pub async fn get_source_model_capability(
        &self,
        source_id: &str,
        upstream_model_id: &str,
        protocol: Protocol,
    ) -> Result<SourceModelCapabilityRecord, CatalogError> {
        sqlx::query_as::<_, SourceModelCapabilityRecord>("SELECT source_id,upstream_model_id,protocol,status,mode,source_protocol,adapter,feature_capabilities,field_source,observed_at,confirmed_at,unavailable_at,updated_at FROM source_model_capabilities WHERE source_id=$1 AND upstream_model_id=$2 AND protocol=$3")
            .bind(source_id)
            .bind(upstream_model_id)
            .bind(protocol)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| CatalogError::NotFound(format!("source model capability {source_id}/{upstream_model_id}/{protocol} not found")))
    }

    pub async fn create_model_binding(
        &self,
        input: &ModelBindingInput,
    ) -> Result<ModelBindingRecord, CatalogError> {
        sqlx::query_as::<_, ModelBindingRecord>("INSERT INTO model_bindings (logical_model_id,source_id,account_id,upstream_model_id,protocol,priority) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id,logical_model_id,source_id,account_id,upstream_model_id,protocol,status,priority,confirmed_at,unavailable_at,created_at,updated_at")
            .bind(&input.logical_model_id)
            .bind(&input.source_id)
            .bind(&input.account_id)
            .bind(&input.upstream_model_id)
            .bind(input.protocol)
            .bind(input.priority)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn transition_model_binding_status(
        &self,
        id: i64,
        next: CatalogStatus,
    ) -> Result<ModelBindingRecord, CatalogError> {
        let current: CatalogStatus =
            sqlx::query_scalar("SELECT status FROM model_bindings WHERE id=$1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or_else(|| CatalogError::NotFound(format!("model binding {id} not found")))?;
        ensure_transition(current, next, "model binding")?;
        sqlx::query_as::<_, ModelBindingRecord>("UPDATE model_bindings SET status=$2,confirmed_at=CASE WHEN $2='confirmed' THEN COALESCE(confirmed_at,NOW()) ELSE confirmed_at END,unavailable_at=CASE WHEN $2='unavailable' THEN NOW() ELSE NULL END,updated_at=NOW() WHERE id=$1 RETURNING id,logical_model_id,source_id,account_id,upstream_model_id,protocol,status,priority,confirmed_at,unavailable_at,created_at,updated_at")
            .bind(id)
            .bind(next)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn list_routable_bindings(
        &self,
        public_name: &str,
        protocol: Protocol,
    ) -> Result<Vec<RoutableBindingRecord>, CatalogError> {
        sqlx::query_as::<_, RoutableBindingRecord>("SELECT b.id AS binding_id,b.logical_model_id,lm.public_name,b.source_id,b.account_id,b.upstream_model_id,b.protocol,capability.mode,capability.source_protocol,capability.adapter,capability.feature_capabilities,b.priority FROM model_bindings b JOIN logical_models lm ON lm.id=b.logical_model_id JOIN sources s ON s.id=b.source_id JOIN accounts a ON a.id=b.account_id AND a.source_id=b.source_id JOIN source_models sm ON sm.source_id=b.source_id AND sm.upstream_model_id=b.upstream_model_id JOIN source_model_capabilities capability ON capability.source_id=b.source_id AND capability.upstream_model_id=b.upstream_model_id AND capability.protocol=b.protocol WHERE lm.public_name=$1 AND b.protocol=$2 AND lm.status='confirmed' AND b.status='confirmed' AND s.enabled AND a.enabled AND sm.confirmation_status='confirmed' AND sm.availability_status='available' AND capability.status='confirmed' AND capability.mode IN ('native','adapter') ORDER BY b.priority DESC,b.id")
            .bind(public_name)
            .bind(protocol)
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }
}

fn validate_source_model_refresh(refresh: &SourceModelRefresh) -> Result<(), CatalogError> {
    if refresh.source_id.trim().is_empty() || refresh.upstream_model_id.trim().is_empty() {
        return Err(CatalogError::InvalidMetadata(
            "source and upstream model IDs cannot be empty".into(),
        ));
    }
    if !refresh.raw_snapshot.is_object() {
        return Err(CatalogError::InvalidMetadata(
            "source model raw snapshot must be an object".to_owned(),
        ));
    }
    if refresh.matched_preset.is_some() != refresh.preset_metadata.is_some() {
        return Err(CatalogError::InvalidMetadata(
            "matched model preset reference and metadata must be provided together".to_owned(),
        ));
    }
    refresh.upstream_metadata.validate()?;
    if let Some(metadata) = refresh.preset_metadata.as_ref() {
        metadata.validate()?;
    }
    Ok(())
}

async fn refresh_source_model_tx(
    tx: &mut Transaction<'_, Postgres>,
    refresh: &SourceModelRefresh,
) -> Result<SourceModelRecord, CatalogError> {
    validate_source_model_refresh(refresh)?;
    let discovered =
        CatalogMetadata::resolve(&refresh.upstream_metadata, refresh.preset_metadata.as_ref())?;
    let (metadata, field_sources) = discovered.to_json()?;
    let (preset_id, preset_version) = preset_parts(refresh.matched_preset.as_ref());
    let inserted = sqlx::query("INSERT INTO source_models (source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,matched_model_preset_id,matched_model_preset_version,first_discovered_at,last_discovered_at) VALUES ($1,$2,'pending','available',$3,$4,$5,$6,$7,$8,$8) ON CONFLICT (source_id,upstream_model_id) DO NOTHING")
        .bind(&refresh.source_id)
        .bind(&refresh.upstream_model_id)
        .bind(&refresh.raw_snapshot)
        .bind(&metadata)
        .bind(&field_sources)
        .bind(preset_id)
        .bind(preset_version)
        .bind(refresh.discovered_at)
        .execute(&mut **tx)
        .await?
        .rows_affected()
        > 0;
    if !inserted {
        let existing =
            fetch_source_model_for_update(tx, &refresh.source_id, &refresh.upstream_model_id)
                .await?;
        let existing_metadata = existing.catalog_metadata()?;
        let refreshed = existing_metadata.refreshed_preserving_user_fields(
            existing.confirmation_status == CatalogStatus::Confirmed,
            &refresh.upstream_metadata,
            refresh.preset_metadata.as_ref(),
        )?;
        let (metadata, field_sources) = refreshed.to_json()?;
        let (update_preset_id, update_preset_version) =
            if existing.confirmation_status == CatalogStatus::Confirmed {
                (
                    existing.matched_model_preset_id.as_deref(),
                    existing.matched_model_preset_version,
                )
            } else {
                (preset_id, preset_version)
            };
        sqlx::query("UPDATE source_models SET availability_status='available',raw_snapshot=$3,metadata=$4,field_sources=$5,matched_model_preset_id=$6,matched_model_preset_version=$7,last_discovered_at=$8,unavailable_at=NULL,updated_at=NOW() WHERE source_id=$1 AND upstream_model_id=$2")
            .bind(&refresh.source_id)
            .bind(&refresh.upstream_model_id)
            .bind(&refresh.raw_snapshot)
            .bind(metadata)
            .bind(field_sources)
            .bind(update_preset_id)
            .bind(update_preset_version)
            .bind(refresh.discovered_at)
            .execute(&mut **tx)
            .await?;
    }
    fetch_source_model(tx, &refresh.source_id, &refresh.upstream_model_id).await
}

async fn lock_source(
    tx: &mut Transaction<'_, Postgres>,
    source_id: &str,
) -> Result<(), CatalogError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(source_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn validate_account_for_source(
    tx: &mut Transaction<'_, Postgres>,
    source_id: &str,
    account_id: Option<&str>,
) -> Result<(), CatalogError> {
    let Some(account_id) = account_id else {
        return Ok(());
    };
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=$1 AND source_id=$2 AND enabled)",
    )
    .bind(account_id)
    .bind(source_id)
    .fetch_one(&mut **tx)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(CatalogError::NotFound(format!(
            "enabled account {account_id} for source {source_id} not found"
        )))
    }
}

fn changed_source_model_fields(
    before: &SourceModelRecord,
    after: &SourceModelRecord,
) -> Vec<String> {
    let mut fields = Vec::new();
    if before.raw_snapshot != after.raw_snapshot {
        fields.push("raw_snapshot".into());
    }
    if before.metadata != after.metadata {
        fields.push("metadata".into());
    }
    if before.field_sources != after.field_sources {
        fields.push("field_sources".into());
    }
    if before.matched_model_preset_id != after.matched_model_preset_id
        || before.matched_model_preset_version != after.matched_model_preset_version
    {
        fields.push("matched_model_preset".into());
    }
    if before.availability_status != after.availability_status {
        fields.push("availability_status".into());
    }
    fields
}

fn sort_discovery_diff(diff: &mut DiscoveryDiff) {
    for entries in [&mut diff.added, &mut diff.changed, &mut diff.missing] {
        entries.sort_by(|left, right| left.upstream_model_id.cmp(&right.upstream_model_id));
        for entry in entries {
            entry.changed_fields.sort();
        }
    }
}

fn preset_parts(preset: Option<&ModelPresetRef>) -> (Option<&str>, Option<i32>) {
    match preset {
        Some(preset) => (Some(preset.id.as_str()), Some(preset.version)),
        None => (None, None),
    }
}

fn ensure_transition(
    current: CatalogStatus,
    next: CatalogStatus,
    entity: &str,
) -> Result<(), CatalogError> {
    if current.can_transition_to(next) {
        Ok(())
    } else {
        Err(CatalogError::InvalidState(format!(
            "invalid {entity} status transition: {current:?} -> {next:?}"
        )))
    }
}

async fn fetch_source_model_for_update(
    tx: &mut Transaction<'_, Postgres>,
    source_id: &str,
    upstream_model_id: &str,
) -> Result<SourceModelRecord, CatalogError> {
    sqlx::query_as::<_, SourceModelRecord>("SELECT source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,matched_model_preset_id,matched_model_preset_version,first_discovered_at,last_discovered_at,confirmed_at,unavailable_at,created_at,updated_at FROM source_models WHERE source_id=$1 AND upstream_model_id=$2 FOR UPDATE")
        .bind(source_id)
        .bind(upstream_model_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| CatalogError::NotFound(format!("source model {source_id}/{upstream_model_id} not found")))
}

async fn fetch_source_model(
    tx: &mut Transaction<'_, Postgres>,
    source_id: &str,
    upstream_model_id: &str,
) -> Result<SourceModelRecord, CatalogError> {
    sqlx::query_as::<_, SourceModelRecord>("SELECT source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,matched_model_preset_id,matched_model_preset_version,first_discovered_at,last_discovered_at,confirmed_at,unavailable_at,created_at,updated_at FROM source_models WHERE source_id=$1 AND upstream_model_id=$2")
        .bind(source_id)
        .bind(upstream_model_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| CatalogError::NotFound(format!("source model {source_id}/{upstream_model_id} not found")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn metadata_priority_and_refresh_protection_are_explicit() {
        let upstream = MetadataValues::from_fields([
            (MetadataField::ContextWindow, json!(8_192)),
            (MetadataField::Tools, json!("supported")),
        ])
        .unwrap();
        let preset = MetadataValues::from_fields([
            (MetadataField::ContextWindow, json!(32_768)),
            (MetadataField::Thinking, json!("unsupported")),
            (MetadataField::Tools, json!("unknown")),
        ])
        .unwrap();
        let mut metadata = CatalogMetadata::resolve(&upstream, Some(&preset)).unwrap();
        assert_eq!(
            metadata.values.0[&MetadataField::ContextWindow],
            json!(32_768)
        );
        assert_eq!(
            metadata.field_sources[&MetadataField::ContextWindow],
            MetadataSource::Preset
        );
        assert_eq!(
            metadata.field_sources[&MetadataField::Tools],
            MetadataSource::Upstream
        );
        assert_eq!(
            metadata.field_sources[&MetadataField::Streaming],
            MetadataSource::Unknown
        );

        metadata
            .apply_user_overrides(
                &MetadataValues::from_fields([(MetadataField::ContextWindow, json!(65_536))])
                    .unwrap(),
            )
            .unwrap();
        let refreshed = metadata
            .refreshed_preserving_user_fields(
                false,
                &MetadataValues::from_fields([(MetadataField::ContextWindow, json!(16_384))])
                    .unwrap(),
                None,
            )
            .unwrap();
        assert_eq!(
            refreshed.values.0[&MetadataField::ContextWindow],
            json!(65_536)
        );
        assert_eq!(
            refreshed.field_sources[&MetadataField::ContextWindow],
            MetadataSource::User
        );
    }

    #[test]
    fn confirmed_metadata_is_not_silently_refreshed() {
        let original = CatalogMetadata::resolve(
            &MetadataValues::from_fields([(MetadataField::ContextWindow, json!(8_192))]).unwrap(),
            None,
        )
        .unwrap();
        let refreshed = original
            .refreshed_preserving_user_fields(
                true,
                &MetadataValues::from_fields([(MetadataField::ContextWindow, json!(128_000))])
                    .unwrap(),
                None,
            )
            .unwrap();
        assert_eq!(refreshed, original);
    }

    #[test]
    fn unknown_and_unsupported_capabilities_are_not_routable() {
        assert!(!SourceProtocolMode::Unknown.is_routable());
        assert!(!SourceProtocolMode::Unsupported.is_routable());
        assert!(SourceProtocolMode::Native.is_routable());
        assert!(SourceProtocolMode::Adapter.is_routable());

        let unknown = SourceModelCapabilityInput {
            source_id: "source".into(),
            upstream_model_id: "model".into(),
            protocol: Protocol::OpenAiResponses,
            status: CatalogStatus::Confirmed,
            mode: SourceProtocolMode::Unknown,
            source_protocol: None,
            adapter: None,
            feature_capabilities: BTreeMap::new(),
            field_source: MetadataSource::Unknown,
            observed_at: Utc::now(),
        };
        assert!(unknown.validate().is_err());
    }

    #[test]
    fn status_transitions_require_reconfirmation_after_unavailable() {
        assert!(CatalogStatus::Pending.can_transition_to(CatalogStatus::Confirmed));
        assert!(CatalogStatus::Confirmed.can_transition_to(CatalogStatus::Unavailable));
        assert!(!CatalogStatus::Unavailable.can_transition_to(CatalogStatus::Confirmed));
        assert!(CatalogStatus::Unavailable.can_transition_to(CatalogStatus::Pending));
    }
}
