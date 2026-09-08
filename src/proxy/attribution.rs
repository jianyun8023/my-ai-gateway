use crate::auth::AuthIdentity;
use axum::http::HeaderMap;

pub(super) fn client_source_from_headers(
    headers: &HeaderMap,
    auth: Option<&AuthIdentity>,
) -> String {
    if let Some(value) = headers
        .get("x-client-source")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return value.to_owned();
    }
    if let Some(identity) = auth {
        return identity.default_client_source();
    }
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(value) = user_agent {
        if let Some(product) = known_client_user_agent(value) {
            return product.to_owned();
        }
    }
    if user_agent.is_some_and(|ua| !ua.is_empty()) {
        tracing::debug!(
            user_agent = user_agent.unwrap_or(""),
            "unrecognised User-Agent mapped to client_source=unknown"
        );
    }
    "unknown".to_owned()
}

/// Recognised upstream products and the canonical `client_source` value
/// they map to.  Prefixes are matched case-sensitively against the first
/// whitespace-delimited token of the User-Agent (parenthesised comments
/// stripped first).  Order matters only when one prefix is a prefix of
/// another — entries are kept short and distinct so the linear walk is
/// sufficient.
///
/// Sources verified against upstream source code:
///   * `Kimi Code CLI`  — `kimi-code-cli/<ver>` set in
///     `MoonshotAI/kimi-code/apps/kimi-code/src/constant/app.ts`
///     (`CLI_USER_AGENT_PRODUCT = "kimi-code-cli"`) and emitted by
///     `packages/oauth/src/identity.ts::createKimiUserAgent`.
///   * `Anthropic SDK Python`  — `Anthropic/Python <ver>` /
///     `AsyncAnthropic/Python <ver>` emitted by
///     `anthropics/anthropic-sdk-python/src/anthropic/_base_client.py`
///     (property `user_agent`).
///   * `Anthropic SDK JS`  — `Anthropic/JS <ver>` from
///     `anthropics/anthropic-sdk-typescript/src/client.ts` (`getUserAgent`).
///   * `Anthropic SDK Go`  — `Anthropic/Go <ver>` from
///     `anthropics/anthropic-sdk-go/internal/requestconfig/requestconfig.go`.
///   * `OpenAI SDK Python`  — `OpenAI/Python <ver>` /
///     `AsyncOpenAI/Python <ver>` from
///     `openai/openai-python/src/openai/_base_client.py`.
///   * `OpenAI SDK Go`  — `OpenAI/Go <ver>` from
///     `openai/openai-go/internal/requestconfig/requestconfig.go`.
///   * `OpenAI Codex CLI`  — `codex_app_server_daemon/<ver> (...) codex_cli_rs/<ver>`
///     from `openai/codex/codex-rs/app-server-daemon/src/client.rs`
///     (round-trip user-agent parser).
///
/// Entries without a verifiable upstream source are deliberately omitted
/// from this table; extend it only after confirming the format in source.
const KNOWN_CLIENT_USER_AGENTS: &[(&str, &str)] = &[
    // Kimi Code CLI / Kimi Code web UI — both ship `kimi-code-cli/<ver>`
    // (the web UI is the same product with a `(web)` suffix in the UA
    // parenthesised comment, which we strip before matching).
    ("kimi-code-cli", "kimi-code-cli"),
    // Kimi Code VS Code extension — ships `kimi-code/<ver>` without the
    // `-cli` suffix, or may appear as `kimi_code/<ver>`.
    ("kimi-code", "kimi-code"),
    ("kimi_code", "kimi-code"),
    // OpenAI Codex CLI — both product tokens seen in the daemon UA.
    ("codex_app_server_daemon", "codex-cli"),
    ("codex_cli_rs", "codex-cli"),
    // Official Anthropic SDKs — Stainless-generated, format `<Brand>/<Lang>`
    // with `Async<Brand>/<Lang>` for async clients.
    ("AsyncAnthropic/", "anthropic-sdk"),
    ("Anthropic/", "anthropic-sdk"),
    // Official OpenAI SDKs — same Stainless shape as Anthropic.
    ("AsyncOpenAI/", "openai-sdk"),
    ("OpenAI/", "openai-sdk"),
];

/// Map a User-Agent string to a stable `client_source` identifier when the
/// upstream product is recognised.  Unknown agents return `None` and the
/// caller falls back to `unknown`.
pub(super) fn known_client_user_agent(user_agent: &str) -> Option<&'static str> {
    let head = user_agent
        .split_once('(')
        .map(|(h, _)| h)
        .unwrap_or(user_agent);
    let product = head.split_whitespace().next()?.trim_end_matches('/');
    if product.is_empty() {
        return None;
    }
    for (prefix, label) in KNOWN_CLIENT_USER_AGENTS {
        if product == *prefix || product.starts_with(prefix) {
            return Some(*label);
        }
    }
    None
}
