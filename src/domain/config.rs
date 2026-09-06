use super::protocol::Protocol;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Features whose preservation can be declared by an adapter.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AdapterFeature {
    Streaming,
    Tools,
    ToolStreaming,
    Thinking,
    WebSearch,
    FileSearch,
    Vision,
    Usage,
}

/// A strongly typed declaration for a protocol adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterDefinition {
    pub name: &'static str,
    pub from_protocol: Protocol,
    pub to_protocol: Protocol,
    pub features: Capabilities,
}

#[allow(dead_code)]
pub type AdapterSpec = AdapterDefinition;
#[allow(dead_code)]
pub type AdapterRegistry = HashMap<&'static str, AdapterDefinition>;

impl AdapterDefinition {
    #[allow(dead_code)]
    pub fn feature(&self, feature: AdapterFeature) -> CapabilityMode {
        match feature {
            AdapterFeature::Streaming => self.features.streaming,
            AdapterFeature::Tools => self.features.tools,
            AdapterFeature::ToolStreaming => self.features.tool_streaming,
            AdapterFeature::Thinking => self.features.thinking,
            AdapterFeature::WebSearch => self.features.web_search,
            AdapterFeature::FileSearch => self.features.file_search,
            AdapterFeature::Vision => self.features.vision,
            AdapterFeature::Usage => self.features.usage,
        }
    }
}

/// Return the built-in adapter registry. A fresh map keeps the registry
/// immutable to callers while retaining a simple, strongly typed declaration.
///
/// No production adapters are registered: Kimi Code, the last adapter-backed
/// provider, serves OpenAI Responses natively since preset `kimi_code@4`
/// (issue #157), so any adapter declaration is rejected as unknown. Unit
/// tests keep a registered entry so the adapter framework (validation,
/// routing chains, degraded semantics) stays covered.
pub fn adapter_registry() -> AdapterRegistry {
    #[cfg(test)]
    {
        HashMap::from([(
            "kimi_responses_adapter",
            AdapterDefinition {
                name: "kimi_responses_adapter",
                // Direction is client/route ingress -> provider/upstream protocol.
                from_protocol: Protocol::OpenAiResponses,
                to_protocol: Protocol::AnthropicMessages,
                features: Capabilities {
                    streaming: CapabilityMode::Translated,
                    tools: CapabilityMode::Translated,
                    tool_streaming: CapabilityMode::Translated,
                    thinking: CapabilityMode::Translated,
                    web_search: CapabilityMode::Translated,
                    file_search: CapabilityMode::Unsupported,
                    vision: CapabilityMode::Translated,
                    usage: CapabilityMode::Translated,
                },
            },
        )])
    }
    #[cfg(not(test))]
    {
        HashMap::new()
    }
}

#[allow(dead_code)]
pub fn registered_adapters() -> AdapterRegistry {
    adapter_registry()
}

pub fn adapter_definition(name: &str) -> Option<AdapterDefinition> {
    adapter_registry().remove(name)
}

/// Protocol handling mode for an upstream source.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolMode {
    Native,
    Adapter,
    Unsupported,
}

/// A protocol capability declaration. Adapter declarations must include both
/// the protocol sent upstream and the adapter implementation name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolCapability {
    pub mode: ProtocolMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_protocol: Option<Protocol>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
}

impl<'de> Deserialize<'de> for ProtocolCapability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Mode(ProtocolMode),
            Fields {
                mode: ProtocolMode,
                #[serde(default)]
                source_protocol: Option<Protocol>,
                #[serde(default)]
                adapter: Option<String>,
            },
        }
        match Repr::deserialize(deserializer)? {
            Repr::Mode(mode) => Ok(Self {
                mode,
                source_protocol: None,
                adapter: None,
            }),
            Repr::Fields {
                mode,
                source_protocol,
                adapter,
            } => Ok(Self {
                mode,
                source_protocol,
                adapter,
            }),
        }
    }
}

impl ProtocolCapability {
    #[allow(dead_code)]
    pub fn native() -> Self {
        Self {
            mode: ProtocolMode::Native,
            source_protocol: None,
            adapter: None,
        }
    }
    #[allow(dead_code)]
    pub fn unsupported() -> Self {
        Self {
            mode: ProtocolMode::Unsupported,
            source_protocol: None,
            adapter: None,
        }
    }
    #[allow(dead_code)]
    pub fn adapter(source_protocol: Protocol, adapter: impl Into<String>) -> Self {
        Self {
            mode: ProtocolMode::Adapter,
            source_protocol: Some(source_protocol),
            adapter: Some(adapter.into()),
        }
    }

    /// Validate local invariants. Source resolution is checked by
    /// `ProtocolCapabilityMatrix::validate`.
    #[allow(dead_code)]
    pub fn validate(&self, target: Protocol) -> Result<(), String> {
        match self.mode {
            ProtocolMode::Native => {
                if self.source_protocol.is_some() || self.adapter.is_some() {
                    return Err(format!(
                        "{target}: native mode cannot set source_protocol or adapter"
                    ));
                }
            }
            ProtocolMode::Adapter => {
                let source = self
                    .source_protocol
                    .ok_or_else(|| format!("{target}: adapter mode requires source_protocol"))?;
                if source == target {
                    return Err(format!(
                        "{target}: adapter source_protocol must differ from target"
                    ));
                }
                match self.adapter.as_deref() {
                    Some(name) if !name.trim().is_empty() => {}
                    _ => return Err(format!("{target}: adapter mode requires adapter")),
                }
            }
            ProtocolMode::Unsupported => {
                if self.source_protocol.is_some() || self.adapter.is_some() {
                    return Err(format!(
                        "{target}: unsupported mode cannot set source_protocol or adapter"
                    ));
                }
            }
        }
        Ok(())
    }
}

pub type ProtocolCapabilityMatrix = HashMap<Protocol, ProtocolCapability>;
#[allow(dead_code)]
pub type ProtocolCapabilities = ProtocolCapabilityMatrix;

/// Feature support mode. `true`/`false` are accepted while reading legacy
/// configurations and map to native/unsupported respectively.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityMode {
    Native,
    Translated,
    #[default]
    Unsupported,
}

#[allow(dead_code)]
pub type FeatureCapability = CapabilityMode;
#[allow(dead_code)]
pub type FeatureCapabilities = Capabilities;
#[allow(dead_code)]
pub type CapabilityMatrix = Capabilities;
#[allow(dead_code)]
pub type CapabilitySupport = CapabilityMode;
#[allow(dead_code)]
pub type ProtocolCapabilityMode = ProtocolMode;
#[allow(dead_code)]
pub type ProtocolSupportMode = ProtocolMode;
#[allow(dead_code)]
pub type ModelConfig = ModelCapabilityOverride;
#[allow(dead_code)]
pub type SourceConfig = ProviderConfig;

impl<'de> Deserialize<'de> for CapabilityMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Bool(bool),
            String(String),
            Object { mode: String },
        }
        let value = Repr::deserialize(deserializer)?;
        let name = match value {
            Repr::Bool(true) => return Ok(Self::Native),
            Repr::Bool(false) => return Ok(Self::Unsupported),
            Repr::String(name) => name,
            Repr::Object { mode } => mode,
        };
        match name.as_str() {
            "native" => Ok(Self::Native),
            "translated" => Ok(Self::Translated),
            "unsupported" => Ok(Self::Unsupported),
            _ => Err(serde::de::Error::custom(format!(
                "unknown capability mode: {name}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Default)]
pub struct Capabilities {
    #[serde(default)]
    pub streaming: CapabilityMode,
    #[serde(default)]
    pub tools: CapabilityMode,
    #[serde(default)]
    pub tool_streaming: CapabilityMode,
    #[serde(default)]
    pub thinking: CapabilityMode,
    #[serde(default)]
    pub web_search: CapabilityMode,
    #[serde(default)]
    pub file_search: CapabilityMode,
    #[serde(default)]
    pub vision: CapabilityMode,
    #[serde(default)]
    pub usage: CapabilityMode,
}

impl Capabilities {
    #[allow(dead_code)]
    pub fn native() -> Self {
        Self {
            streaming: CapabilityMode::Native,
            tools: CapabilityMode::Native,
            tool_streaming: CapabilityMode::Native,
            thinking: CapabilityMode::Native,
            web_search: CapabilityMode::Native,
            file_search: CapabilityMode::Native,
            vision: CapabilityMode::Native,
            usage: CapabilityMode::Native,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct ModelCapabilityOverride {
    #[serde(default)]
    pub protocol_capabilities: ProtocolCapabilityMatrix,
    #[serde(default)]
    pub capabilities: Option<Capabilities>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub native_protocols: Vec<Protocol>,
    #[serde(default)]
    pub endpoints: HashMap<Protocol, String>,
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Explicit protocol matrix. Entries absent here fall back to the legacy
    /// `native_protocols`/`endpoints` fields.
    #[serde(default)]
    pub protocol_capabilities: ProtocolCapabilityMatrix,
    /// Per-model overrides of provider defaults.
    #[serde(default)]
    pub model_overrides: HashMap<String, ModelCapabilityOverride>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AccountConfig {
    pub id: String,
    pub provider_id: String,
    pub display_name: String,
    #[serde(default)]
    pub credential_env: Option<String>,
    /// Application-layer envelope.  This is retained in memory only as an
    /// opaque ciphertext and is never serialized into a runtime snapshot.
    #[serde(default, skip_serializing)]
    pub credential_ciphertext: Option<String>,
    #[serde(default, skip_serializing)]
    pub credential: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_weight")]
    pub weight: u32,
    #[serde(default)]
    pub protocol_capabilities: ProtocolCapabilityMatrix,
    #[serde(default)]
    pub capabilities: Option<Capabilities>,
    #[serde(default)]
    pub model_overrides: HashMap<String, ModelCapabilityOverride>,
    #[serde(default)]
    pub model_map: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RouteConfig {
    pub id: String,
    pub model: String,
    pub provider_id: String,
    #[serde(default)]
    pub protocols: Vec<Protocol>,
    pub primary_account_id: String,
    #[serde(default)]
    pub fallback_accounts: Vec<String>,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub adapter: Option<String>,
    #[serde(default)]
    pub allow_lossy_conversion: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GatewayConfig {
    pub listen_addr: String,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
}

impl GatewayConfig {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        for (provider_index, provider) in self.providers.iter().enumerate() {
            let provider_scope = format!("providers[{provider_index}] ({})", provider.id);
            if let Err(error) = validate_matrix(
                &provider.protocol_capabilities,
                &provider.endpoints,
                &provider.native_protocols,
                &provider_scope,
            ) {
                errors.push(error);
            }
            for (model, override_config) in &provider.model_overrides {
                if let Err(error) = validate_override_matrix(
                    &override_config.protocol_capabilities,
                    &provider.protocol_capabilities,
                    &provider.endpoints,
                    &provider.native_protocols,
                    &format!("{provider_scope}.model_overrides.{model}"),
                ) {
                    errors.push(error);
                }
            }
        }
        for (account_index, account) in self.accounts.iter().enumerate() {
            let account_scope = format!("accounts[{account_index}] ({})", account.id);
            let provider = self.provider(&account.provider_id);
            let endpoints = provider.map(|p| &p.endpoints).cloned().unwrap_or_default();
            let native = provider
                .map(|p| p.native_protocols.as_slice())
                .unwrap_or(&[]);
            let base_matrix = provider
                .map(|p| &p.protocol_capabilities)
                .cloned()
                .unwrap_or_default();
            if let Err(error) = validate_override_matrix(
                &account.protocol_capabilities,
                &base_matrix,
                &endpoints,
                native,
                &account_scope,
            ) {
                errors.push(error);
            }
            let mut account_base_matrix = base_matrix.clone();
            account_base_matrix.extend(
                account
                    .protocol_capabilities
                    .iter()
                    .map(|(protocol, capability)| (*protocol, capability.clone())),
            );
            for (model, override_config) in &account.model_overrides {
                if let Err(error) = validate_override_matrix(
                    &override_config.protocol_capabilities,
                    &account_base_matrix,
                    &endpoints,
                    native,
                    &format!("{account_scope}.model_overrides.{model}"),
                ) {
                    errors.push(error);
                }
            }
        }
        for (index, route) in self.routes.iter().enumerate() {
            let scope = format!("routes[{index}] ({})", route.id);
            let Some(provider) = self.provider(&route.provider_id) else {
                errors.push(format!(
                    "{scope}.provider_id: unknown provider {}",
                    route.provider_id
                ));
                continue;
            };
            if route.protocols.is_empty() {
                errors.push(format!(
                    "{scope}.protocols: must contain at least one protocol"
                ));
            }
            for (protocol_index, protocol) in route.protocols.iter().enumerate() {
                let protocol_scope = format!("{scope}.protocols[{protocol_index}] ({protocol})");
                match route.mode.as_str() {
                    "native" => {
                        let capability = self.protocol_capability(
                            &provider.id,
                            Some(&route.primary_account_id),
                            &route.model,
                            *protocol,
                        );
                        if capability.mode != ProtocolMode::Native {
                            errors.push(format!(
                                "{protocol_scope}: native route requires a native protocol capability"
                            ));
                        }
                    }
                    "adapter" => {
                        let Some(name) = route.adapter.as_deref() else {
                            errors.push(format!("{protocol_scope}.adapter: adapter route requires adapter"));
                            continue;
                        };
                        let Some(definition) = adapter_definition(name) else {
                            errors.push(format!(
                                "{protocol_scope}.adapter: unknown adapter '{name}'"
                            ));
                            continue;
                        };
                        if definition.from_protocol != *protocol {
                            errors.push(format!(
                                "{protocol_scope}.adapter: adapter '{name}' accepts {}, not {protocol}",
                                definition.from_protocol
                            ));
                        }
                    }
                    "unsupported" => {
                        if route.adapter.is_some() {
                            errors.push(format!(
                                "{protocol_scope}.adapter: unsupported route cannot set adapter"
                            ));
                        }
                    }
                    mode => errors.push(format!(
                        "{scope}.mode: unknown route mode '{mode}' (expected native, adapter, or unsupported)"
                    )),
                }
            }
            for (fb_index, fallback_id) in route.fallback_accounts.iter().enumerate() {
                let fb_scope = format!("{scope}.fallback_accounts[{fb_index}] ({fallback_id})");
                let Some(fb_account) = self.account(fallback_id) else {
                    errors.push(format!("{fb_scope}: unknown account"));
                    continue;
                };
                if fb_account.provider_id != provider.id {
                    if route.mode == "adapter" {
                        errors.push(format!(
                            "{fb_scope}: adapter route cannot have cross-provider fallback"
                        ));
                    }
                    let Some(fb_provider) = self.provider(&fb_account.provider_id) else {
                        errors.push(format!(
                            "{fb_scope}: unknown provider '{}'",
                            fb_account.provider_id
                        ));
                        continue;
                    };
                    for protocol in &route.protocols {
                        let fb_cap = self.protocol_capability(
                            &fb_provider.id,
                            Some(&fb_account.id),
                            &route.model,
                            *protocol,
                        );
                        if fb_cap.mode != ProtocolMode::Native {
                            errors.push(format!(
                                "{fb_scope}: cross-provider fallback requires native protocol {protocol} on provider '{}'",
                                fb_provider.id
                            ));
                        }
                    }
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Resolve the effective protocol declaration with precedence:
    /// account+model, provider+model, account, provider, legacy defaults.
    #[allow(dead_code)]
    pub fn protocol_capability(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
        model: &str,
        protocol: Protocol,
    ) -> ProtocolCapability {
        let provider = self.provider(provider_id);
        let account = account_id.and_then(|id| self.account(id));
        if let Some(account) = account {
            if let Some(value) = find_model_override(&account.model_overrides, model)
                .and_then(|m| m.protocol_capabilities.get(&protocol))
            {
                return value.clone();
            }
        }
        if let Some(provider) = provider {
            if let Some(value) = find_model_override(&provider.model_overrides, model)
                .and_then(|m| m.protocol_capabilities.get(&protocol))
            {
                return value.clone();
            }
        }
        if let Some(account) = account {
            if let Some(value) = account.protocol_capabilities.get(&protocol) {
                return value.clone();
            }
        }
        if let Some(provider) = provider {
            if let Some(value) = provider.protocol_capabilities.get(&protocol) {
                return value.clone();
            }
            if provider.native_protocols.contains(&protocol)
                && provider.endpoints.contains_key(&protocol)
            {
                return ProtocolCapability::native();
            }
        }
        ProtocolCapability::unsupported()
    }

    #[allow(dead_code)]
    pub fn effective_protocol_capabilities(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
        model: &str,
    ) -> ProtocolCapabilityMatrix {
        Protocol::ALL
            .into_iter()
            .map(|protocol| {
                (
                    protocol,
                    self.protocol_capability(provider_id, account_id, model, protocol),
                )
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn capabilities(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
        model: &str,
    ) -> Capabilities {
        let provider = self.provider(provider_id);
        let account = account_id.and_then(|id| self.account(id));
        if let Some(account) = account {
            if let Some(value) = find_model_override(&account.model_overrides, model)
                .and_then(|m| m.capabilities.as_ref())
            {
                return value.clone();
            }
        }
        if let Some(provider) = provider {
            if let Some(value) = find_model_override(&provider.model_overrides, model)
                .and_then(|m| m.capabilities.as_ref())
            {
                return value.clone();
            }
        }
        if let Some(account) = account {
            if let Some(value) = &account.capabilities {
                return value.clone();
            }
        }
        if let Some(provider) = provider {
            return provider.capabilities.clone();
        }
        Capabilities::default()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn models(&self) -> Vec<String> {
        let mut models = Vec::new();
        for model in self
            .providers
            .iter()
            .flat_map(|provider| provider.models.iter())
        {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }
        models
    }
    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }
    pub fn account(&self, id: &str) -> Option<&AccountConfig> {
        self.accounts.iter().find(|a| a.id == id)
    }
}

fn default_true() -> bool {
    true
}
fn default_weight() -> u32 {
    100
}
fn default_strategy() -> String {
    "primary_then_weighted_fallback".into()
}
fn default_mode() -> String {
    "native".into()
}

fn validate_matrix(
    matrix: &ProtocolCapabilityMatrix,
    endpoints: &HashMap<Protocol, String>,
    legacy_native: &[Protocol],
    scope: &str,
) -> Result<(), String> {
    for protocol in legacy_native {
        if !has_endpoint(endpoints, protocol) {
            return Err(format!(
                "{scope}.native_protocols.{protocol}: native protocol requires an endpoint"
            ));
        }
    }
    for (protocol, capability) in matrix {
        capability
            .validate(*protocol)
            .map_err(|error| format!("{scope}.protocol_capabilities.{protocol}: {error}"))?;
        if capability.mode == ProtocolMode::Native && !has_endpoint(endpoints, protocol) {
            return Err(format!(
                "{scope}.protocol_capabilities.{protocol}: native protocol requires an endpoint"
            ));
        }
        if capability.mode == ProtocolMode::Adapter {
            let source = capability
                .source_protocol
                .expect("validated adapter source");
            let name = capability
                .adapter
                .as_deref()
                .expect("validated adapter name");
            let Some(definition) = adapter_definition(name) else {
                return Err(format!(
                    "{scope}.protocol_capabilities.{protocol}.adapter: unknown adapter '{name}'"
                ));
            };
            if definition.from_protocol != *protocol || definition.to_protocol != source {
                return Err(format!(
                    "{scope}.protocol_capabilities.{protocol}: adapter '{name}' direction is {} -> {}, configured {} -> {}",
                    definition.from_protocol,
                    definition.to_protocol,
                    protocol,
                    source
                ));
            }
        }
    }
    // Adapter sources must be directly available. Chaining adapters would
    // create multi-segment conversion and is rejected (as are cycles).
    for (protocol, capability) in matrix {
        if capability.mode != ProtocolMode::Adapter {
            continue;
        }
        let source = capability
            .source_protocol
            .expect("validated adapter source");
        if let Some(source_capability) = matrix.get(&source) {
            match source_capability.mode {
                ProtocolMode::Native => {
                    if !has_endpoint(endpoints, &source) {
                        return Err(format!(
                            "{scope}.protocol_capabilities.{protocol}.source_protocol: native source {source} requires an endpoint"
                        ));
                    }
                }
                ProtocolMode::Adapter => {
                    return Err(format!(
                        "{scope}.protocol_capabilities.{protocol}.source_protocol: multi-segment adapter chain via {source} is not allowed"
                    ));
                }
                ProtocolMode::Unsupported => {
                    return Err(format!(
                        "{scope}.protocol_capabilities.{protocol}.source_protocol: source protocol {source} is unsupported"
                    ));
                }
            }
        } else if !has_endpoint(endpoints, &source) && !legacy_native.contains(&source) {
            return Err(format!(
                "{scope}.protocol_capabilities.{protocol}.source_protocol: source protocol {source} is unavailable"
            ));
        }
    }
    Ok(())
}

fn has_endpoint(endpoints: &HashMap<Protocol, String>, protocol: &Protocol) -> bool {
    endpoints
        .get(protocol)
        .is_some_and(|endpoint| !endpoint.trim().is_empty())
}

fn validate_override_matrix(
    matrix: &ProtocolCapabilityMatrix,
    base: &ProtocolCapabilityMatrix,
    endpoints: &HashMap<Protocol, String>,
    legacy_native: &[Protocol],
    scope: &str,
) -> Result<(), String> {
    validate_matrix(matrix, endpoints, legacy_native, scope)?;
    for (target, capability) in matrix {
        if capability.mode != ProtocolMode::Adapter {
            continue;
        }
        let source = capability
            .source_protocol
            .expect("validated adapter source");
        if let Some(base_capability) = base.get(&source) {
            match base_capability.mode {
                ProtocolMode::Unsupported => {
                    return Err(format!(
                        "{scope}.protocol_capabilities.{target}.source_protocol: source protocol {source} is unsupported in the base matrix"
                    ));
                }
                ProtocolMode::Adapter => {
                    return Err(format!(
                        "{scope}.protocol_capabilities.{target}.source_protocol: multi-segment adapter chain via {source} is not allowed"
                    ));
                }
                ProtocolMode::Native if !has_endpoint(endpoints, &source) => {
                    return Err(format!(
                        "{scope}.protocol_capabilities.{target}.source_protocol: native source {source} requires an endpoint"
                    ));
                }
                ProtocolMode::Native => {}
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn find_model_override<'a>(
    overrides: &'a HashMap<String, ModelCapabilityOverride>,
    model: &str,
) -> Option<&'a ModelCapabilityOverride> {
    overrides.get(model).or_else(|| {
        overrides
            .iter()
            .find(|(pattern, _)| {
                pattern.as_str() == "*"
                    || (pattern.ends_with('*') && model.starts_with(&pattern[..pattern.len() - 1]))
            })
            .map(|(_, value)| value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kimi_adapter_registry_declares_direction_and_feature_modes() {
        let adapter = adapter_definition("kimi_responses_adapter").unwrap();
        assert_eq!(adapter.from_protocol, Protocol::OpenAiResponses);
        assert_eq!(adapter.to_protocol, Protocol::AnthropicMessages);
        assert_eq!(
            adapter.feature(AdapterFeature::Thinking),
            CapabilityMode::Translated
        );
        assert_eq!(
            adapter.feature(AdapterFeature::FileSearch),
            CapabilityMode::Unsupported
        );
    }

    #[test]
    fn unknown_adapter_is_rejected_with_configuration_path() {
        let mut matrix = ProtocolCapabilityMatrix::new();
        matrix.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "does_not_exist"),
        );
        let error = validate_matrix(
            &matrix,
            &HashMap::from([(Protocol::AnthropicMessages, "/v1/messages".into())]),
            &[],
            "providers[0]",
        )
        .unwrap_err();
        assert!(error.contains("protocol_capabilities.openai_responses.adapter"));
        assert!(error.contains("unknown adapter"));
    }

    #[test]
    fn adapter_direction_mismatch_is_rejected() {
        let mut matrix = ProtocolCapabilityMatrix::new();
        matrix.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::OpenAiChatCompletions, "kimi_responses_adapter"),
        );
        let error = validate_matrix(
            &matrix,
            &HashMap::from([(
                Protocol::OpenAiChatCompletions,
                "/v1/chat/completions".into(),
            )]),
            &[],
            "providers[0]",
        )
        .unwrap_err();
        assert!(error.contains("direction"));
    }

    #[test]
    fn adapter_source_unavailable_is_rejected() {
        let mut matrix = ProtocolCapabilityMatrix::new();
        matrix.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "kimi_responses_adapter"),
        );
        let error = validate_matrix(&matrix, &HashMap::new(), &[], "providers[0]").unwrap_err();
        assert!(error.contains("source_protocol"));
        assert!(error.contains("unavailable"));
    }

    #[test]
    fn kimi_adapter_configuration_is_valid() {
        let mut matrix = ProtocolCapabilityMatrix::new();
        matrix.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "kimi_responses_adapter"),
        );
        matrix.insert(Protocol::AnthropicMessages, ProtocolCapability::native());
        assert!(validate_matrix(
            &matrix,
            &HashMap::from([(Protocol::AnthropicMessages, "/v1/messages".into())]),
            &[],
            "providers[0]",
        )
        .is_ok());
    }

    #[test]
    fn adapter_chain_is_rejected_as_multi_segment_conversion() {
        let mut matrix = ProtocolCapabilityMatrix::new();
        matrix.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "kimi_responses_adapter"),
        );
        matrix.insert(
            Protocol::AnthropicMessages,
            ProtocolCapability::adapter(Protocol::OpenAiChatCompletions, "kimi_responses_adapter"),
        );
        let error = validate_matrix(
            &matrix,
            &HashMap::from([(
                Protocol::OpenAiChatCompletions,
                "/v1/chat/completions".into(),
            )]),
            &[],
            "providers[0]",
        )
        .unwrap_err();
        assert!(error.contains("direction") || error.contains("multi-segment"));
    }

    #[test]
    fn native_protocol_requires_non_empty_endpoint() {
        let mut matrix = ProtocolCapabilityMatrix::new();
        matrix.insert(
            Protocol::OpenAiChatCompletions,
            ProtocolCapability::native(),
        );
        let error = validate_matrix(
            &matrix,
            &HashMap::from([(Protocol::OpenAiChatCompletions, " ".into())]),
            &[],
            "providers[0]",
        )
        .unwrap_err();
        assert!(error.contains("requires an endpoint"));
    }

    #[test]
    fn protocol_capability_modes_round_trip() {
        let json = r#"{
          "openai_chat_completions": {"mode":"native"},
          "openai_responses": {"mode":"adapter","source_protocol":"openai_chat_completions","adapter":"chat_to_responses"},
          "anthropic_messages": {"mode":"unsupported"}
        }"#;
        let matrix: ProtocolCapabilityMatrix = serde_json::from_str(json).unwrap();
        assert_eq!(
            matrix[&Protocol::OpenAiResponses].mode,
            ProtocolMode::Adapter
        );
        assert_eq!(
            serde_json::to_value(&matrix).unwrap()["openai_responses"]["adapter"],
            "chat_to_responses"
        );
        let shorthand: ProtocolCapability = serde_json::from_str("\"unsupported\"").unwrap();
        assert_eq!(shorthand.mode, ProtocolMode::Unsupported);
    }

    #[test]
    fn legacy_boolean_capabilities_are_accepted_and_new_modes_round_trip() {
        let capabilities: Capabilities =
            serde_json::from_str(r#"{"streaming":true,"tools":"translated","thinking":false}"#)
                .unwrap();
        assert_eq!(capabilities.streaming, CapabilityMode::Native);
        assert_eq!(capabilities.tools, CapabilityMode::Translated);
        assert_eq!(capabilities.thinking, CapabilityMode::Unsupported);
        let output = serde_json::to_value(&capabilities).unwrap();
        assert_eq!(output["tools"], "translated");
    }

    #[test]
    fn invalid_protocol_combinations_are_rejected() {
        let missing_adapter = ProtocolCapability {
            mode: ProtocolMode::Adapter,
            source_protocol: Some(Protocol::OpenAiChatCompletions),
            adapter: None,
        };
        assert!(missing_adapter.validate(Protocol::OpenAiResponses).is_err());
        let native_with_adapter = ProtocolCapability {
            mode: ProtocolMode::Native,
            source_protocol: None,
            adapter: Some("x".into()),
        };
        assert!(native_with_adapter
            .validate(Protocol::OpenAiChatCompletions)
            .is_err());
        let mut matrix = ProtocolCapabilityMatrix::new();
        matrix.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "a"),
        );
        matrix.insert(
            Protocol::AnthropicMessages,
            ProtocolCapability::unsupported(),
        );
        assert!(validate_matrix(&matrix, &HashMap::new(), &[], "test").is_err());
    }

    #[test]
    fn protocol_and_feature_overrides_follow_documented_precedence() {
        let config: GatewayConfig = serde_json::from_value(serde_json::json!({
            "listen_addr": "127.0.0.1:1",
            "providers": [{
                "id": "p",
                "name": "Provider",
                "base_url": "https://provider.example",
                "models": ["provider-model", "account-model", "plain-model"],
                "endpoints": {
                    "openai_responses": "/v1/responses",
                    "anthropic_messages": "/v1/messages"
                },
                "protocol_capabilities": {
                    "openai_responses": {
                        "mode": "adapter",
                        "source_protocol": "anthropic_messages",
                        "adapter": "kimi_responses_adapter"
                    },
                    "anthropic_messages": {"mode": "native"}
                },
                "capabilities": {"tools": "native"},
                "model_overrides": {
                    "provider-model": {
                        "protocol_capabilities": {
                            "openai_responses": {"mode": "native"}
                        },
                        "capabilities": {"tools": "translated"}
                    },
                    "account-model": {
                        "protocol_capabilities": {
                            "openai_responses": {"mode": "unsupported"}
                        },
                        "capabilities": {"tools": "translated"}
                    }
                }
            }],
            "accounts": [{
                "id": "a",
                "provider_id": "p",
                "display_name": "Account",
                "protocol_capabilities": {
                    "openai_responses": {"mode": "unsupported"}
                },
                "capabilities": {"tools": "unsupported"},
                "model_overrides": {
                    "account-model": {
                        "protocol_capabilities": {
                            "openai_responses": {"mode": "native"}
                        },
                        "capabilities": {"tools": "native"}
                    }
                }
            }],
            "routes": []
        }))
        .expect("precedence fixture");
        assert!(config.validate().is_ok());

        // Provider+Model beats the Account default for both matrices.
        assert_eq!(
            config
                .protocol_capability("p", Some("a"), "provider-model", Protocol::OpenAiResponses)
                .mode,
            ProtocolMode::Native
        );
        assert_eq!(
            config.capabilities("p", Some("a"), "provider-model").tools,
            CapabilityMode::Translated
        );

        // Account+Model remains the highest-priority declaration.
        assert_eq!(
            config
                .protocol_capability("p", Some("a"), "account-model", Protocol::OpenAiResponses)
                .mode,
            ProtocolMode::Native
        );
        assert_eq!(
            config.capabilities("p", Some("a"), "account-model").tools,
            CapabilityMode::Native
        );

        // Without a model override, the Account default beats the Provider default.
        assert_eq!(
            config
                .protocol_capability("p", Some("a"), "plain-model", Protocol::OpenAiResponses)
                .mode,
            ProtocolMode::Unsupported
        );
        assert_eq!(
            config.capabilities("p", Some("a"), "plain-model").tools,
            CapabilityMode::Unsupported
        );

        // Missing lookups retain the existing safe defaults.
        assert_eq!(
            config
                .protocol_capability(
                    "missing-provider",
                    Some("missing-account"),
                    "plain-model",
                    Protocol::OpenAiResponses,
                )
                .mode,
            ProtocolMode::Unsupported
        );
        assert_eq!(
            config.capabilities("missing-provider", Some("missing-account"), "plain-model"),
            Capabilities::default()
        );
    }

    #[test]
    fn shipped_example_is_valid() {
        let config: GatewayConfig =
            serde_json::from_str(include_str!("../../config.example.json")).unwrap();
        assert!(
            config.validate().is_ok(),
            "example config errors: {:?}",
            config.validate()
        );
        assert_eq!(
            config
                .protocol_capability(
                    "kimi_code",
                    Some("kimi-main"),
                    "kimi-for-coding-highspeed",
                    Protocol::OpenAiResponses
                )
                .mode,
            ProtocolMode::Native
        );
    }
}
