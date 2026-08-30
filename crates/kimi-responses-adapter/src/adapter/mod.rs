pub mod config;
pub mod convert;
pub mod models;
pub mod nonstream;
pub mod reasoning;
pub mod server;
pub mod stream;
pub mod types;

#[cfg(test)]
pub(crate) fn test_config() -> config::Config {
    config::Config {
        listen_addr: String::new(),
        kimi_base_url: String::new(),
        anthropic_beta: String::new(),
        model_map: Default::default(),
        client_source: String::new(),
        models: vec![],
        max_tokens: 32768,
        thinking_budgets: [
            ("low".to_string(), 4096),
            ("medium".to_string(), 16384),
            ("high".to_string(), 32768),
        ]
        .into_iter()
        .collect(),
        search_status_prefix: "Search results for query:".to_string(),
    }
}

/// All environment variables Config::load reads (plus the debug tee).
#[cfg(test)]
pub(crate) const ENV_KEYS: [&str; 10] = [
    "LISTEN_ADDR",
    "KIMI_BASE_URL",
    "KIMI_ANTHROPIC_BETA",
    "KIMI_MODEL_MAP",
    "KIMI_CLIENT_SOURCE",
    "KIMI_MODELS",
    "KIMI_MAX_TOKENS",
    "KIMI_THINKING_BUDGETS",
    "KIMI_SEARCH_STATUS_PREFIX",
    "KIMI_DEBUG_SSE_FILE",
];

/// Serializes tests that mutate process env (Config::load reads it).
#[cfg(test)]
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Removes every adapter env var so each test starts from a clean slate.
/// Call again at test end (or rely on the lock + next test's clear).
#[cfg(test)]
pub(crate) fn clear_env() {
    for k in ENV_KEYS {
        // SAFETY: callers hold env_lock, so no concurrent env access from
        // this test binary's env-mutating tests.
        unsafe { std::env::remove_var(k) };
    }
}

/// Sets an env var; see clear_env for the safety contract.
#[cfg(test)]
pub(crate) fn set_env(key: &str, value: &str) {
    // SAFETY: callers hold env_lock.
    unsafe { std::env::set_var(key, value) };
}
