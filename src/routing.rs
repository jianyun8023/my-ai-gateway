use crate::{
    config::{adapter_definition, Capabilities, CapabilityMode, GatewayConfig, ProtocolMode},
    protocol::Protocol,
};
use serde::Serialize;
use std::{fmt, sync::Arc};

#[derive(Clone)]
pub struct RouteResolver {
    config: Arc<GatewayConfig>,
}

/// Complete ingress -> upstream explanation of a selected route.
#[derive(Clone, Debug, Serialize)]
pub struct ResolvedRoute {
    pub route_id: String,
    /// Backward-compatible alias for `protocol_in`.
    pub protocol: Protocol,
    pub protocol_in: Protocol,
    pub protocol_upstream: Protocol,
    pub model: String,
    pub requested_model: String,
    pub provider_id: String,
    pub primary_account_id: String,
    pub fallback_accounts: Vec<String>,
    pub upstream_endpoint: String,
    pub mode: String,
    pub adapter: Option<String>,
    pub effective_capabilities: Capabilities,
    pub degraded_features: Vec<String>,
    pub allow_lossy_conversion: bool,
}

impl ResolvedRoute {
    pub fn is_degraded(&self) -> bool {
        !self.degraded_features.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RouteResolutionError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_id: Option<String>,
}

impl RouteResolutionError {
    fn new(code: impl Into<String>, message: impl Into<String>, route_id: Option<&str>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            route_id: route_id.map(str::to_owned),
        }
    }
}
impl fmt::Display for RouteResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for RouteResolutionError {}

impl RouteResolver {
    pub fn new(config: Arc<GatewayConfig>) -> Self {
        Self { config }
    }

    pub fn resolve_detailed(
        &self,
        protocol: Protocol,
        model: &str,
    ) -> Result<ResolvedRoute, RouteResolutionError> {
        self.resolve_selected(protocol, model)
            .map(|(_, route)| route)
    }

    /// Resolve one configured route while still applying the resolver's global
    /// candidate ordering. This is used by diagnostics that need to explain
    /// every configured route without maintaining a second routing algorithm.
    pub(crate) fn resolve_configured_route(
        &self,
        route_index: usize,
        protocol: Protocol,
        model: &str,
    ) -> Result<ResolvedRoute, RouteResolutionError> {
        let Some(route) = self.config.routes.get(route_index) else {
            return Err(RouteResolutionError::new(
                "route_not_found",
                format!("configured route index {route_index} does not exist"),
                None,
            ));
        };
        if !route.protocols.contains(&protocol) {
            return Err(RouteResolutionError::new(
                "route_protocol_not_configured",
                format!(
                    "route '{}' is not configured for protocol {protocol}",
                    route.id
                ),
                Some(&route.id),
            ));
        }
        if model_match_rank(&route.model, model).is_none() {
            return Err(RouteResolutionError::new(
                "route_model_not_matched",
                format!("route '{}' does not match model '{model}'", route.id),
                Some(&route.id),
            ));
        }

        let candidate = self.resolve_candidate(route_index, protocol, model)?;
        let (selected_index, selected) = self.resolve_selected(protocol, model)?;
        if selected_index != route_index {
            return Err(RouteResolutionError::new(
                "route_not_selected",
                format!(
                    "route '{}' is routable but route '{}' has higher priority",
                    route.id, selected.route_id
                ),
                Some(&route.id),
            ));
        }
        Ok(candidate)
    }

    fn resolve_selected(
        &self,
        protocol: Protocol,
        model: &str,
    ) -> Result<(usize, ResolvedRoute), RouteResolutionError> {
        let mut candidates: Vec<(usize, usize, bool, u8)> = self
            .config
            .routes
            .iter()
            .enumerate()
            .filter_map(|(index, route)| {
                route
                    .protocols
                    .contains(&protocol)
                    .then(|| {
                        model_match_rank(&route.model, model).map(|(exact, specificity)| {
                            let mode_rank = if route.mode == "native" { 0 } else { 1 };
                            (index, specificity, exact, mode_rank)
                        })
                    })
                    .flatten()
            })
            .collect();
        // Exact model > longest prefix > wildcard; ties retain config order.
        candidates.sort_by_key(|(_, specificity, exact, mode_rank)| {
            (!*exact, std::cmp::Reverse(*specificity), *mode_rank)
        });
        let mut last_error = None;
        for (index, _, _, _) in candidates {
            match self.resolve_candidate(index, protocol, model) {
                Ok(route) => return Ok((index, route)),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            RouteResolutionError::new(
                "route_not_found",
                format!("no route matches protocol {protocol} and model '{model}'"),
                None,
            )
        }))
    }

    fn resolve_candidate(
        &self,
        index: usize,
        protocol: Protocol,
        model: &str,
    ) -> Result<ResolvedRoute, RouteResolutionError> {
        let route = &self.config.routes[index];
        let provider = self.config.provider(&route.provider_id).ok_or_else(|| {
            RouteResolutionError::new(
                "provider_not_found",
                format!("route references unknown provider '{}'", route.provider_id),
                Some(&route.id),
            )
        })?;
        let account = self
            .config
            .account(&route.primary_account_id)
            .ok_or_else(|| {
                RouteResolutionError::new(
                    "account_not_found",
                    format!(
                        "route references unknown primary account '{}'",
                        route.primary_account_id
                    ),
                    Some(&route.id),
                )
            })?;
        if account.provider_id != provider.id {
            return Err(RouteResolutionError::new(
                "account_provider_mismatch",
                format!(
                    "primary account '{}' belongs to provider '{}'",
                    account.id, account.provider_id
                ),
                Some(&route.id),
            ));
        }
        if !account.enabled {
            return Err(RouteResolutionError::new(
                "account_disabled",
                format!("primary account '{}' is disabled", account.id),
                Some(&route.id),
            ));
        }
        for fallback in &route.fallback_accounts {
            let Some(account) = self.config.account(fallback) else {
                return Err(RouteResolutionError::new(
                    "fallback_account_not_found",
                    format!("route references unknown fallback account '{fallback}'"),
                    Some(&route.id),
                ));
            };
            if account.provider_id != provider.id {
                return Err(RouteResolutionError::new(
                    "fallback_provider_mismatch",
                    format!(
                        "fallback account '{}' belongs to provider '{}'",
                        fallback, account.provider_id
                    ),
                    Some(&route.id),
                ));
            }
        }

        let declared =
            self.config
                .protocol_capability(&provider.id, Some(&account.id), model, protocol);
        let (upstream, mode, adapter_name, adapter_features) = match route.mode.as_str() {
            "native" => {
                if declared.mode != ProtocolMode::Native {
                    return Err(RouteResolutionError::new(
                        "unsupported_protocol",
                        format!(
                            "protocol {protocol} is not natively supported for model '{model}'"
                        ),
                        Some(&route.id),
                    ));
                }
                (protocol, "native".to_owned(), None, None)
            }
            "adapter" => {
                let name = route.adapter.as_deref().ok_or_else(|| {
                    RouteResolutionError::new(
                        "adapter_missing",
                        "adapter route requires an adapter name",
                        Some(&route.id),
                    )
                })?;
                let definition = adapter_definition(name).ok_or_else(|| {
                    RouteResolutionError::new(
                        "adapter_unknown",
                        format!("unknown adapter '{name}'"),
                        Some(&route.id),
                    )
                })?;
                if definition.from_protocol != protocol {
                    return Err(RouteResolutionError::new(
                        "adapter_direction_mismatch",
                        format!(
                            "adapter '{name}' accepts {}, not {protocol}",
                            definition.from_protocol
                        ),
                        Some(&route.id),
                    ));
                }
                if declared.mode != ProtocolMode::Adapter
                    || declared.adapter.as_deref() != Some(name)
                    || declared.source_protocol != Some(definition.to_protocol)
                {
                    return Err(RouteResolutionError::new("adapter_capability_mismatch", format!("route adapter '{name}' does not match the effective protocol capability"), Some(&route.id)));
                }
                let source_capability = self.config.protocol_capability(
                    &provider.id,
                    Some(&account.id),
                    model,
                    definition.to_protocol,
                );
                if source_capability.mode != ProtocolMode::Native {
                    return Err(RouteResolutionError::new(
                        "adapter_source_unsupported",
                        format!(
                            "adapter source protocol {} is not natively available",
                            definition.to_protocol
                        ),
                        Some(&route.id),
                    ));
                }
                (
                    definition.to_protocol,
                    "adapter".to_owned(),
                    Some(name.to_owned()),
                    Some(definition.features),
                )
            }
            "unsupported" => {
                return Err(RouteResolutionError::new(
                    "unsupported_protocol",
                    format!(
                        "route '{}' explicitly marks protocol {protocol} unsupported",
                        route.id
                    ),
                    Some(&route.id),
                ))
            }
            other => {
                return Err(RouteResolutionError::new(
                    "invalid_route_mode",
                    format!("unknown route mode '{other}'"),
                    Some(&route.id),
                ))
            }
        };
        let endpoint = provider.endpoints.get(&upstream).ok_or_else(|| {
            RouteResolutionError::new(
                "endpoint_missing",
                format!("provider '{}' has no endpoint for {upstream}", provider.id),
                Some(&route.id),
            )
        })?;
        if endpoint.trim().is_empty() {
            return Err(RouteResolutionError::new(
                "endpoint_missing",
                format!(
                    "provider '{}' has an empty endpoint for {upstream}",
                    provider.id
                ),
                Some(&route.id),
            ));
        }
        let (effective_capabilities, degraded_features) = intersect_capabilities(
            &self
                .config
                .capabilities(&provider.id, Some(&account.id), model),
            adapter_features.as_ref(),
            route.allow_lossy_conversion,
        )
        .map_err(|feature| {
            RouteResolutionError::new(
                "lossy_conversion_not_allowed",
                format!("feature '{feature}' cannot be represented by adapter"),
                Some(&route.id),
            )
        })?;
        Ok(ResolvedRoute {
            route_id: route.id.clone(),
            protocol,
            protocol_in: protocol,
            protocol_upstream: upstream,
            model: model.to_owned(),
            requested_model: model.to_owned(),
            provider_id: provider.id.clone(),
            primary_account_id: account.id.clone(),
            fallback_accounts: route.fallback_accounts.clone(),
            upstream_endpoint: join_endpoint(&provider.base_url, endpoint),
            mode,
            adapter: adapter_name,
            effective_capabilities,
            degraded_features,
            allow_lossy_conversion: route.allow_lossy_conversion,
        })
    }
}

fn model_match_rank(pattern: &str, model: &str) -> Option<(bool, usize)> {
    if pattern == model {
        Some((true, pattern.len()))
    } else if pattern == "*" {
        Some((false, 0))
    } else if pattern.ends_with('*') && model.starts_with(&pattern[..pattern.len() - 1]) {
        Some((false, pattern.len() - 1))
    } else {
        None
    }
}

fn join_endpoint(base: &str, endpoint: &str) -> String {
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_owned()
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        )
    }
}

fn intersect_capabilities(
    model: &Capabilities,
    adapter: Option<&Capabilities>,
    allow_lossy: bool,
) -> Result<(Capabilities, Vec<String>), String> {
    let mut degraded = Vec::new();
    macro_rules! intersect {
        ($field:ident) => {{
            let source = model.$field;
            match adapter.map(|a| a.$field).unwrap_or(CapabilityMode::Native) {
                CapabilityMode::Unsupported if source != CapabilityMode::Unsupported => {
                    if !allow_lossy {
                        return Err(stringify!($field).to_owned());
                    }
                    degraded.push(stringify!($field).to_owned());
                    CapabilityMode::Unsupported
                }
                CapabilityMode::Translated if source == CapabilityMode::Native => {
                    degraded.push(stringify!($field).to_owned());
                    CapabilityMode::Translated
                }
                mode => {
                    if source == CapabilityMode::Unsupported {
                        CapabilityMode::Unsupported
                    } else {
                        mode
                    }
                }
            }
        }};
    }
    Ok((
        Capabilities {
            streaming: intersect!(streaming),
            tools: intersect!(tools),
            tool_streaming: intersect!(tool_streaming),
            thinking: intersect!(thinking),
            web_search: intersect!(web_search),
            file_search: intersect!(file_search),
            vision: intersect!(vision),
            usage: intersect!(usage),
        },
        degraded,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AccountConfig, ModelCapabilityOverride, ProtocolCapability, ProviderConfig, RouteConfig,
    };
    use std::collections::HashMap;

    fn config(provider: ProviderConfig, routes: Vec<RouteConfig>) -> GatewayConfig {
        GatewayConfig {
            listen_addr: "127.0.0.1:1".into(),
            providers: vec![provider],
            accounts: vec![AccountConfig {
                id: "a".into(),
                provider_id: "p".into(),
                display_name: "a".into(),
                credential_env: None,
                credential: None,
                enabled: true,
                weight: 100,
                protocol_capabilities: HashMap::new(),
                capabilities: Some(Capabilities::native()),
                model_overrides: HashMap::new(),
                model_map: HashMap::new(),
            }],
            routes,
        }
    }
    fn provider() -> ProviderConfig {
        ProviderConfig {
            id: "p".into(),
            name: "p".into(),
            base_url: "https://up.example/api/".into(),
            models: vec!["m".into()],
            native_protocols: vec![Protocol::OpenAiChatCompletions],
            endpoints: HashMap::from([(Protocol::OpenAiChatCompletions, "/v1/chat".into())]),
            capabilities: Capabilities::native(),
            protocol_capabilities: HashMap::new(),
            model_overrides: HashMap::new(),
        }
    }
    fn route(id: &str, model: &str, protocol: Protocol) -> RouteConfig {
        RouteConfig {
            id: id.into(),
            model: model.into(),
            provider_id: "p".into(),
            protocols: vec![protocol],
            primary_account_id: "a".into(),
            fallback_accounts: vec![],
            strategy: "x".into(),
            mode: "native".into(),
            adapter: None,
            allow_lossy_conversion: false,
        }
    }

    #[test]
    fn exact_model_wins_and_chain_serializes() {
        let routes = vec![
            route("wild", "m-*", Protocol::OpenAiChatCompletions),
            route("exact", "m", Protocol::OpenAiChatCompletions),
        ];
        let resolved = RouteResolver::new(Arc::new(config(provider(), routes)))
            .resolve_detailed(Protocol::OpenAiChatCompletions, "m")
            .unwrap();
        assert_eq!(resolved.route_id, "exact");
        assert_eq!(resolved.protocol_in, Protocol::OpenAiChatCompletions);
        assert_eq!(resolved.protocol_upstream, Protocol::OpenAiChatCompletions);
        assert_eq!(resolved.upstream_endpoint, "https://up.example/api/v1/chat");
        assert!(serde_json::to_value(resolved).unwrap()["effective_capabilities"].is_object());
    }

    #[test]
    fn kimi_adapter_direction_endpoint_and_lossy_policy() {
        let mut p = provider();
        p.base_url = "https://kimi.example/coding".into();
        p.native_protocols.push(Protocol::AnthropicMessages);
        p.endpoints
            .insert(Protocol::AnthropicMessages, "/v1/messages".into());
        p.protocol_capabilities.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "kimi_responses_adapter"),
        );
        p.model_overrides.insert(
            "m".into(),
            ModelCapabilityOverride {
                protocol_capabilities: HashMap::new(),
                capabilities: Some(Capabilities {
                    file_search: CapabilityMode::Native,
                    ..Capabilities::native()
                }),
            },
        );
        let mut r = route("adapter", "m", Protocol::OpenAiResponses);
        r.mode = "adapter".into();
        r.adapter = Some("kimi_responses_adapter".into());
        r.allow_lossy_conversion = true;
        let resolved = RouteResolver::new(Arc::new(config(p, vec![r])))
            .resolve_detailed(Protocol::OpenAiResponses, "m")
            .unwrap();
        assert_eq!(resolved.protocol_upstream, Protocol::AnthropicMessages);
        assert_eq!(
            resolved.upstream_endpoint,
            "https://kimi.example/coding/v1/messages"
        );
        assert!(resolved
            .degraded_features
            .contains(&"file_search".to_owned()));
    }

    #[test]
    fn three_protocols_native_resolve() {
        let mut p = provider();
        p.native_protocols = vec![
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ];
        p.endpoints
            .insert(Protocol::OpenAiResponses, "/v1/responses".into());
        p.endpoints
            .insert(Protocol::AnthropicMessages, "/v1/messages".into());
        let routes = vec![
            RouteConfig {
                id: "chat".into(),
                model: "m".into(),
                provider_id: "p".into(),
                protocols: vec![Protocol::OpenAiChatCompletions],
                primary_account_id: "a".into(),
                fallback_accounts: vec![],
                strategy: "x".into(),
                mode: "native".into(),
                adapter: None,
                allow_lossy_conversion: false,
            },
            RouteConfig {
                id: "resp".into(),
                model: "m".into(),
                provider_id: "p".into(),
                protocols: vec![Protocol::OpenAiResponses],
                primary_account_id: "a".into(),
                fallback_accounts: vec![],
                strategy: "x".into(),
                mode: "native".into(),
                adapter: None,
                allow_lossy_conversion: false,
            },
            RouteConfig {
                id: "anth".into(),
                model: "m".into(),
                provider_id: "p".into(),
                protocols: vec![Protocol::AnthropicMessages],
                primary_account_id: "a".into(),
                fallback_accounts: vec![],
                strategy: "x".into(),
                mode: "native".into(),
                adapter: None,
                allow_lossy_conversion: false,
            },
        ];
        let resolver = RouteResolver::new(Arc::new(config(p, routes)));
        for (protocol, expected_id, expected_endpoint) in [
            (Protocol::OpenAiChatCompletions, "chat", "/v1/chat"),
            (Protocol::OpenAiResponses, "resp", "/v1/responses"),
            (Protocol::AnthropicMessages, "anth", "/v1/messages"),
        ] {
            let resolved = resolver.resolve_detailed(protocol, "m").unwrap();
            assert_eq!(resolved.route_id, expected_id);
            assert_eq!(resolved.protocol_upstream, protocol);
            assert!(
                resolved.upstream_endpoint.ends_with(expected_endpoint),
                "endpoint mismatch for {protocol}: {}",
                resolved.upstream_endpoint
            );
        }
    }

    #[test]
    fn three_protocol_by_native_adapter_unsupported_resolution_matrix() {
        let protocols = [
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ];

        for protocol in protocols {
            for mode in ["native", "adapter", "unsupported"] {
                let mut p = provider();
                p.native_protocols = protocols.to_vec();
                p.endpoints
                    .insert(Protocol::OpenAiResponses, "/v1/responses".into());
                p.endpoints
                    .insert(Protocol::AnthropicMessages, "/v1/messages".into());
                let mut r = route("matrix", "m", protocol);
                r.mode = mode.into();
                if mode == "adapter" {
                    r.adapter = Some("kimi_responses_adapter".into());
                    r.allow_lossy_conversion = true;
                    if protocol == Protocol::OpenAiResponses {
                        p.protocol_capabilities.insert(
                            Protocol::OpenAiResponses,
                            ProtocolCapability::adapter(
                                Protocol::AnthropicMessages,
                                "kimi_responses_adapter",
                            ),
                        );
                    }
                }

                let resolved = RouteResolver::new(Arc::new(config(p, vec![r])))
                    .resolve_detailed(protocol, "m");
                match (protocol, mode) {
                    (_, "native") => {
                        let route = resolved.expect("native matrix cell must resolve");
                        assert_eq!(route.mode, "native");
                        assert_eq!(route.protocol_upstream, protocol);
                    }
                    (Protocol::OpenAiResponses, "adapter") => {
                        let route = resolved.expect("registered adapter matrix cell must resolve");
                        assert_eq!(route.mode, "adapter");
                        assert_eq!(route.protocol_upstream, Protocol::AnthropicMessages);
                    }
                    (_, "adapter") => {
                        assert_eq!(resolved.unwrap_err().code, "adapter_direction_mismatch");
                    }
                    (_, "unsupported") => {
                        assert_eq!(resolved.unwrap_err().code, "unsupported_protocol");
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn lossy_adapter_is_rejected_when_conversion_loss_is_not_allowed() {
        let mut p = provider();
        p.native_protocols = vec![Protocol::AnthropicMessages];
        p.endpoints
            .insert(Protocol::AnthropicMessages, "/v1/messages".into());
        p.protocol_capabilities.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "kimi_responses_adapter"),
        );
        let mut r = route("lossy", "m", Protocol::OpenAiResponses);
        r.mode = "adapter".into();
        r.adapter = Some("kimi_responses_adapter".into());

        let error = RouteResolver::new(Arc::new(config(p, vec![r])))
            .resolve_detailed(Protocol::OpenAiResponses, "m")
            .unwrap_err();
        assert_eq!(error.code, "lossy_conversion_not_allowed");
        assert_eq!(error.route_id.as_deref(), Some("lossy"));
    }

    #[test]
    fn allow_lossy_without_actual_feature_loss_is_not_degraded() {
        let mut r = route("native-lossy-enabled", "m", Protocol::OpenAiChatCompletions);
        r.allow_lossy_conversion = true;
        let resolved = RouteResolver::new(Arc::new(config(provider(), vec![r])))
            .resolve_detailed(Protocol::OpenAiChatCompletions, "m")
            .unwrap();
        assert!(resolved.degraded_features.is_empty());
        assert!(!resolved.is_degraded());
    }

    #[test]
    fn unknown_model_returns_route_not_found() {
        let resolver = RouteResolver::new(Arc::new(config(
            provider(),
            vec![route("r1", "m", Protocol::OpenAiChatCompletions)],
        )));
        let error = resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "nonexistent")
            .unwrap_err();
        assert_eq!(error.code, "route_not_found");
    }

    #[test]
    fn unknown_protocol_returns_route_not_found() {
        let resolver = RouteResolver::new(Arc::new(config(
            provider(),
            vec![route("r1", "m", Protocol::OpenAiChatCompletions)],
        )));
        let error = resolver
            .resolve_detailed(Protocol::AnthropicMessages, "m")
            .unwrap_err();
        assert_eq!(error.code, "route_not_found");
    }

    #[test]
    fn wildcard_model_matches_prefix() {
        let routes = vec![route("wild", "gpt-*", Protocol::OpenAiChatCompletions)];
        let resolver = RouteResolver::new(Arc::new(config(provider(), routes)));
        let resolved = resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "gpt-4o")
            .unwrap();
        assert_eq!(resolved.route_id, "wild");
        assert_eq!(resolved.requested_model, "gpt-4o");
    }

    #[test]
    fn multi_protocol_route_matches_each_protocol() {
        let mut p = provider();
        p.native_protocols = vec![Protocol::OpenAiChatCompletions, Protocol::AnthropicMessages];
        p.endpoints
            .insert(Protocol::AnthropicMessages, "/v1/messages".into());
        let routes = vec![RouteConfig {
            id: "multi".into(),
            model: "m".into(),
            provider_id: "p".into(),
            protocols: vec![Protocol::OpenAiChatCompletions, Protocol::AnthropicMessages],
            primary_account_id: "a".into(),
            fallback_accounts: vec![],
            strategy: "x".into(),
            mode: "native".into(),
            adapter: None,
            allow_lossy_conversion: false,
        }];
        let resolver = RouteResolver::new(Arc::new(config(p, routes)));
        assert!(resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "m")
            .is_ok());
        assert!(resolver
            .resolve_detailed(Protocol::AnthropicMessages, "m")
            .is_ok());
        assert!(resolver
            .resolve_detailed(Protocol::OpenAiResponses, "m")
            .is_err());
    }

    #[test]
    fn config_validation_rejects_adapter_cross_provider_fallback() {
        let mut p = provider();
        p.id = "p1".into();
        p.native_protocols = vec![Protocol::OpenAiChatCompletions, Protocol::AnthropicMessages];
        p.endpoints
            .insert(Protocol::AnthropicMessages, "/v1/messages".into());
        p.protocol_capabilities.insert(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "kimi_responses_adapter"),
        );
        let p2 = ProviderConfig {
            id: "p2".into(),
            name: "p2".into(),
            base_url: "https://other.example/".into(),
            models: vec![],
            native_protocols: vec![Protocol::OpenAiChatCompletions],
            endpoints: HashMap::from([(Protocol::OpenAiChatCompletions, "/v1/chat".into())]),
            capabilities: Capabilities::native(),
            protocol_capabilities: HashMap::new(),
            model_overrides: HashMap::new(),
        };
        let a2 = AccountConfig {
            id: "a2".into(),
            provider_id: "p2".into(),
            display_name: "a2".into(),
            credential_env: None,
            credential: None,
            enabled: true,
            weight: 100,
            protocol_capabilities: HashMap::new(),
            capabilities: Some(Capabilities::native()),
            model_overrides: HashMap::new(),
            model_map: HashMap::new(),
        };
        let mut r = route("adapter-route", "m", Protocol::OpenAiResponses);
        r.provider_id = "p1".into();
        r.mode = "adapter".into();
        r.adapter = Some("kimi_responses_adapter".into());
        r.fallback_accounts = vec!["a2".into()];
        let cfg = GatewayConfig {
            listen_addr: "127.0.0.1:1".into(),
            providers: vec![p, p2],
            accounts: vec![
                AccountConfig {
                    id: "a".into(),
                    provider_id: "p1".into(),
                    display_name: "a".into(),
                    credential_env: None,
                    credential: None,
                    enabled: true,
                    weight: 100,
                    protocol_capabilities: HashMap::new(),
                    capabilities: Some(Capabilities::native()),
                    model_overrides: HashMap::new(),
                    model_map: HashMap::new(),
                },
                a2,
            ],
            routes: vec![r],
        };
        let errors = cfg.validate().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("adapter route cannot have cross-provider fallback")),
            "expected cross-provider adapter error, got: {errors:?}"
        );
    }
}
