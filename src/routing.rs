use crate::{config::GatewayConfig, protocol::Protocol};
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone)]
pub struct RouteResolver {
    config: Arc<GatewayConfig>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedRoute {
    pub route_id: String,
    pub protocol: Protocol,
    pub model: String,
    pub provider_id: String,
    pub primary_account_id: String,
    pub fallback_accounts: Vec<String>,
    pub mode: String,
    pub adapter: Option<String>,
    pub allow_lossy_conversion: bool,
}

impl RouteResolver {
    pub fn new(config: Arc<GatewayConfig>) -> Self {
        Self { config }
    }

    pub fn resolve(&self, protocol: Protocol, model: &str) -> Option<ResolvedRoute> {
        self.config
            .routes
            .iter()
            .find(|route| route.protocols.contains(&protocol) && model_matches(&route.model, model))
            .map(|route| ResolvedRoute {
                route_id: route.id.clone(),
                protocol,
                model: model.to_owned(),
                provider_id: route.provider_id.clone(),
                primary_account_id: route.primary_account_id.clone(),
                fallback_accounts: route.fallback_accounts.clone(),
                mode: route.mode.clone(),
                adapter: route.adapter.clone(),
                allow_lossy_conversion: route.allow_lossy_conversion,
            })
    }
}

fn model_matches(pattern: &str, model: &str) -> bool {
    pattern == "*"
        || pattern == model
        || (pattern.ends_with('*') && model.starts_with(&pattern[..pattern.len() - 1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{AccountConfig, Capabilities, GatewayConfig, ProviderConfig, RouteConfig},
        protocol::Protocol,
    };
    use std::collections::HashMap;

    #[test]
    fn exact_route_and_wildcard_route_match() {
        let config = GatewayConfig {
            listen_addr: "127.0.0.1:1".into(),
            providers: vec![ProviderConfig {
                id: "p".into(),
                name: "p".into(),
                base_url: "http://localhost".into(),
                models: vec!["deepseek-chat".into()],
                native_protocols: vec![Protocol::OpenAiChatCompletions],
                endpoints: HashMap::new(),
                capabilities: Capabilities::default(),
            }],
            accounts: vec![AccountConfig {
                id: "a".into(),
                provider_id: "p".into(),
                display_name: "a".into(),
                credential_env: None,
                credential: None,
                enabled: true,
                weight: 100,
            }],
            routes: vec![RouteConfig {
                id: "r".into(),
                model: "deepseek-*".into(),
                provider_id: "p".into(),
                protocols: vec![Protocol::OpenAiChatCompletions],
                primary_account_id: "a".into(),
                fallback_accounts: vec![],
                strategy: "primary_then_weighted_fallback".into(),
                mode: "native".into(),
                adapter: None,
                allow_lossy_conversion: false,
            }],
        };
        let resolver = RouteResolver::new(Arc::new(config));
        assert_eq!(
            resolver
                .resolve(Protocol::OpenAiChatCompletions, "deepseek-chat")
                .unwrap()
                .primary_account_id,
            "a"
        );
        assert!(resolver
            .resolve(Protocol::OpenAiResponses, "deepseek-chat")
            .is_none());
    }
}
