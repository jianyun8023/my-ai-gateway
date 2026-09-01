use crate::{
    model_catalog::{
        CapabilitySupport, CatalogError, CatalogMetadata, MetadataField, MetadataValues,
        ModelCatalogRepository, ModelPresetInput, ProviderPresetInput, ProviderPresetRecord,
        SourceProtocolMode, SourceRecord,
    },
    protocol::Protocol,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const PROVIDER_PRESET_SCHEMA_VERSION: u32 = 1;
/// The latest built-in provider preset record version.  Record versions are
/// immutable snapshots; bumping this value never rewrites an existing Source.
pub const BUILTIN_PROVIDER_PRESET_VERSION: i32 = 2;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpMethod {
    Get,
    Post,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CredentialHeaderTemplate {
    pub header: String,
    pub prefix: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceAuthConfig {
    pub credential_header: CredentialHeaderTemplate,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ConnectionTestTemplate {
    pub method: HttpMethod,
    pub default_model: String,
    pub body: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProtocolPreset {
    pub endpoint: String,
    pub mode: SourceProtocolMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_protocol: Option<Protocol>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub default_capabilities: BTreeMap<String, CapabilitySupport>,
    pub connection_test: ConnectionTestTemplate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SourceProtocolCapability {
    pub mode: SourceProtocolMode,
    #[serde(default)]
    pub source_protocol: Option<Protocol>,
    #[serde(default)]
    pub adapter: Option<String>,
    #[serde(default)]
    pub features: BTreeMap<String, CapabilitySupport>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiscoveryParser {
    pub list_path: String,
    pub id_path: String,
    #[serde(default)]
    pub metadata_paths: BTreeMap<MetadataField, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "support", rename_all = "snake_case")]
pub enum DiscoveryPreset {
    Supported {
        method: HttpMethod,
        endpoint: String,
        parser: DiscoveryParser,
    },
    Unsupported {
        reason: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProviderPresetDefinition {
    pub schema_version: u32,
    pub default_base_url: String,
    pub credential_header: CredentialHeaderTemplate,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    pub protocols: BTreeMap<Protocol, ProtocolPreset>,
    pub discovery: DiscoveryPreset,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetDiffKind {
    Added,
    Changed,
    Missing,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PresetDiffEntry {
    pub path: String,
    pub kind: PresetDiffKind,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProviderPresetDiff {
    pub source_id: String,
    pub provider_preset_id: String,
    pub source_version: i32,
    pub latest_version: i32,
    pub changes: Vec<PresetDiffEntry>,
}

pub fn provider_preset_diff(
    source: &SourceRecord,
    latest: &ProviderPresetRecord,
) -> ProviderPresetDiff {
    let mut changes = Vec::new();
    diff_json(
        "$",
        Some(&source.provider_preset_snapshot),
        Some(&latest.definition),
        &mut changes,
    );
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    ProviderPresetDiff {
        source_id: source.id.clone(),
        provider_preset_id: source.provider_preset_id.clone(),
        source_version: source.provider_preset_version,
        latest_version: latest.version,
        changes,
    }
}

fn diff_json(
    path: &str,
    before: Option<&Value>,
    after: Option<&Value>,
    changes: &mut Vec<PresetDiffEntry>,
) {
    match (before, after) {
        (Some(Value::Object(before)), Some(Value::Object(after))) => {
            let keys = before
                .keys()
                .chain(after.keys())
                .collect::<std::collections::BTreeSet<_>>();
            for key in keys {
                diff_json(
                    &format!("{path}.{key}"),
                    before.get(key),
                    after.get(key),
                    changes,
                );
            }
        }
        (Some(before), Some(after)) if before == after => {}
        (Some(before), Some(after)) => changes.push(PresetDiffEntry {
            path: path.into(),
            kind: PresetDiffKind::Changed,
            before: Some(before.clone()),
            after: Some(after.clone()),
        }),
        (None, Some(after)) => changes.push(PresetDiffEntry {
            path: path.into(),
            kind: PresetDiffKind::Added,
            before: None,
            after: Some(after.clone()),
        }),
        (Some(before), None) => changes.push(PresetDiffEntry {
            path: path.into(),
            kind: PresetDiffKind::Missing,
            before: Some(before.clone()),
            after: None,
        }),
        (None, None) => {}
    }
}

impl ProviderPresetDefinition {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.schema_version != PROVIDER_PRESET_SCHEMA_VERSION {
            return Err(CatalogError::InvalidState(format!(
                "unsupported provider preset schema version {}",
                self.schema_version
            )));
        }
        let base_url = reqwest::Url::parse(&self.default_base_url).map_err(|_| {
            CatalogError::InvalidState("provider preset default_base_url is invalid".into())
        })?;
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(CatalogError::InvalidState(
                "provider preset default_base_url must be an http(s) origin without credentials, query, or fragment"
                    .into(),
            ));
        }
        if self.credential_header.header.trim().is_empty() {
            return Err(CatalogError::InvalidState(
                "provider preset credential header cannot be empty".into(),
            ));
        }
        for protocol in [
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ] {
            let definition = self.protocols.get(&protocol).ok_or_else(|| {
                CatalogError::InvalidState(format!(
                    "provider preset must declare protocol {protocol}"
                ))
            })?;
            validate_relative_endpoint(&definition.endpoint)?;
            if !definition.connection_test.body.is_object()
                || definition.connection_test.default_model.trim().is_empty()
            {
                return Err(CatalogError::InvalidState(format!(
                    "provider preset protocol {protocol} requires an object test body and default model"
                )));
            }
            match definition.mode {
                SourceProtocolMode::Adapter => {
                    if definition.source_protocol.is_none()
                        || definition.adapter.as_deref().is_none_or(str::is_empty)
                    {
                        return Err(CatalogError::InvalidState(format!(
                            "provider preset protocol {protocol} adapter requires source_protocol and adapter"
                        )));
                    }
                }
                SourceProtocolMode::Native
                | SourceProtocolMode::Unsupported
                | SourceProtocolMode::Unknown => {
                    if definition.source_protocol.is_some() || definition.adapter.is_some() {
                        return Err(CatalogError::InvalidState(format!(
                            "provider preset protocol {protocol} may only set adapter fields in adapter mode"
                        )));
                    }
                }
            }
        }
        if let DiscoveryPreset::Supported {
            method,
            endpoint,
            parser,
        } = &self.discovery
        {
            if *method != HttpMethod::Get {
                return Err(CatalogError::InvalidState(
                    "model discovery currently requires GET".into(),
                ));
            }
            validate_relative_endpoint(endpoint)?;
            if parser.list_path.trim().is_empty() || parser.id_path.trim().is_empty() {
                return Err(CatalogError::InvalidState(
                    "model discovery parser paths cannot be empty".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn auth_snapshot(&self) -> Value {
        serde_json::to_value(SourceAuthConfig {
            credential_header: self.credential_header.clone(),
            default_headers: self.default_headers.clone(),
        })
        .expect("provider authentication snapshot serializes")
    }

    pub fn protocol_capabilities_snapshot(&self) -> Value {
        let capabilities = self
            .protocols
            .iter()
            .map(|(protocol, definition)| {
                (
                    *protocol,
                    SourceProtocolCapability {
                        mode: definition.mode,
                        source_protocol: definition.source_protocol,
                        adapter: definition.adapter.clone(),
                        features: definition.default_capabilities.clone(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        serde_json::to_value(capabilities).expect("provider capabilities serialize")
    }
}

fn validate_relative_endpoint(endpoint: &str) -> Result<(), CatalogError> {
    if !endpoint.starts_with('/') || endpoint.starts_with("//") {
        return Err(CatalogError::InvalidState(format!(
            "provider preset endpoint '{endpoint}' must be an absolute path"
        )));
    }
    Ok(())
}

pub async fn install_builtin_presets(
    repository: &ModelCatalogRepository,
) -> Result<(), CatalogError> {
    for preset in builtin_provider_presets()? {
        repository.insert_provider_preset(&preset).await?;
    }
    for preset in builtin_model_presets()? {
        repository.insert_model_preset(&preset).await?;
    }
    Ok(())
}

pub fn builtin_provider_presets() -> Result<Vec<ProviderPresetInput>, CatalogError> {
    let (deepseek_id, deepseek_name, deepseek_definition) = deepseek();
    let (minimax_id, minimax_name, minimax_definition) = minimax();
    let (kimi_id, kimi_name, kimi_definition) = kimi_code();

    let mut deepseek_v2 = deepseek_definition.clone();
    mark_protocol_feature(
        &mut deepseek_v2,
        Protocol::OpenAiResponses,
        "web_search",
        CapabilitySupport::Supported,
    );
    let mut minimax_v2 = minimax_definition.clone();
    mark_protocol_feature(
        &mut minimax_v2,
        Protocol::OpenAiResponses,
        "web_search",
        CapabilitySupport::Supported,
    );
    let mut kimi_v2 = kimi_definition.clone();
    // The embedded Responses adapter translates streamed tool calls.  Keep
    // this fact in the Source capability snapshot so the runtime matrix does
    // not report the feature as an implicit unsupported value.
    mark_protocol_feature(
        &mut kimi_v2,
        Protocol::OpenAiResponses,
        "tool_streaming",
        CapabilitySupport::Supported,
    );

    [
        (deepseek_id, deepseek_name, 1, deepseek_definition),
        (minimax_id, minimax_name, 1, minimax_definition),
        (kimi_id, kimi_name, 1, kimi_definition),
        (
            deepseek_id,
            deepseek_name,
            BUILTIN_PROVIDER_PRESET_VERSION,
            deepseek_v2,
        ),
        (
            minimax_id,
            minimax_name,
            BUILTIN_PROVIDER_PRESET_VERSION,
            minimax_v2,
        ),
        (kimi_id, kimi_name, BUILTIN_PROVIDER_PRESET_VERSION, kimi_v2),
    ]
    .into_iter()
    .map(|(id, display_name, version, definition)| {
        definition.validate()?;
        Ok(ProviderPresetInput {
            id: id.into(),
            version,
            display_name: display_name.into(),
            definition: serde_json::to_value(definition)?,
        })
    })
    .collect()
}

fn mark_protocol_feature(
    definition: &mut ProviderPresetDefinition,
    protocol: Protocol,
    feature: &str,
    support: CapabilitySupport,
) {
    if let Some(protocol_definition) = definition.protocols.get_mut(&protocol) {
        protocol_definition
            .default_capabilities
            .insert(feature.to_owned(), support);
    }
}

pub fn builtin_model_presets() -> Result<Vec<ModelPresetInput>, CatalogError> {
    Ok(vec![
        model_preset(
            "deepseek-v4-flash",
            "deepseek-v4-flash",
            vec![],
            [
                (MetadataField::DisplayName, json!("DeepSeek V4 Flash")),
                (MetadataField::ContextWindow, json!(1_000_000)),
                (MetadataField::MaxOutputTokens, json!(384_000)),
                (MetadataField::InputModalities, json!(["text"])),
                (MetadataField::OutputModalities, json!(["text"])),
                (MetadataField::Tools, json!("supported")),
                (MetadataField::Thinking, json!("supported")),
                (MetadataField::StructuredOutput, json!("supported")),
                (MetadataField::Streaming, json!("supported")),
                (MetadataField::Usage, json!("supported")),
            ],
        )?,
        model_preset(
            "deepseek-v4-pro",
            "deepseek-v4-pro",
            vec![],
            [
                (MetadataField::DisplayName, json!("DeepSeek V4 Pro")),
                (MetadataField::ContextWindow, json!(1_000_000)),
                (MetadataField::MaxOutputTokens, json!(384_000)),
                (MetadataField::InputModalities, json!(["text"])),
                (MetadataField::OutputModalities, json!(["text"])),
                (MetadataField::Tools, json!("supported")),
                (MetadataField::Thinking, json!("supported")),
                (MetadataField::StructuredOutput, json!("supported")),
                (MetadataField::Streaming, json!("supported")),
                (MetadataField::Usage, json!("supported")),
            ],
        )?,
        model_preset(
            "minimax-m3",
            "MiniMax-M3",
            vec![],
            [
                (MetadataField::DisplayName, json!("MiniMax M3")),
                (MetadataField::ContextWindow, json!(1_000_000)),
                (
                    MetadataField::InputModalities,
                    json!(["text", "image", "video"]),
                ),
                (MetadataField::OutputModalities, json!(["text"])),
                (MetadataField::Tools, json!("supported")),
                (MetadataField::Thinking, json!("supported")),
                (MetadataField::Streaming, json!("supported")),
                (MetadataField::Usage, json!("supported")),
            ],
        )?,
        model_preset(
            "minimax-m2.7",
            "MiniMax-M2.7",
            vec!["MiniMax-M2.7-highspeed".into()],
            [
                (MetadataField::DisplayName, json!("MiniMax M2.7")),
                (MetadataField::ContextWindow, json!(204_800)),
                (MetadataField::InputModalities, json!(["text"])),
                (MetadataField::OutputModalities, json!(["text"])),
                (MetadataField::Tools, json!("supported")),
                (MetadataField::Thinking, json!("supported")),
                (MetadataField::Streaming, json!("supported")),
                (MetadataField::Usage, json!("supported")),
            ],
        )?,
        model_preset(
            "kimi-k3",
            "k3",
            vec![],
            [
                (MetadataField::DisplayName, json!("Kimi K3")),
                (MetadataField::ContextWindow, json!(1_000_000)),
                (MetadataField::InputModalities, json!(["text"])),
                (MetadataField::OutputModalities, json!(["text"])),
                (MetadataField::Tools, json!("supported")),
                (MetadataField::Thinking, json!("supported")),
                (MetadataField::WebSearch, json!("supported")),
                (MetadataField::Streaming, json!("supported")),
                (MetadataField::Usage, json!("supported")),
            ],
        )?,
        model_preset(
            "kimi-k3-256k",
            "k3-256k",
            vec![],
            [
                (MetadataField::DisplayName, json!("Kimi K3 256K")),
                (MetadataField::ContextWindow, json!(262_144)),
                (MetadataField::InputModalities, json!(["text"])),
                (MetadataField::OutputModalities, json!(["text"])),
                (MetadataField::Tools, json!("supported")),
                (MetadataField::Thinking, json!("supported")),
                (MetadataField::WebSearch, json!("supported")),
                (MetadataField::Streaming, json!("supported")),
                (MetadataField::Usage, json!("supported")),
            ],
        )?,
        model_preset(
            "kimi-for-coding",
            "kimi-for-coding",
            vec!["kimi-for-coding-highspeed".into()],
            [
                (MetadataField::DisplayName, json!("Kimi for Coding")),
                (MetadataField::ContextWindow, json!(262_144)),
                (MetadataField::InputModalities, json!(["text"])),
                (MetadataField::OutputModalities, json!(["text"])),
                (MetadataField::Tools, json!("supported")),
                (MetadataField::Thinking, json!("supported")),
                (MetadataField::WebSearch, json!("supported")),
                (MetadataField::Streaming, json!("supported")),
                (MetadataField::Usage, json!("supported")),
            ],
        )?,
    ])
}

fn model_preset<const N: usize>(
    id: &str,
    canonical_model_id: &str,
    aliases: Vec<String>,
    fields: [(MetadataField, Value); N],
) -> Result<ModelPresetInput, CatalogError> {
    let values = MetadataValues::from_fields(fields)?;
    Ok(ModelPresetInput {
        id: id.into(),
        version: 1,
        canonical_model_id: canonical_model_id.into(),
        aliases,
        metadata: CatalogMetadata::resolve(&MetadataValues::default(), Some(&values))?,
    })
}

fn default_features() -> BTreeMap<String, CapabilitySupport> {
    BTreeMap::from([
        ("streaming".into(), CapabilitySupport::Supported),
        ("tools".into(), CapabilitySupport::Supported),
        ("thinking".into(), CapabilitySupport::Supported),
        ("web_search".into(), CapabilitySupport::Unknown),
        ("structured_output".into(), CapabilitySupport::Unknown),
        ("usage".into(), CapabilitySupport::Supported),
    ])
}

fn bearer_auth() -> CredentialHeaderTemplate {
    CredentialHeaderTemplate {
        header: "authorization".into(),
        prefix: "Bearer ".into(),
    }
}

fn anthropic_headers() -> BTreeMap<String, String> {
    BTreeMap::from([("anthropic-version".into(), "2023-06-01".into())])
}

fn connection_test(default_model: &str, body: Value) -> ConnectionTestTemplate {
    ConnectionTestTemplate {
        method: HttpMethod::Post,
        default_model: default_model.into(),
        body,
    }
}

fn native_protocol(
    endpoint: &str,
    default_model: &str,
    body: Value,
    headers: BTreeMap<String, String>,
) -> ProtocolPreset {
    ProtocolPreset {
        endpoint: endpoint.into(),
        mode: SourceProtocolMode::Native,
        source_protocol: None,
        adapter: None,
        headers,
        default_capabilities: default_features(),
        connection_test: connection_test(default_model, body),
    }
}

fn openai_discovery(endpoint: &str) -> DiscoveryPreset {
    DiscoveryPreset::Supported {
        method: HttpMethod::Get,
        endpoint: endpoint.into(),
        parser: DiscoveryParser {
            list_path: "data".into(),
            id_path: "id".into(),
            metadata_paths: BTreeMap::from([
                (MetadataField::LogicalModelName, "id".into()),
                (MetadataField::DisplayName, "id".into()),
            ]),
        },
    }
}

fn deepseek() -> (&'static str, &'static str, ProviderPresetDefinition) {
    let model = "deepseek-v4-flash";
    (
        "deepseek",
        "DeepSeek",
        ProviderPresetDefinition {
            schema_version: PROVIDER_PRESET_SCHEMA_VERSION,
            default_base_url: "https://api.deepseek.com".into(),
            credential_header: bearer_auth(),
            default_headers: BTreeMap::from([
                ("accept".into(), "application/json".into()),
                ("content-type".into(), "application/json".into()),
            ]),
            protocols: BTreeMap::from([
                (
                    Protocol::OpenAiChatCompletions,
                    native_protocol(
                        "/chat/completions",
                        model,
                        json!({"model":"{{model}}","messages":[{"role":"user","content":"ping"}],"max_tokens":1,"stream":false}),
                        BTreeMap::new(),
                    ),
                ),
                (
                    Protocol::OpenAiResponses,
                    native_protocol(
                        "/responses",
                        model,
                        json!({"model":"{{model}}","input":"ping","max_output_tokens":1,"stream":false}),
                        BTreeMap::new(),
                    ),
                ),
                (
                    Protocol::AnthropicMessages,
                    native_protocol(
                        "/anthropic/v1/messages",
                        "deepseek-v4-pro",
                        json!({"model":"{{model}}","messages":[{"role":"user","content":"ping"}],"max_tokens":1,"stream":false}),
                        anthropic_headers(),
                    ),
                ),
            ]),
            discovery: openai_discovery("/models"),
        },
    )
}

fn minimax() -> (&'static str, &'static str, ProviderPresetDefinition) {
    let model = "MiniMax-M3";
    (
        "minimax",
        "MiniMax",
        ProviderPresetDefinition {
            schema_version: PROVIDER_PRESET_SCHEMA_VERSION,
            default_base_url: "https://api.minimax.io".into(),
            credential_header: bearer_auth(),
            default_headers: BTreeMap::from([
                ("accept".into(), "application/json".into()),
                ("content-type".into(), "application/json".into()),
            ]),
            protocols: BTreeMap::from([
                (
                    Protocol::OpenAiChatCompletions,
                    native_protocol(
                        "/v1/chat/completions",
                        model,
                        json!({"model":"{{model}}","messages":[{"role":"user","content":"ping"}],"max_tokens":1,"stream":false}),
                        BTreeMap::new(),
                    ),
                ),
                (
                    Protocol::OpenAiResponses,
                    native_protocol(
                        "/v1/responses",
                        model,
                        json!({"model":"{{model}}","input":"ping","max_output_tokens":1,"stream":false}),
                        BTreeMap::new(),
                    ),
                ),
                (
                    Protocol::AnthropicMessages,
                    native_protocol(
                        "/anthropic/v1/messages",
                        model,
                        json!({"model":"{{model}}","messages":[{"role":"user","content":"ping"}],"max_tokens":1,"stream":false}),
                        anthropic_headers(),
                    ),
                ),
            ]),
            discovery: openai_discovery("/v1/models"),
        },
    )
}

fn kimi_code() -> (&'static str, &'static str, ProviderPresetDefinition) {
    let model = "k3";
    let mut adapter_features = default_features();
    adapter_features.insert("web_search".into(), CapabilitySupport::Supported);
    (
        "kimi_code",
        "Kimi Code",
        ProviderPresetDefinition {
            schema_version: PROVIDER_PRESET_SCHEMA_VERSION,
            default_base_url: "https://api.kimi.com/coding".into(),
            credential_header: bearer_auth(),
            default_headers: BTreeMap::from([
                ("accept".into(), "application/json".into()),
                ("content-type".into(), "application/json".into()),
            ]),
            protocols: BTreeMap::from([
                (
                    Protocol::OpenAiChatCompletions,
                    native_protocol(
                        "/v1/chat/completions",
                        model,
                        json!({"model":"{{model}}","messages":[{"role":"user","content":"ping"}],"max_tokens":1,"stream":false}),
                        BTreeMap::new(),
                    ),
                ),
                (
                    Protocol::OpenAiResponses,
                    ProtocolPreset {
                        endpoint: "/v1/messages".into(),
                        mode: SourceProtocolMode::Adapter,
                        source_protocol: Some(Protocol::AnthropicMessages),
                        adapter: Some("kimi_responses_adapter".into()),
                        headers: anthropic_headers(),
                        default_capabilities: adapter_features,
                        connection_test: connection_test(
                            model,
                            json!({"model":"{{model}}","messages":[{"role":"user","content":"ping"}],"max_tokens":1,"stream":false}),
                        ),
                    },
                ),
                (
                    Protocol::AnthropicMessages,
                    native_protocol(
                        "/v1/messages",
                        model,
                        json!({"model":"{{model}}","messages":[{"role":"user","content":"ping"}],"max_tokens":1,"stream":false}),
                        anthropic_headers(),
                    ),
                ),
            ]),
            discovery: DiscoveryPreset::Unsupported {
                reason: "Kimi Code does not document an authenticated model-list endpoint; use the versioned ModelPreset catalog and user confirmation".into(),
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn builtins_are_versioned_complete_and_explicit_about_discovery() {
        let presets = builtin_provider_presets().expect("valid builtins");
        assert_eq!(presets.len(), 6);
        for preset in &presets {
            assert!(matches!(
                preset.version,
                1 | BUILTIN_PROVIDER_PRESET_VERSION
            ));
            let definition: ProviderPresetDefinition =
                serde_json::from_value(preset.definition.clone()).unwrap();
            definition.validate().unwrap();
            assert_eq!(definition.protocols.len(), 3);
        }
        let kimi = presets
            .iter()
            .find(|preset| preset.id == "kimi_code" && preset.version == 1)
            .unwrap();
        let definition: ProviderPresetDefinition =
            serde_json::from_value(kimi.definition.clone()).unwrap();
        assert!(matches!(
            definition.discovery,
            DiscoveryPreset::Unsupported { .. }
        ));
        assert_eq!(
            definition.protocols[&Protocol::OpenAiResponses].mode,
            SourceProtocolMode::Adapter
        );
        let kimi_v2 = presets
            .iter()
            .find(|preset| {
                preset.id == "kimi_code" && preset.version == BUILTIN_PROVIDER_PRESET_VERSION
            })
            .unwrap();
        let kimi_v2_definition: ProviderPresetDefinition =
            serde_json::from_value(kimi_v2.definition.clone()).unwrap();
        assert_eq!(
            kimi_v2_definition.protocols[&Protocol::OpenAiResponses].default_capabilities
                ["tool_streaming"],
            CapabilitySupport::Supported
        );
        for provider_id in ["deepseek", "minimax"] {
            let preset = presets
                .iter()
                .find(|preset| {
                    preset.id == provider_id && preset.version == BUILTIN_PROVIDER_PRESET_VERSION
                })
                .unwrap();
            let definition: ProviderPresetDefinition =
                serde_json::from_value(preset.definition.clone()).unwrap();
            assert_eq!(
                definition.protocols[&Protocol::OpenAiResponses].default_capabilities["web_search"],
                CapabilitySupport::Supported
            );
        }
    }

    #[test]
    fn model_presets_only_assert_documented_fields() {
        let presets = builtin_model_presets().expect("valid model presets");
        assert!(presets
            .iter()
            .any(|preset| preset.canonical_model_id == "deepseek-v4-flash"));
        assert!(presets
            .iter()
            .any(|preset| preset.canonical_model_id == "MiniMax-M3"));
        assert!(presets
            .iter()
            .any(|preset| preset.canonical_model_id == "k3"));
        for preset in presets {
            assert!(preset
                .metadata
                .field_sources
                .values()
                .all(|source| matches!(
                    source,
                    crate::model_catalog::MetadataSource::Preset
                        | crate::model_catalog::MetadataSource::Unknown
                )));
        }
    }

    #[test]
    fn preset_upgrade_diff_is_stable_and_never_mutates_the_source_snapshot() {
        let snapshot = json!({"default_base_url":"https://one.example","nested":{"a":1}});
        let source = SourceRecord {
            id: "source".into(),
            display_name: "Source".into(),
            provider_preset_id: "provider".into(),
            provider_preset_version: 1,
            provider_preset_snapshot: snapshot.clone(),
            base_url: "https://source.example".into(),
            endpoints: json!({}),
            auth_config: json!({}),
            protocol_capabilities: json!({}),
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let latest = ProviderPresetRecord {
            id: "provider".into(),
            version: 2,
            display_name: "Provider".into(),
            definition: json!({"default_base_url":"https://two.example","nested":{"b":2}}),
            created_at: Utc::now(),
        };
        let diff = provider_preset_diff(&source, &latest);
        assert_eq!(
            diff.changes
                .iter()
                .map(|change| change.path.as_str())
                .collect::<Vec<_>>(),
            vec!["$.default_base_url", "$.nested.a", "$.nested.b"]
        );
        assert_eq!(source.provider_preset_snapshot, snapshot);
    }
}
