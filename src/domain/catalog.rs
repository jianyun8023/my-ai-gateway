use super::{config::adapter_definition, protocol::Protocol};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, error::Error, fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatalogStatus {
    #[default]
    Pending,
    Confirmed,
    Unavailable,
}

impl CatalogStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Confirmed => "confirmed",
            Self::Unavailable => "unavailable",
        }
    }

    pub(crate) fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Pending, Self::Confirmed | Self::Unavailable)
                    | (Self::Confirmed, Self::Unavailable)
                    | (Self::Unavailable, Self::Pending)
            )
    }
}

impl fmt::Display for CatalogStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CatalogStatus {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "confirmed" => Ok(Self::Confirmed),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatalogAvailability {
    Unknown,
    Available,
    Unavailable,
}

impl CatalogAvailability {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Available => "available",
            Self::Unavailable => "unavailable",
        }
    }
}

impl fmt::Display for CatalogAvailability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CatalogAvailability {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "available" => Ok(Self::Available),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceProtocolMode {
    Unknown,
    Native,
    Adapter,
    Unsupported,
}

impl SourceProtocolMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Native => "native",
            Self::Adapter => "adapter",
            Self::Unsupported => "unsupported",
        }
    }

    pub(crate) fn is_routable(self) -> bool {
        matches!(self, Self::Native | Self::Adapter)
    }
}

impl fmt::Display for SourceProtocolMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SourceProtocolMode {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "native" => Ok(Self::Native),
            "adapter" => Ok(Self::Adapter),
            "unsupported" => Ok(Self::Unsupported),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MetadataSource {
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
pub(crate) enum MetadataField {
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
    pub(crate) const ALL: [Self; 13] = [
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
pub(crate) enum CapabilitySupport {
    Supported,
    Unsupported,
    Unknown,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(transparent)]
pub(crate) struct MetadataValues(pub BTreeMap<MetadataField, Value>);

impl MetadataValues {
    pub(crate) fn from_fields(
        fields: impl IntoIterator<Item = (MetadataField, Value)>,
    ) -> Result<Self, CatalogError> {
        let values = Self(fields.into_iter().collect());
        values.validate()?;
        Ok(values)
    }

    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
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
pub(crate) struct CatalogMetadata {
    pub(crate) values: MetadataValues,
    pub(crate) field_sources: BTreeMap<MetadataField, MetadataSource>,
}

impl CatalogMetadata {
    pub(crate) fn resolve(
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

    pub(crate) fn unknown() -> Self {
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

    pub(crate) fn apply_user_overrides(
        &mut self,
        overrides: &MetadataValues,
    ) -> Result<(), CatalogError> {
        overrides.validate()?;
        self.apply_values(overrides, MetadataSource::User);
        Ok(())
    }

    pub(crate) fn refreshed_preserving_user_fields(
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

    pub(crate) fn to_json(&self) -> Result<(Value, Value), CatalogError> {
        Ok((
            serde_json::to_value(&self.values)?,
            serde_json::to_value(&self.field_sources)?,
        ))
    }

    pub(crate) fn from_json(values: Value, field_sources: Value) -> Result<Self, CatalogError> {
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
pub(crate) enum CatalogError {
    Json(serde_json::Error),
    InvalidMetadata(String),
    InvalidState(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::InvalidMetadata(message) | Self::InvalidState(message) => f.write_str(message),
        }
    }
}

impl Error for CatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for CatalogError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ModelPresetRef {
    pub(crate) id: String,
    pub(crate) version: i32,
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderPresetInput {
    pub(crate) id: String,
    pub(crate) version: i32,
    pub(crate) display_name: String,
    pub(crate) definition: Value,
}

#[derive(Clone, Debug)]
#[cfg(test)]
pub(crate) struct SourceInput {
    pub(crate) id: String,
    pub(crate) display_name: String,
    pub(crate) provider_preset_id: String,
    pub(crate) provider_preset_version: i32,
    pub(crate) base_url: String,
    pub(crate) endpoints: Value,
    pub(crate) auth_config: Value,
    pub(crate) protocol_capabilities: Value,
}

#[derive(Clone, Debug)]
pub(crate) struct ModelPresetInput {
    pub(crate) id: String,
    pub(crate) version: i32,
    pub(crate) canonical_model_id: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) metadata: CatalogMetadata,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceModelRefresh {
    pub(crate) source_id: String,
    pub(crate) upstream_model_id: String,
    pub(crate) raw_snapshot: Value,
    pub(crate) upstream_metadata: MetadataValues,
    pub(crate) matched_preset: Option<ModelPresetRef>,
    pub(crate) preset_metadata: Option<MetadataValues>,
    pub(crate) discovered_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct DiscoveryDiffEntry {
    pub(crate) upstream_model_id: String,
    pub(crate) changed_fields: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct DiscoveryDiff {
    pub(crate) added: Vec<DiscoveryDiffEntry>,
    pub(crate) changed: Vec<DiscoveryDiffEntry>,
    pub(crate) missing: Vec<DiscoveryDiffEntry>,
}

#[derive(Clone, Debug)]
pub(crate) struct DiscoveryApplyInput {
    pub(crate) source_id: String,
    pub(crate) account_id: Option<String>,
    pub(crate) raw_snapshot: Value,
    pub(crate) models: Vec<SourceModelRefresh>,
    pub(crate) http_status: i32,
    pub(crate) latency_ms: i64,
    pub(crate) requested_by: String,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) completed_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub(crate) struct DiscoveryFailureInput {
    pub(crate) source_id: String,
    pub(crate) account_id: Option<String>,
    pub(crate) status: String,
    pub(crate) http_status: Option<i32>,
    pub(crate) latency_ms: i64,
    pub(crate) error_code: String,
    pub(crate) error_message: String,
    pub(crate) requested_by: String,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) completed_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub(crate) struct ConnectionTestInput {
    pub(crate) source_id: String,
    pub(crate) account_id: Option<String>,
    pub(crate) protocol: Protocol,
    pub(crate) upstream_protocol: Protocol,
    pub(crate) mode: SourceProtocolMode,
    pub(crate) status: String,
    pub(crate) http_status: Option<i32>,
    pub(crate) latency_ms: i64,
    pub(crate) error_code: Option<String>,
    pub(crate) error_message: Option<String>,
    pub(crate) requested_by: String,
    pub(crate) tested_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceModelConfirmation {
    pub(crate) upstream_model_id: String,
    pub(crate) user_overrides: MetadataValues,
}

#[derive(Clone, Debug)]
#[cfg(test)]
pub(crate) struct LogicalModelInput {
    pub(crate) id: String,
    pub(crate) public_name: String,
    pub(crate) display_name: String,
    pub(crate) status: CatalogStatus,
    pub(crate) model_preset: Option<ModelPresetRef>,
    pub(crate) metadata: CatalogMetadata,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceModelCapabilityInput {
    pub(crate) source_id: String,
    pub(crate) upstream_model_id: String,
    pub(crate) protocol: Protocol,
    pub(crate) status: CatalogStatus,
    pub(crate) mode: SourceProtocolMode,
    pub(crate) source_protocol: Option<Protocol>,
    pub(crate) adapter: Option<String>,
    pub(crate) feature_capabilities: BTreeMap<String, CapabilitySupport>,
    pub(crate) field_source: MetadataSource,
    pub(crate) observed_at: DateTime<Utc>,
}

impl SourceModelCapabilityInput {
    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
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

#[derive(Clone, Debug)]
#[cfg(test)]
pub(crate) struct ModelBindingInput {
    pub(crate) logical_model_id: String,
    pub(crate) source_id: String,
    pub(crate) account_id: String,
    pub(crate) upstream_model_id: String,
    pub(crate) protocol: Protocol,
    pub(crate) priority: i32,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PublishedModel {
    pub(crate) id: String,
    pub(crate) display_name: String,
    #[serde(skip)]
    pub(crate) account_ids: Vec<String>,
}
