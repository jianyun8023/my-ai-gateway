use crate::protocol::Protocol;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env};

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct Capabilities {
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub tool_streaming: bool,
    #[serde(default)]
    pub thinking: bool,
    #[serde(default)]
    pub web_search: bool,
    #[serde(default)]
    pub file_search: bool,
    #[serde(default)]
    pub vision: bool,
    #[serde(default)]
    pub usage: bool,
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
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AccountConfig {
    pub id: String,
    pub provider_id: String,
    pub display_name: String,
    #[serde(default)]
    pub credential_env: Option<String>,
    #[serde(default)]
    pub credential: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_weight")]
    pub weight: u32,
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
    pub fn from_env() -> Self {
        if let Ok(raw) = env::var("GATEWAY_CONFIG_JSON") {
            match serde_json::from_str::<Self>(&raw) {
                Ok(config) => return config,
                Err(error) => tracing::warn!(%error, "invalid GATEWAY_CONFIG_JSON; using defaults"),
            }
        }
        let mut endpoints = HashMap::new();
        endpoints.insert(
            Protocol::OpenAiChatCompletions,
            "/v1/chat/completions".into(),
        );
        endpoints.insert(Protocol::AnthropicMessages, "/v1/messages".into());
        let providers = vec![ProviderConfig {
            id: "kimi".into(),
            name: "Kimi Code".into(),
            base_url: env::var("KIMI_BASE_URL")
                .unwrap_or_else(|_| "https://api.kimi.com/coding".into()),
            models: vec!["kimi-for-coding-highspeed".into()],
            native_protocols: vec![Protocol::AnthropicMessages, Protocol::OpenAiChatCompletions],
            endpoints,
            capabilities: Capabilities {
                streaming: true,
                tools: true,
                thinking: true,
                web_search: true,
                usage: true,
                ..Default::default()
            },
        }];
        let accounts = vec![AccountConfig {
            id: "kimi-01".into(),
            provider_id: "kimi".into(),
            display_name: "Kimi primary".into(),
            credential_env: Some("KIMI_API_KEY".into()),
            credential: None,
            enabled: true,
            weight: 100,
        }];
        let routes = vec![RouteConfig {
            id: "kimi-responses-adapter".into(),
            model: "kimi-for-coding-highspeed".into(),
            provider_id: "kimi".into(),
            protocols: vec![Protocol::OpenAiResponses],
            primary_account_id: "kimi-01".into(),
            fallback_accounts: vec![],
            strategy: default_strategy(),
            mode: "adapter".into(),
            adapter: Some("kimi_responses_adapter".into()),
            allow_lossy_conversion: false,
        }];
        Self {
            listen_addr: env::var("GATEWAY_LISTEN_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:8787".into()),
            providers,
            accounts,
            routes,
        }
    }

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
    pub fn credential_for(&self, account: &AccountConfig) -> Option<String> {
        account
            .credential_env
            .as_deref()
            .and_then(|name| env::var(name).ok())
            .or_else(|| account.credential.clone())
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
