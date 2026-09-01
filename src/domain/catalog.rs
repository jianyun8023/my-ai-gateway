use super::{config::adapter_definition, protocol::Protocol};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[derive(Clone, Debug, Serialize)]
pub struct PublishedModel {
    pub id: String,
    pub display_name: String,
    #[serde(skip)]
    pub account_ids: Vec<String>,
}
