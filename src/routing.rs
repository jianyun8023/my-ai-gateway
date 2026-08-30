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

    /// Historical Option API; callers needing diagnostics should use `resolve_detailed`.
    pub fn resolve(&self, protocol: Protocol, model: &str) -> Option<ResolvedRoute> {
        self.resolve_detailed(protocol, model).ok()
    }

    pub fn resolve_detailed(
        &self,
        protocol: Protocol,
        model: &str,
    ) -> Result<ResolvedRoute, RouteResolutionError> {
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
                Ok(route) => return Ok(route),
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
}
