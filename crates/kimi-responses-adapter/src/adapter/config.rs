use std::collections::HashMap;

/// Config holds all adapter configuration. Everything is driven by
/// environment variables so the binary stays a thin, stateless proxy.
#[derive(Clone, Debug)]
pub struct Config {
    /// Address the HTTP server binds to.
    pub listen_addr: String,
    /// Base URL of the Kimi Code Anthropic-compatible API.
    pub kimi_base_url: String,
    /// Sent as the anthropic-beta header when non-empty.
    pub anthropic_beta: String,
    /// Maps incoming Responses model names to upstream Kimi models.
    /// Unmapped names are passed through unchanged.
    pub model_map: HashMap<String, String>,
    /// Overrides the upstream User-Agent. Empty means pass through the
    /// inbound client's User-Agent unchanged.
    pub client_source: String,
    /// The list returned by GET /v1/models.
    pub models: Vec<String>,
    /// Default Anthropic max_tokens when the client does not specify
    /// max_output_tokens.
    pub max_tokens: i64,
    /// Maps reasoning effort to Anthropic thinking budget.
    pub thinking_budgets: HashMap<String, i64>,
    /// Marks Kimi's web-search status text blocks
    /// (e.g. "Search results for query: ...") that must be suppressed.
    pub search_status_prefix: String,
}

impl Config {
    pub fn load() -> Config {
        let mut cfg = Config {
            listen_addr: env_or("LISTEN_ADDR", ":8787"),
            kimi_base_url: env_or("KIMI_BASE_URL", "https://api.kimi.com/coding")
                .trim_end_matches('/')
                .to_string(),
            anthropic_beta: std::env::var("KIMI_ANTHROPIC_BETA").unwrap_or_default(),
            model_map: [(
                "codex-auto-review".to_string(),
                "kimi-for-coding-highspeed".to_string(),
            )]
            .into_iter()
            .collect(),
            client_source: std::env::var("KIMI_CLIENT_SOURCE").unwrap_or_default(),
            models: vec![
                "k3".to_string(),
                "k3-256k".to_string(),
                "kimi-for-coding".to_string(),
                "kimi-for-coding-highspeed".to_string(),
            ],
            max_tokens: env_int("KIMI_MAX_TOKENS", 32768),
            search_status_prefix: env_or("KIMI_SEARCH_STATUS_PREFIX", "Search results for query:"),
            thinking_budgets: [
                ("low".to_string(), 4096),
                ("medium".to_string(), 16384),
                ("high".to_string(), 32768),
            ]
            .into_iter()
            .collect(),
        };
        if let Ok(v) = std::env::var("KIMI_MODEL_MAP") {
            if !v.is_empty() {
                if let Ok(m) = serde_json::from_str::<HashMap<String, String>>(&v) {
                    cfg.model_map.extend(m);
                }
            }
        }
        if let Ok(v) = std::env::var("KIMI_MODELS") {
            if !v.is_empty() {
                cfg.models = v
                    .split(',')
                    .map(str::trim)
                    .filter(|m| !m.is_empty())
                    .map(str::to_string)
                    .collect();
            }
        }
        if let Ok(v) = std::env::var("KIMI_THINKING_BUDGETS") {
            if !v.is_empty() {
                if let Ok(b) = serde_json::from_str(&v) {
                    cfg.thinking_budgets = b;
                }
            }
        }
        cfg
    }
}

fn env_or(key: &str, def: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => v,
        _ => def.to_string(),
    }
}

fn env_int(key: &str, def: i64) -> i64 {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => v.parse().unwrap_or(def),
        _ => def,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::convert::build_anthropic_request;
    use crate::adapter::stream::translate_sse;
    use crate::adapter::types::ResponsesRequest;
    use crate::adapter::{clear_env, env_lock, set_env};

    // ---- 正向: env vars are honored ----

    #[test]
    fn env_all_custom_values_loaded() {
        let _g = env_lock();
        clear_env();
        set_env("LISTEN_ADDR", "127.0.0.1:9000");
        set_env("KIMI_BASE_URL", "https://kimi.internal/coding");
        set_env("KIMI_ANTHROPIC_BETA", "interleaved-thinking-2025-05-14");
        set_env("KIMI_MODEL_MAP", r#"{"k3":"kimi-for-coding"}"#);
        set_env("KIMI_CLIENT_SOURCE", "my-codex/1.0");
        set_env("KIMI_MODELS", "k3,k3-256k");
        set_env("KIMI_MAX_TOKENS", "8192");
        set_env(
            "KIMI_THINKING_BUDGETS",
            r#"{"low":100,"medium":200,"high":300}"#,
        );
        set_env("KIMI_SEARCH_STATUS_PREFIX", "STATUS:");

        let cfg = Config::load();
        assert_eq!(cfg.listen_addr, "127.0.0.1:9000");
        assert_eq!(cfg.kimi_base_url, "https://kimi.internal/coding");
        assert_eq!(cfg.anthropic_beta, "interleaved-thinking-2025-05-14");
        assert_eq!(cfg.model_map.get("k3").unwrap(), "kimi-for-coding");
        assert_eq!(
            cfg.model_map.get("codex-auto-review").unwrap(),
            "kimi-for-coding-highspeed"
        );
        assert_eq!(cfg.client_source, "my-codex/1.0");
        assert_eq!(cfg.models, vec!["k3", "k3-256k"]);
        assert_eq!(cfg.max_tokens, 8192);
        assert_eq!(cfg.thinking_budgets.get("high"), Some(&300));
        assert_eq!(cfg.search_status_prefix, "STATUS:");
        clear_env();
    }

    #[test]
    fn env_base_url_trailing_slashes_trimmed() {
        let _g = env_lock();
        clear_env();
        set_env("KIMI_BASE_URL", "https://kimi.internal/coding///");
        let cfg = Config::load();
        assert_eq!(cfg.kimi_base_url, "https://kimi.internal/coding");
        clear_env();
    }

    #[test]
    fn env_models_list_skips_blanks() {
        let _g = env_lock();
        clear_env();
        set_env("KIMI_MODELS", " k3 , ,k3-256k ,");
        let cfg = Config::load();
        assert_eq!(cfg.models, vec!["k3", "k3-256k"]);
        clear_env();
    }

    // Env-driven behavior: model map from env reshapes the upstream request.
    #[test]
    fn env_model_map_applied_to_request() {
        let _g = env_lock();
        clear_env();
        set_env(
            "KIMI_MODEL_MAP",
            r#"{"k3-256k":"kimi-for-coding-highspeed"}"#,
        );
        let cfg = Config::load();
        let req: ResponsesRequest =
            serde_json::from_str(r#"{"model":"k3-256k","input":"hi"}"#).unwrap();
        let out = build_anthropic_request(&cfg, &req, None).unwrap();
        assert_eq!(out.model, "kimi-for-coding-highspeed");
        clear_env();
    }

    #[test]
    fn codex_auto_review_maps_to_highspeed_by_default() {
        let _g = env_lock();
        clear_env();
        let cfg = Config::load();
        let req: ResponsesRequest =
            serde_json::from_str(r#"{"model":"codex-auto-review","input":"review"}"#).unwrap();
        let out = build_anthropic_request(&cfg, &req, None).unwrap();
        assert_eq!(out.model, "kimi-for-coding-highspeed");
        clear_env();
    }

    #[test]
    fn env_model_map_can_override_default() {
        let _g = env_lock();
        clear_env();
        set_env(
            "KIMI_MODEL_MAP",
            r#"{"codex-auto-review":"kimi-for-coding"}"#,
        );
        let cfg = Config::load();
        assert_eq!(
            cfg.model_map.get("codex-auto-review").unwrap(),
            "kimi-for-coding"
        );
        clear_env();
    }

    // Env-driven behavior: custom budgets change the thinking config.
    #[test]
    fn env_thinking_budgets_applied_to_request() {
        let _g = env_lock();
        clear_env();
        set_env(
            "KIMI_THINKING_BUDGETS",
            r#"{"low":100,"medium":200,"high":300}"#,
        );
        set_env("KIMI_MAX_TOKENS", "8192");
        let cfg = Config::load();
        let req: ResponsesRequest =
            serde_json::from_str(r#"{"model":"k3","input":"hi","reasoning":{"effort":"high"}}"#)
                .unwrap();
        let out = build_anthropic_request(&cfg, &req, None).unwrap();
        let thinking = out.thinking.unwrap();
        assert_eq!(thinking.r#type, "enabled");
        assert_eq!(thinking.budget_tokens, 300);
        clear_env();
    }

    // Env-driven behavior: a custom status prefix is what the stream
    // translator suppresses (and salvages the query from).
    #[test]
    fn env_search_prefix_applied_to_stream() {
        let _g = env_lock();
        clear_env();
        set_env("KIMI_SEARCH_STATUS_PREFIX", "STATUS:");
        let cfg = Config::load();
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"STATUS: golang\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"server_tool_use\",\"name\":\"web_search\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let req: ResponsesRequest =
            serde_json::from_str(r#"{"model":"k3","stream":true,"input":"hi"}"#).unwrap();
        let mut out = String::new();
        translate_sse(&cfg, &req, upstream.as_bytes(), |s| out.push_str(&s)).unwrap();
        assert!(
            !out.contains("STATUS: golang"),
            "custom status text leaked:\n{out}"
        );
        assert!(
            out.contains(r#""query":"golang""#),
            "salvaged query missing:\n{out}"
        );
        clear_env();
    }

    // ---- 反向: unset/empty/wrong-typed values fall back to defaults ----

    #[test]
    fn env_unset_uses_defaults() {
        let _g = env_lock();
        clear_env();
        let cfg = Config::load();
        assert_eq!(cfg.listen_addr, ":8787");
        assert_eq!(cfg.kimi_base_url, "https://api.kimi.com/coding");
        assert_eq!(cfg.anthropic_beta, "");
        assert_eq!(
            cfg.model_map.get("codex-auto-review").unwrap(),
            "kimi-for-coding-highspeed"
        );
        assert!(cfg.client_source.is_empty());
        assert_eq!(
            cfg.models,
            vec![
                "k3",
                "k3-256k",
                "kimi-for-coding",
                "kimi-for-coding-highspeed"
            ]
        );
        assert_eq!(cfg.max_tokens, 32768);
        assert_eq!(cfg.thinking_budgets.get("low"), Some(&4096));
        assert_eq!(cfg.thinking_budgets.get("medium"), Some(&16384));
        assert_eq!(cfg.thinking_budgets.get("high"), Some(&32768));
        assert_eq!(cfg.search_status_prefix, "Search results for query:");
    }

    #[test]
    fn env_empty_strings_fall_back_to_defaults() {
        let _g = env_lock();
        clear_env();
        // Empty is treated as unset for every variable.
        for k in crate::adapter::ENV_KEYS {
            set_env(k, "");
        }
        let cfg = Config::load();
        assert_eq!(cfg.listen_addr, ":8787");
        assert_eq!(cfg.kimi_base_url, "https://api.kimi.com/coding");
        assert_eq!(cfg.max_tokens, 32768);
        assert_eq!(cfg.models.len(), 4);
        assert_eq!(
            cfg.model_map.get("codex-auto-review").unwrap(),
            "kimi-for-coding-highspeed"
        );
        clear_env();
    }

    #[test]
    fn env_non_numeric_max_tokens_falls_back() {
        let _g = env_lock();
        clear_env();
        set_env("KIMI_MAX_TOKENS", "abc");
        assert_eq!(Config::load().max_tokens, 32768);
        set_env("KIMI_MAX_TOKENS", "12x");
        assert_eq!(Config::load().max_tokens, 32768);
        clear_env();
    }

    // ---- 异常: malformed input must not panic and must not corrupt state ----

    #[test]
    fn env_malformed_model_map_ignored() {
        let _g = env_lock();
        clear_env();
        set_env("KIMI_MODEL_MAP", "{not json");
        let cfg = Config::load();
        assert_eq!(
            cfg.model_map.get("codex-auto-review").unwrap(),
            "kimi-for-coding-highspeed",
            "malformed map must leave defaults intact"
        );
        // A subsequent valid request still converts with an unmapped model.
        let req: ResponsesRequest = serde_json::from_str(r#"{"model":"k3","input":"hi"}"#).unwrap();
        let out = build_anthropic_request(&cfg, &req, None).unwrap();
        assert_eq!(out.model, "k3");
        clear_env();
    }

    #[test]
    fn env_malformed_thinking_budgets_ignored() {
        let _g = env_lock();
        clear_env();
        set_env("KIMI_THINKING_BUDGETS", "not json at all");
        let cfg = Config::load();
        assert_eq!(cfg.thinking_budgets.get("medium"), Some(&16384));
        clear_env();
    }

    #[test]
    fn env_models_only_separators_yields_empty_list() {
        let _g = env_lock();
        clear_env();
        set_env("KIMI_MODELS", ",, ,");
        let cfg = Config::load();
        assert!(cfg.models.is_empty());
        clear_env();
    }
}
