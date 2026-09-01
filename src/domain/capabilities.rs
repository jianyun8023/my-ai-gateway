use super::{
    config::{Capabilities, GatewayConfig, ProtocolMode},
    protocol::Protocol,
    routing::{ResolvedBinding, ResolvedRoute, RouteResolutionError, RouteResolver},
};
use crate::control_plane::PublishedModel;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{collections::BTreeMap, error::Error, fmt};

pub const CAPABILITY_MATRIX_VERSION: &str = "v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityFactSource {
    RuntimeSnapshot,
}

#[derive(Clone, Debug, Serialize)]
pub struct CapabilityMatrixResponse {
    pub version: &'static str,
    pub fact_source: CapabilityFactSource,
    pub snapshot_revision: i64,
    pub snapshot_generated_at: DateTime<Utc>,
    pub data: Vec<RouteCapabilityMatrix>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RouteCapabilityMatrix {
    pub route_id: String,
    pub source: RuntimeSourceRef,
    pub account: RuntimeAccountRef,
    pub model: String,
    pub model_display_name: String,
    pub upstream_model_id: String,
    pub protocols: Vec<EffectiveProtocolCapability>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuntimeSourceRef {
    pub source_id: String,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuntimeAccountRef {
    pub account_id: String,
    pub display_name: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRouteStatus {
    Routable,
    Unroutable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityBindingSelection {
    Primary,
    Fallback,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolConversionHop {
    pub protocol_from: Protocol,
    pub protocol_to: Protocol,
    pub mode: ProtocolMode,
    pub adapter: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EffectiveProtocolCapability {
    pub protocol_in: Protocol,
    pub status: CapabilityRouteStatus,
    pub binding_id: Option<i64>,
    pub selection: Option<CapabilityBindingSelection>,
    pub selection_rank: Option<usize>,
    pub protocol_upstream: Option<Protocol>,
    pub endpoint: Option<String>,
    pub mode: Option<ProtocolMode>,
    pub adapter: Option<String>,
    pub conversion_chain: Vec<ProtocolConversionHop>,
    pub effective_capabilities: Capabilities,
    pub degraded: bool,
    pub degraded_features: Vec<String>,
    pub allow_lossy_conversion: Option<bool>,
    pub error: Option<RouteResolutionError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapabilityMatrixBuildError {
    NotRuntimeSnapshot,
    MissingRuntimeBindingId {
        route_id: String,
        protocol: Protocol,
    },
    InvalidRuntimeMode {
        route_id: String,
        binding_id: i64,
        mode: String,
    },
    DuplicateRuntimeBinding {
        route_id: String,
        binding_id: i64,
        protocol: Protocol,
    },
}

impl CapabilityMatrixBuildError {
    pub fn code(&self) -> &'static str {
        "invalid_runtime_snapshot"
    }
}

impl fmt::Display for CapabilityMatrixBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRuntimeSnapshot => {
                f.write_str("capability matrix requires the published runtime snapshot")
            }
            Self::MissingRuntimeBindingId { route_id, protocol } => write!(
                f,
                "runtime route '{route_id}' for {protocol} is missing its binding id"
            ),
            Self::InvalidRuntimeMode {
                route_id,
                binding_id,
                mode,
            } => write!(
                f,
                "runtime route '{route_id}' binding {binding_id} has invalid mode '{mode}'"
            ),
            Self::DuplicateRuntimeBinding {
                route_id,
                binding_id,
                protocol,
            } => write!(
                f,
                "runtime route '{route_id}' contains duplicate binding {binding_id} for {protocol}"
            ),
        }
    }
}

impl Error for CapabilityMatrixBuildError {}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct MatrixKey {
    model: String,
    route_id: String,
    source_id: String,
    account_id: String,
    upstream_model_id: String,
}

struct RouteMatrixBuilder {
    route_id: String,
    source: RuntimeSourceRef,
    account: RuntimeAccountRef,
    model: String,
    model_display_name: String,
    upstream_model_id: String,
    protocols: BTreeMap<Protocol, EffectiveProtocolCapability>,
}

impl CapabilityMatrixResponse {
    /// Build the effective matrix from the same immutable DB-backed snapshot
    /// used by proxy routing. `config` is only the transport/display metadata
    /// materialized inside that snapshot; configured routes are never read.
    pub fn from_runtime_snapshot(
        config: &GatewayConfig,
        resolver: &RouteResolver,
        models: &[PublishedModel],
        snapshot_revision: i64,
        snapshot_generated_at: DateTime<Utc>,
    ) -> Result<Self, CapabilityMatrixBuildError> {
        if !resolver.is_runtime_snapshot() {
            return Err(CapabilityMatrixBuildError::NotRuntimeSnapshot);
        }

        let mut rows = BTreeMap::<MatrixKey, RouteMatrixBuilder>::new();
        let mut resolutions = BTreeMap::<(String, Protocol), Option<RouteResolutionError>>::new();

        for model in models {
            for protocol in Protocol::ALL {
                match resolver.resolve_detailed(protocol, &model.id) {
                    Ok(resolved) => {
                        insert_resolved_route(config, model, &resolved, &mut rows)?;
                        resolutions.insert((model.id.clone(), protocol), None);
                    }
                    Err(error) => {
                        resolutions.insert((model.id.clone(), protocol), Some(error));
                    }
                }
            }
        }

        let data = rows
            .into_values()
            .map(|row| row.finish(&resolutions))
            .collect();
        Ok(Self {
            version: CAPABILITY_MATRIX_VERSION,
            fact_source: CapabilityFactSource::RuntimeSnapshot,
            snapshot_revision,
            snapshot_generated_at,
            data,
        })
    }
}

fn insert_resolved_route(
    config: &GatewayConfig,
    model: &PublishedModel,
    resolved: &ResolvedRoute,
    rows: &mut BTreeMap<MatrixKey, RouteMatrixBuilder>,
) -> Result<(), CapabilityMatrixBuildError> {
    let binding_id =
        resolved
            .binding_id
            .ok_or_else(|| CapabilityMatrixBuildError::MissingRuntimeBindingId {
                route_id: resolved.route_id.clone(),
                protocol: resolved.protocol_in,
            })?;
    insert_binding(
        config,
        model,
        &resolved.route_id,
        resolved.protocol_in,
        binding_id,
        &resolved.source_id,
        &resolved.primary_account_id,
        &resolved.upstream_model_id,
        resolved.protocol_upstream,
        &resolved.upstream_endpoint,
        &resolved.mode,
        resolved.adapter.as_deref(),
        &resolved.effective_capabilities,
        &resolved.degraded_features,
        resolved.allow_lossy_conversion,
        CapabilityBindingSelection::Primary,
        0,
        rows,
    )?;
    for (index, binding) in resolved.fallback_bindings.iter().enumerate() {
        insert_fallback_binding(config, model, resolved, binding, index + 1, rows)?;
    }
    Ok(())
}

fn insert_fallback_binding(
    config: &GatewayConfig,
    model: &PublishedModel,
    resolved: &ResolvedRoute,
    binding: &ResolvedBinding,
    selection_rank: usize,
    rows: &mut BTreeMap<MatrixKey, RouteMatrixBuilder>,
) -> Result<(), CapabilityMatrixBuildError> {
    insert_binding(
        config,
        model,
        &resolved.route_id,
        resolved.protocol_in,
        binding.binding_id,
        &binding.source_id,
        &binding.account_id,
        &binding.upstream_model_id,
        binding.protocol_upstream,
        &binding.upstream_endpoint,
        &binding.mode,
        binding.adapter.as_deref(),
        &binding.effective_capabilities,
        &binding.degraded_features,
        resolved.allow_lossy_conversion,
        CapabilityBindingSelection::Fallback,
        selection_rank,
        rows,
    )
}

#[allow(clippy::too_many_arguments)]
fn insert_binding(
    config: &GatewayConfig,
    model: &PublishedModel,
    route_id: &str,
    protocol_in: Protocol,
    binding_id: i64,
    source_id: &str,
    account_id: &str,
    upstream_model_id: &str,
    protocol_upstream: Protocol,
    endpoint: &str,
    mode: &str,
    adapter: Option<&str>,
    effective_capabilities: &Capabilities,
    degraded_features: &[String],
    allow_lossy_conversion: bool,
    selection: CapabilityBindingSelection,
    selection_rank: usize,
    rows: &mut BTreeMap<MatrixKey, RouteMatrixBuilder>,
) -> Result<(), CapabilityMatrixBuildError> {
    let mode = runtime_mode(route_id, binding_id, mode)?;
    let key = MatrixKey {
        model: model.id.clone(),
        route_id: route_id.to_owned(),
        source_id: source_id.to_owned(),
        account_id: account_id.to_owned(),
        upstream_model_id: upstream_model_id.to_owned(),
    };
    let row = rows.entry(key).or_insert_with(|| RouteMatrixBuilder {
        route_id: route_id.to_owned(),
        source: RuntimeSourceRef {
            source_id: source_id.to_owned(),
            display_name: config
                .provider(source_id)
                .map(|provider| provider.name.clone()),
        },
        account: RuntimeAccountRef {
            account_id: account_id.to_owned(),
            display_name: config
                .account(account_id)
                .map(|account| account.display_name.clone()),
            enabled: config.account(account_id).map(|account| account.enabled),
        },
        model: model.id.clone(),
        model_display_name: model.display_name.clone(),
        upstream_model_id: upstream_model_id.to_owned(),
        protocols: BTreeMap::new(),
    });
    let cell = EffectiveProtocolCapability::routable(
        protocol_in,
        binding_id,
        selection,
        selection_rank,
        protocol_upstream,
        endpoint,
        mode,
        adapter,
        effective_capabilities,
        degraded_features,
        allow_lossy_conversion,
    );
    if row.protocols.insert(protocol_in, cell).is_some() {
        return Err(CapabilityMatrixBuildError::DuplicateRuntimeBinding {
            route_id: route_id.to_owned(),
            binding_id,
            protocol: protocol_in,
        });
    }
    Ok(())
}

fn runtime_mode(
    route_id: &str,
    binding_id: i64,
    mode: &str,
) -> Result<ProtocolMode, CapabilityMatrixBuildError> {
    match mode {
        "native" => Ok(ProtocolMode::Native),
        "adapter" => Ok(ProtocolMode::Adapter),
        _ => Err(CapabilityMatrixBuildError::InvalidRuntimeMode {
            route_id: route_id.to_owned(),
            binding_id,
            mode: mode.to_owned(),
        }),
    }
}

impl RouteMatrixBuilder {
    fn finish(
        mut self,
        resolutions: &BTreeMap<(String, Protocol), Option<RouteResolutionError>>,
    ) -> RouteCapabilityMatrix {
        let protocols = Protocol::ALL
            .into_iter()
            .map(|protocol| {
                self.protocols.remove(&protocol).unwrap_or_else(|| {
                    let error = match resolutions.get(&(self.model.clone(), protocol)) {
                        Some(Some(error)) => error.clone(),
                        Some(None) | None => RouteResolutionError {
                            code: "runtime_binding_not_available".to_owned(),
                            message: format!(
                                "runtime snapshot has no confirmed, available binding for source '{}' account '{}' upstream model '{}' on {protocol}",
                                self.source.source_id,
                                self.account.account_id,
                                self.upstream_model_id
                            ),
                            route_id: Some(self.route_id.clone()),
                        },
                    };
                    EffectiveProtocolCapability::unroutable(protocol, error)
                })
            })
            .collect();
        RouteCapabilityMatrix {
            route_id: self.route_id,
            source: self.source,
            account: self.account,
            model: self.model,
            model_display_name: self.model_display_name,
            upstream_model_id: self.upstream_model_id,
            protocols,
        }
    }
}

impl EffectiveProtocolCapability {
    #[allow(clippy::too_many_arguments)]
    fn routable(
        protocol_in: Protocol,
        binding_id: i64,
        selection: CapabilityBindingSelection,
        selection_rank: usize,
        protocol_upstream: Protocol,
        endpoint: &str,
        mode: ProtocolMode,
        adapter: Option<&str>,
        effective_capabilities: &Capabilities,
        degraded_features: &[String],
        allow_lossy_conversion: bool,
    ) -> Self {
        let adapter = adapter.map(str::to_owned);
        Self {
            protocol_in,
            status: CapabilityRouteStatus::Routable,
            binding_id: Some(binding_id),
            selection: Some(selection),
            selection_rank: Some(selection_rank),
            protocol_upstream: Some(protocol_upstream),
            endpoint: Some(endpoint.to_owned()),
            mode: Some(mode),
            adapter: adapter.clone(),
            conversion_chain: vec![ProtocolConversionHop {
                protocol_from: protocol_in,
                protocol_to: protocol_upstream,
                mode,
                adapter,
            }],
            effective_capabilities: effective_capabilities.clone(),
            degraded: !degraded_features.is_empty(),
            degraded_features: degraded_features.to_vec(),
            allow_lossy_conversion: Some(allow_lossy_conversion),
            error: None,
        }
    }

    fn unroutable(protocol_in: Protocol, error: RouteResolutionError) -> Self {
        Self {
            protocol_in,
            status: CapabilityRouteStatus::Unroutable,
            binding_id: None,
            selection: None,
            selection_rank: None,
            protocol_upstream: None,
            endpoint: None,
            mode: None,
            adapter: None,
            conversion_chain: Vec::new(),
            effective_capabilities: Capabilities::default(),
            degraded: false,
            degraded_features: Vec::new(),
            allow_lossy_conversion: None,
            error: Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        config::{AccountConfig, CapabilityMode, ProviderConfig},
        routing::{RuntimeBinding, RuntimeRoute},
    };
    use std::{collections::HashMap, sync::Arc};

    fn provider(id: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.into(),
            name: format!("{id} display"),
            base_url: format!("https://{id}.example.test"),
            models: vec!["upstream-a".into(), "upstream-partial".into()],
            native_protocols: Vec::new(),
            endpoints: HashMap::new(),
            capabilities: Capabilities::default(),
            protocol_capabilities: HashMap::new(),
            model_overrides: HashMap::new(),
        }
    }

    fn account(id: &str, source_id: &str) -> AccountConfig {
        AccountConfig {
            id: id.into(),
            provider_id: source_id.into(),
            display_name: format!("{id} display"),
            credential_env: Some("CAPABILITY_TEST_SECRET_ENV".into()),
            credential_ciphertext: None,
            credential: Some("capability-test-secret-value".into()),
            enabled: true,
            weight: 100,
            protocol_capabilities: HashMap::new(),
            capabilities: None,
            model_overrides: HashMap::new(),
            model_map: HashMap::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn binding(
        binding_id: i64,
        source_id: &str,
        account_id: &str,
        upstream_model_id: &str,
        protocol_upstream: Protocol,
        mode: &str,
        adapter: Option<&str>,
        effective_capabilities: Capabilities,
        degraded_features: Vec<&str>,
    ) -> RuntimeBinding {
        RuntimeBinding {
            binding_id,
            source_id: source_id.into(),
            provider_id: source_id.into(),
            account_id: account_id.into(),
            upstream_model_id: upstream_model_id.into(),
            protocol_upstream,
            upstream_endpoint: format!("https://{source_id}.example.test/{}", protocol_upstream),
            mode: mode.into(),
            adapter: adapter.map(str::to_owned),
            effective_capabilities,
            degraded_features: degraded_features.into_iter().map(str::to_owned).collect(),
        }
    }

    fn runtime_fixture() -> (
        Arc<GatewayConfig>,
        RouteResolver,
        Vec<PublishedModel>,
        DateTime<Utc>,
    ) {
        let config = Arc::new(GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: vec![provider("source-a"), provider("source-b")],
            accounts: vec![
                account("account-a", "source-a"),
                account("account-b", "source-b"),
            ],
            routes: Vec::new(),
        });
        let translated = Capabilities {
            streaming: CapabilityMode::Translated,
            tools: CapabilityMode::Translated,
            tool_streaming: CapabilityMode::Translated,
            thinking: CapabilityMode::Translated,
            web_search: CapabilityMode::Translated,
            file_search: CapabilityMode::Unsupported,
            vision: CapabilityMode::Translated,
            usage: CapabilityMode::Translated,
        };
        let routes = vec![
            RuntimeRoute {
                route_id: "route-a".into(),
                model: "logical-a".into(),
                protocol: Protocol::OpenAiChatCompletions,
                allow_lossy_conversion: false,
                bindings: vec![
                    binding(
                        1,
                        "source-a",
                        "account-a",
                        "upstream-a",
                        Protocol::OpenAiChatCompletions,
                        "native",
                        None,
                        Capabilities::native(),
                        Vec::new(),
                    ),
                    binding(
                        4,
                        "source-b",
                        "account-b",
                        "upstream-a",
                        Protocol::OpenAiChatCompletions,
                        "native",
                        None,
                        Capabilities::native(),
                        Vec::new(),
                    ),
                ],
            },
            RuntimeRoute {
                route_id: "route-a".into(),
                model: "logical-a".into(),
                protocol: Protocol::OpenAiResponses,
                allow_lossy_conversion: true,
                bindings: vec![binding(
                    2,
                    "source-a",
                    "account-a",
                    "upstream-a",
                    Protocol::AnthropicMessages,
                    "adapter",
                    Some("kimi_responses_adapter"),
                    translated,
                    vec!["file_search"],
                )],
            },
            RuntimeRoute {
                route_id: "route-a".into(),
                model: "logical-a".into(),
                protocol: Protocol::AnthropicMessages,
                allow_lossy_conversion: false,
                bindings: vec![binding(
                    3,
                    "source-a",
                    "account-a",
                    "upstream-a",
                    Protocol::AnthropicMessages,
                    "native",
                    None,
                    Capabilities::native(),
                    Vec::new(),
                )],
            },
            RuntimeRoute {
                route_id: "route-partial".into(),
                model: "logical-partial".into(),
                protocol: Protocol::OpenAiChatCompletions,
                allow_lossy_conversion: false,
                bindings: vec![binding(
                    5,
                    "source-a",
                    "account-a",
                    "upstream-partial",
                    Protocol::OpenAiChatCompletions,
                    "native",
                    None,
                    Capabilities::native(),
                    Vec::new(),
                )],
            },
        ];
        let resolver = RouteResolver::from_runtime(config.clone(), routes);
        let models = vec![
            PublishedModel {
                id: "logical-a".into(),
                display_name: "Logical A".into(),
                account_ids: vec!["account-a".into(), "account-b".into()],
            },
            PublishedModel {
                id: "logical-partial".into(),
                display_name: "Logical Partial".into(),
                account_ids: vec!["account-a".into()],
            },
        ];
        (config, resolver, models, Utc::now())
    }

    fn matrix_cell(
        row: &RouteCapabilityMatrix,
        protocol: Protocol,
    ) -> &EffectiveProtocolCapability {
        row.protocols
            .iter()
            .find(|cell| cell.protocol_in == protocol)
            .unwrap_or_else(|| panic!("missing {protocol} cell"))
    }

    #[test]
    fn runtime_matrix_is_typed_complete_and_secret_free() {
        let (config, resolver, models, generated_at) = runtime_fixture();
        let response = CapabilityMatrixResponse::from_runtime_snapshot(
            &config,
            &resolver,
            &models,
            42,
            generated_at,
        )
        .expect("build runtime capability matrix");

        assert_eq!(response.version, "v1");
        assert_eq!(response.fact_source, CapabilityFactSource::RuntimeSnapshot);
        assert_eq!(response.snapshot_revision, 42);
        assert_eq!(response.snapshot_generated_at, generated_at);
        assert!(response
            .data
            .iter()
            .all(|entry| entry.protocols.len() == Protocol::ALL.len()));
        assert!(response.data.iter().all(|entry| {
            entry
                .protocols
                .iter()
                .map(|cell| cell.protocol_in)
                .eq(Protocol::ALL)
        }));

        let primary = response
            .data
            .iter()
            .find(|entry| entry.model == "logical-a" && entry.source.source_id == "source-a")
            .expect("primary source matrix row");
        assert_eq!(primary.upstream_model_id, "upstream-a");
        let chat = matrix_cell(primary, Protocol::OpenAiChatCompletions);
        assert_eq!(chat.status, CapabilityRouteStatus::Routable);
        assert_eq!(chat.selection, Some(CapabilityBindingSelection::Primary));
        assert_eq!(chat.mode, Some(ProtocolMode::Native));
        assert_eq!(chat.protocol_upstream, Some(chat.protocol_in));
        assert_eq!(chat.conversion_chain.len(), 1);

        let responses = matrix_cell(primary, Protocol::OpenAiResponses);
        assert_eq!(responses.status, CapabilityRouteStatus::Routable);
        assert_eq!(responses.mode, Some(ProtocolMode::Adapter));
        assert_eq!(
            responses.protocol_upstream,
            Some(Protocol::AnthropicMessages)
        );
        assert_eq!(responses.adapter.as_deref(), Some("kimi_responses_adapter"));
        assert_eq!(responses.conversion_chain.len(), 1);
        assert!(responses.degraded);
        assert_eq!(responses.degraded_features, vec!["file_search"]);
        assert_eq!(responses.allow_lossy_conversion, Some(true));

        let fallback = response
            .data
            .iter()
            .find(|entry| entry.model == "logical-a" && entry.source.source_id == "source-b")
            .expect("fallback source matrix row");
        let fallback_chat = matrix_cell(fallback, Protocol::OpenAiChatCompletions);
        assert_eq!(
            fallback_chat.selection,
            Some(CapabilityBindingSelection::Fallback)
        );
        assert_eq!(fallback_chat.selection_rank, Some(1));
        let fallback_responses = matrix_cell(fallback, Protocol::OpenAiResponses);
        assert_eq!(fallback_responses.status, CapabilityRouteStatus::Unroutable);
        assert_eq!(fallback_responses.mode, None);
        assert_eq!(fallback_responses.allow_lossy_conversion, None);
        assert_eq!(
            fallback_responses
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("runtime_binding_not_available")
        );

        let partial = response
            .data
            .iter()
            .find(|entry| entry.model == "logical-partial")
            .expect("partial runtime row");
        let unsupported = matrix_cell(partial, Protocol::OpenAiResponses);
        assert_eq!(unsupported.status, CapabilityRouteStatus::Unroutable);
        assert_eq!(unsupported.mode, None);
        assert_eq!(
            unsupported.error.as_ref().map(|error| error.code.as_str()),
            Some("route_not_found")
        );
        assert_eq!(unsupported.effective_capabilities, Capabilities::default());

        let serialized = serde_json::to_string(&response).expect("serialize capability matrix");
        for secret in [
            "CAPABILITY_TEST_SECRET_ENV",
            "capability-test-secret-value",
            "Authorization",
            "API Key",
        ] {
            assert!(
                !serialized.contains(secret),
                "capability matrix leaked {secret}: {serialized}"
            );
        }
    }

    #[test]
    fn config_resolver_cannot_masquerade_as_runtime_snapshot() {
        let (config, _, models, generated_at) = runtime_fixture();
        let resolver = RouteResolver::new(config.clone());
        let error = CapabilityMatrixResponse::from_runtime_snapshot(
            &config,
            &resolver,
            &models,
            1,
            generated_at,
        )
        .expect_err("config resolver must not back the runtime matrix");
        assert_eq!(error, CapabilityMatrixBuildError::NotRuntimeSnapshot);
    }
}
