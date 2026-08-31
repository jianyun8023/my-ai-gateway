use crate::{
    config::{Capabilities, GatewayConfig, ProtocolMode, RouteConfig},
    protocol::Protocol,
    routing::{ResolvedRoute, RouteResolutionError, RouteResolver},
};
use serde::Serialize;
use std::sync::Arc;

pub const CAPABILITY_MATRIX_VERSION: &str = "v1";

#[derive(Clone, Debug, Serialize)]
pub struct CapabilityMatrixResponse {
    pub version: &'static str,
    pub data: Vec<RouteCapabilityMatrix>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RouteCapabilityMatrix {
    pub route_id: String,
    pub source: ConfigSourceRef,
    pub account: ConfigAccountRef,
    /// The configured route model expression. It may be an exact model or a
    /// wildcard pattern and is resolved with the same matching rules as proxy
    /// traffic.
    pub model: String,
    pub protocols: Vec<EffectiveProtocolCapability>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConfigSourceRef {
    pub provider_id: String,
    pub provider_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConfigAccountRef {
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
    pub protocol_upstream: Option<Protocol>,
    /// A resolved URL assembled only from the configured Provider base URL and
    /// protocol endpoint. No credential lookup is performed for this API.
    pub endpoint: Option<String>,
    pub mode: Option<ProtocolMode>,
    pub adapter: Option<String>,
    pub conversion_chain: Vec<ProtocolConversionHop>,
    pub effective_capabilities: Capabilities,
    pub degraded: bool,
    pub degraded_features: Vec<String>,
    pub allow_lossy_conversion: bool,
    pub error: Option<RouteResolutionError>,
}

impl CapabilityMatrixResponse {
    pub fn from_config(config: Arc<GatewayConfig>) -> Self {
        let resolver = RouteResolver::new(config.clone());
        let data = config
            .routes
            .iter()
            .enumerate()
            .map(|(route_index, route)| RouteCapabilityMatrix {
                route_id: route.id.clone(),
                source: ConfigSourceRef {
                    provider_id: route.provider_id.clone(),
                    provider_name: config
                        .provider(&route.provider_id)
                        .map(|provider| provider.name.clone()),
                },
                account: ConfigAccountRef {
                    account_id: route.primary_account_id.clone(),
                    display_name: config
                        .account(&route.primary_account_id)
                        .map(|account| account.display_name.clone()),
                    enabled: config
                        .account(&route.primary_account_id)
                        .map(|account| account.enabled),
                },
                model: route.model.clone(),
                protocols: Protocol::ALL
                    .into_iter()
                    .map(|protocol| {
                        EffectiveProtocolCapability::resolve(
                            &resolver,
                            route_index,
                            route,
                            protocol,
                        )
                    })
                    .collect(),
            })
            .collect();
        Self {
            version: CAPABILITY_MATRIX_VERSION,
            data,
        }
    }
}

impl EffectiveProtocolCapability {
    fn resolve(
        resolver: &RouteResolver,
        route_index: usize,
        route: &RouteConfig,
        protocol: Protocol,
    ) -> Self {
        match resolver.resolve_configured_route(route_index, protocol, &route.model) {
            Ok(resolved) => Self::routable(resolved),
            Err(error) => Self::unroutable(route, protocol, error),
        }
    }

    fn routable(resolved: ResolvedRoute) -> Self {
        let mode = match resolved.mode.as_str() {
            "native" => ProtocolMode::Native,
            "adapter" => ProtocolMode::Adapter,
            _ => unreachable!("RouteResolver only emits validated route modes"),
        };
        let conversion_chain = vec![ProtocolConversionHop {
            protocol_from: resolved.protocol_in,
            protocol_to: resolved.protocol_upstream,
            mode,
            adapter: resolved.adapter.clone(),
        }];
        let degraded = resolved.is_degraded();
        Self {
            protocol_in: resolved.protocol_in,
            status: CapabilityRouteStatus::Routable,
            protocol_upstream: Some(resolved.protocol_upstream),
            endpoint: Some(resolved.upstream_endpoint),
            mode: Some(mode),
            adapter: resolved.adapter,
            conversion_chain,
            effective_capabilities: resolved.effective_capabilities,
            degraded,
            degraded_features: resolved.degraded_features,
            allow_lossy_conversion: resolved.allow_lossy_conversion,
            error: None,
        }
    }

    fn unroutable(route: &RouteConfig, protocol: Protocol, error: RouteResolutionError) -> Self {
        let protocol_not_configured = error.code == "route_protocol_not_configured";
        let mode = if protocol_not_configured {
            Some(ProtocolMode::Unsupported)
        } else {
            configured_mode(route)
        };
        let adapter = (mode == Some(ProtocolMode::Adapter))
            .then(|| route.adapter.clone())
            .flatten();
        Self {
            protocol_in: protocol,
            status: CapabilityRouteStatus::Unroutable,
            protocol_upstream: None,
            endpoint: None,
            mode,
            adapter,
            conversion_chain: Vec::new(),
            effective_capabilities: Capabilities::default(),
            degraded: false,
            degraded_features: Vec::new(),
            allow_lossy_conversion: route.allow_lossy_conversion,
            error: Some(error),
        }
    }
}

fn configured_mode(route: &RouteConfig) -> Option<ProtocolMode> {
    match route.mode.as_str() {
        "native" => Some(ProtocolMode::Native),
        "adapter" => Some(ProtocolMode::Adapter),
        "unsupported" => Some(ProtocolMode::Unsupported),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AccountConfig, CapabilityMode, ModelCapabilityOverride, ProtocolCapability, ProviderConfig,
    };
    use std::collections::HashMap;

    fn provider(id: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.into(),
            name: format!("{id} name"),
            base_url: format!("https://{id}.example.test/api/"),
            models: vec!["model-a".into()],
            native_protocols: Vec::new(),
            endpoints: HashMap::new(),
            capabilities: Capabilities::native(),
            protocol_capabilities: HashMap::new(),
            model_overrides: HashMap::new(),
        }
    }

    fn account(id: &str, provider_id: &str) -> AccountConfig {
        AccountConfig {
            id: id.into(),
            provider_id: provider_id.into(),
            display_name: format!("{id} display"),
            credential_env: Some("CAPABILITY_TEST_SECRET_ENV".into()),
            credential: Some("capability-test-secret-value".into()),
            enabled: true,
            weight: 100,
            protocol_capabilities: HashMap::new(),
            capabilities: None,
            model_overrides: HashMap::new(),
            model_map: HashMap::new(),
        }
    }

    fn route(
        id: &str,
        model: &str,
        provider_id: &str,
        account_id: &str,
        protocol: Protocol,
        mode: &str,
    ) -> RouteConfig {
        RouteConfig {
            id: id.into(),
            model: model.into(),
            provider_id: provider_id.into(),
            protocols: vec![protocol],
            primary_account_id: account_id.into(),
            fallback_accounts: Vec::new(),
            strategy: "primary_then_weighted_fallback".into(),
            mode: mode.into(),
            adapter: None,
            allow_lossy_conversion: false,
        }
    }

    fn gateway(
        providers: Vec<ProviderConfig>,
        accounts: Vec<AccountConfig>,
        routes: Vec<RouteConfig>,
    ) -> GatewayConfig {
        GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers,
            accounts,
            routes,
        }
    }

    fn matrix_cell<'a>(
        response: &'a CapabilityMatrixResponse,
        route_id: &str,
        protocol: Protocol,
    ) -> &'a EffectiveProtocolCapability {
        response
            .data
            .iter()
            .find(|entry| entry.route_id == route_id)
            .unwrap_or_else(|| panic!("missing matrix entry for {route_id}"))
            .protocols
            .iter()
            .find(|cell| cell.protocol_in == protocol)
            .unwrap_or_else(|| panic!("missing {protocol} cell for {route_id}"))
    }

    #[test]
    fn three_protocol_matrix_is_typed_complete_and_secret_free() {
        let mut provider = provider("source-a");
        provider.endpoints = HashMap::from([
            (Protocol::OpenAiChatCompletions, "/v1/chat".into()),
            (Protocol::AnthropicMessages, "/v1/messages".into()),
        ]);
        provider.protocol_capabilities = HashMap::from([
            (
                Protocol::OpenAiChatCompletions,
                ProtocolCapability::native(),
            ),
            (
                Protocol::OpenAiResponses,
                ProtocolCapability::adapter(Protocol::AnthropicMessages, "kimi_responses_adapter"),
            ),
            (Protocol::AnthropicMessages, ProtocolCapability::native()),
        ]);
        provider.capabilities.file_search = CapabilityMode::Unsupported;

        let mut account = account("account-a", "source-a");
        account.model_overrides.insert(
            "model-a".into(),
            ModelCapabilityOverride {
                protocol_capabilities: HashMap::new(),
                capabilities: Some(Capabilities {
                    tools: CapabilityMode::Unsupported,
                    file_search: CapabilityMode::Unsupported,
                    ..Capabilities::native()
                }),
            },
        );

        let chat = route(
            "chat-native",
            "model-a",
            "source-a",
            "account-a",
            Protocol::OpenAiChatCompletions,
            "native",
        );
        let mut responses = route(
            "responses-adapter",
            "model-a",
            "source-a",
            "account-a",
            Protocol::OpenAiResponses,
            "adapter",
        );
        responses.adapter = Some("kimi_responses_adapter".into());
        let messages = route(
            "messages-unsupported",
            "model-a",
            "source-a",
            "account-a",
            Protocol::AnthropicMessages,
            "unsupported",
        );

        let response = CapabilityMatrixResponse::from_config(Arc::new(gateway(
            vec![provider],
            vec![account],
            vec![chat, responses, messages],
        )));

        assert_eq!(response.version, "v1");
        assert_eq!(response.data.len(), 3);
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

        let chat = matrix_cell(&response, "chat-native", Protocol::OpenAiChatCompletions);
        assert_eq!(chat.status, CapabilityRouteStatus::Routable);
        assert_eq!(chat.mode, Some(ProtocolMode::Native));
        assert_eq!(
            chat.protocol_upstream,
            Some(Protocol::OpenAiChatCompletions)
        );
        assert_eq!(
            chat.endpoint.as_deref(),
            Some("https://source-a.example.test/api/v1/chat")
        );
        assert_eq!(chat.conversion_chain.len(), 1);
        assert_eq!(chat.conversion_chain[0].protocol_from, chat.protocol_in);
        assert_eq!(chat.conversion_chain[0].protocol_to, chat.protocol_in);
        assert_eq!(
            chat.effective_capabilities.tools,
            CapabilityMode::Unsupported,
            "account+model capability override must be visible"
        );

        let responses = matrix_cell(&response, "responses-adapter", Protocol::OpenAiResponses);
        assert_eq!(responses.status, CapabilityRouteStatus::Routable);
        assert_eq!(responses.mode, Some(ProtocolMode::Adapter));
        assert_eq!(
            responses.protocol_upstream,
            Some(Protocol::AnthropicMessages)
        );
        assert_eq!(responses.adapter.as_deref(), Some("kimi_responses_adapter"));
        assert_eq!(responses.conversion_chain.len(), 1);
        assert_eq!(
            responses.conversion_chain[0],
            ProtocolConversionHop {
                protocol_from: Protocol::OpenAiResponses,
                protocol_to: Protocol::AnthropicMessages,
                mode: ProtocolMode::Adapter,
                adapter: Some("kimi_responses_adapter".into()),
            }
        );
        assert!(responses.degraded);
        assert!(responses.degraded_features.contains(&"streaming".into()));
        assert!(!responses.degraded_features.contains(&"file_search".into()));

        let unsupported = matrix_cell(
            &response,
            "messages-unsupported",
            Protocol::AnthropicMessages,
        );
        assert_eq!(unsupported.status, CapabilityRouteStatus::Unroutable);
        assert_eq!(unsupported.mode, Some(ProtocolMode::Unsupported));
        assert_eq!(
            unsupported.error.as_ref().map(|error| error.code.as_str()),
            Some("unsupported_protocol")
        );
        assert_eq!(unsupported.effective_capabilities, Capabilities::default());

        let unconfigured = matrix_cell(&response, "chat-native", Protocol::AnthropicMessages);
        assert_eq!(unconfigured.status, CapabilityRouteStatus::Unroutable);
        assert_eq!(unconfigured.mode, Some(ProtocolMode::Unsupported));
        assert_eq!(
            unconfigured.error.as_ref().map(|error| error.code.as_str()),
            Some("route_protocol_not_configured")
        );

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
    fn missing_endpoint_and_unknown_adapter_return_structured_errors() {
        let mut provider = provider("invalid-source");
        provider
            .endpoints
            .insert(Protocol::AnthropicMessages, "/v1/messages".into());
        provider.protocol_capabilities.insert(
            Protocol::OpenAiChatCompletions,
            ProtocolCapability::native(),
        );

        let missing_endpoint = route(
            "missing-endpoint",
            "missing-model",
            "invalid-source",
            "invalid-account",
            Protocol::OpenAiChatCompletions,
            "native",
        );
        let mut unknown_adapter = route(
            "unknown-adapter",
            "unknown-model",
            "invalid-source",
            "invalid-account",
            Protocol::OpenAiResponses,
            "adapter",
        );
        unknown_adapter.adapter = Some("does_not_exist".into());

        let response = CapabilityMatrixResponse::from_config(Arc::new(gateway(
            vec![provider],
            vec![account("invalid-account", "invalid-source")],
            vec![missing_endpoint, unknown_adapter],
        )));

        let missing = matrix_cell(
            &response,
            "missing-endpoint",
            Protocol::OpenAiChatCompletions,
        );
        assert_eq!(missing.status, CapabilityRouteStatus::Unroutable);
        assert_eq!(missing.mode, Some(ProtocolMode::Native));
        assert!(missing.endpoint.is_none());
        assert_eq!(
            missing.error.as_ref().map(|error| error.code.as_str()),
            Some("endpoint_missing")
        );

        let unknown = matrix_cell(&response, "unknown-adapter", Protocol::OpenAiResponses);
        assert_eq!(unknown.status, CapabilityRouteStatus::Unroutable);
        assert_eq!(unknown.mode, Some(ProtocolMode::Adapter));
        assert_eq!(unknown.adapter.as_deref(), Some("does_not_exist"));
        assert!(unknown.conversion_chain.is_empty());
        assert_eq!(
            unknown.error.as_ref().map(|error| error.code.as_str()),
            Some("adapter_unknown")
        );
    }

    #[test]
    fn lossy_conversion_requires_opt_in_and_reports_degradation() {
        let mut provider = provider("lossy-source");
        provider
            .endpoints
            .insert(Protocol::AnthropicMessages, "/v1/messages".into());
        provider
            .protocol_capabilities
            .insert(Protocol::AnthropicMessages, ProtocolCapability::native());
        provider.protocol_capabilities.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "kimi_responses_adapter"),
        );
        let mut adapter_route = route(
            "lossy-adapter",
            "model-a",
            "lossy-source",
            "lossy-account",
            Protocol::OpenAiResponses,
            "adapter",
        );
        adapter_route.adapter = Some("kimi_responses_adapter".into());
        let config = gateway(
            vec![provider],
            vec![account("lossy-account", "lossy-source")],
            vec![adapter_route],
        );

        let rejected = CapabilityMatrixResponse::from_config(Arc::new(config.clone()));
        let rejected = matrix_cell(&rejected, "lossy-adapter", Protocol::OpenAiResponses);
        assert_eq!(rejected.status, CapabilityRouteStatus::Unroutable);
        assert!(!rejected.allow_lossy_conversion);
        assert!(!rejected.degraded);
        assert_eq!(
            rejected.error.as_ref().map(|error| error.code.as_str()),
            Some("lossy_conversion_not_allowed")
        );

        let mut allowed_config = config;
        allowed_config.routes[0].allow_lossy_conversion = true;
        let allowed = CapabilityMatrixResponse::from_config(Arc::new(allowed_config));
        let allowed = matrix_cell(&allowed, "lossy-adapter", Protocol::OpenAiResponses);
        assert_eq!(allowed.status, CapabilityRouteStatus::Routable);
        assert!(allowed.allow_lossy_conversion);
        assert!(allowed.degraded);
        assert!(allowed.degraded_features.contains(&"file_search".into()));
        assert_eq!(
            allowed.effective_capabilities.file_search,
            CapabilityMode::Unsupported
        );
    }

    #[test]
    fn native_route_priority_is_reflected_without_reimplementing_selection() {
        let mut adapter_provider = provider("adapter-source");
        adapter_provider
            .endpoints
            .insert(Protocol::AnthropicMessages, "/v1/messages".into());
        adapter_provider
            .protocol_capabilities
            .insert(Protocol::AnthropicMessages, ProtocolCapability::native());
        adapter_provider.protocol_capabilities.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "kimi_responses_adapter"),
        );
        adapter_provider.capabilities.file_search = CapabilityMode::Unsupported;

        let mut native_provider = provider("native-source");
        native_provider
            .endpoints
            .insert(Protocol::OpenAiResponses, "/v1/responses".into());
        native_provider
            .protocol_capabilities
            .insert(Protocol::OpenAiResponses, ProtocolCapability::native());

        let mut adapter_route = route(
            "adapter-first-in-config",
            "model-a",
            "adapter-source",
            "adapter-account",
            Protocol::OpenAiResponses,
            "adapter",
        );
        adapter_route.adapter = Some("kimi_responses_adapter".into());
        let native_route = route(
            "native-second-in-config",
            "model-a",
            "native-source",
            "native-account",
            Protocol::OpenAiResponses,
            "native",
        );

        let response = CapabilityMatrixResponse::from_config(Arc::new(gateway(
            vec![adapter_provider, native_provider],
            vec![
                account("adapter-account", "adapter-source"),
                account("native-account", "native-source"),
            ],
            vec![adapter_route, native_route],
        )));

        let adapter = matrix_cell(
            &response,
            "adapter-first-in-config",
            Protocol::OpenAiResponses,
        );
        assert_eq!(adapter.status, CapabilityRouteStatus::Unroutable);
        assert_eq!(
            adapter.error.as_ref().map(|error| error.code.as_str()),
            Some("route_not_selected")
        );
        let native = matrix_cell(
            &response,
            "native-second-in-config",
            Protocol::OpenAiResponses,
        );
        assert_eq!(native.status, CapabilityRouteStatus::Routable);
        assert_eq!(native.mode, Some(ProtocolMode::Native));
    }
}
