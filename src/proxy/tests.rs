use std::time::Duration;

use super::attribution::{client_source_from_headers, known_client_user_agent};
use super::policy::{retry_after_delay, MAX_SAME_ACCOUNT_RETRY_AFTER};
use crate::auth::AuthIdentity;
use axum::http::{HeaderMap, HeaderValue};

fn header(headers: &mut HeaderMap, name: &'static str, value: &str) {
    headers.insert(name, HeaderValue::from_str(value).unwrap());
}

#[test]
fn retry_after_accepts_only_short_valid_delays() {
    let mut headers = HeaderMap::new();
    header(&mut headers, "retry-after", "2");
    assert_eq!(retry_after_delay(&headers), Some(Duration::from_secs(2)));
    header(&mut headers, "retry-after", "3");
    assert_eq!(retry_after_delay(&headers), None);
    header(&mut headers, "retry-after", "invalid");
    assert_eq!(retry_after_delay(&headers), None);
    assert_eq!(MAX_SAME_ACCOUNT_RETRY_AFTER, Duration::from_secs(2));
}

#[test]
fn explicit_x_client_source_takes_precedence_over_user_agent() {
    let mut headers = HeaderMap::new();
    header(&mut headers, "x-client-source", "my-business-app");
    header(&mut headers, "user-agent", "kimi-code-cli/1.2.3");
    assert_eq!(
        client_source_from_headers(&headers, None),
        "my-business-app"
    );
}

#[test]
fn empty_x_client_source_falls_through_to_user_agent() {
    let mut headers = HeaderMap::new();
    header(&mut headers, "x-client-source", "   ");
    header(&mut headers, "user-agent", "kimi-code-cli/1.2.3");
    assert_eq!(client_source_from_headers(&headers, None), "kimi-code-cli");
}

#[test]
fn user_agent_identifies_kimi_code_cli() {
    let mut headers = HeaderMap::new();
    header(&mut headers, "user-agent", "kimi-code-cli/1.2.3 (web)");
    assert_eq!(client_source_from_headers(&headers, None), "kimi-code-cli");
}

#[test]
fn user_agent_identifies_anthropic_sdk_across_languages() {
    let cases = [
        "Anthropic/Python 0.39.0",
        "AsyncAnthropic/Python 0.39.0",
        "Anthropic/JS 0.30.0",
        "Anthropic/Go 0.20.0",
    ];
    for ua in cases {
        let mut headers = HeaderMap::new();
        header(&mut headers, "user-agent", ua);
        assert_eq!(
            client_source_from_headers(&headers, None),
            "anthropic-sdk",
            "ua={ua}"
        );
    }
}

#[test]
fn user_agent_identifies_openai_sdk_across_languages() {
    let cases = [
        "OpenAI/Python 1.68.0",
        "AsyncOpenAI/Python 1.68.0",
        "OpenAI/Go 1.0.0",
    ];
    for ua in cases {
        let mut headers = HeaderMap::new();
        header(&mut headers, "user-agent", ua);
        assert_eq!(
            client_source_from_headers(&headers, None),
            "openai-sdk",
            "ua={ua}"
        );
    }
}

#[test]
fn user_agent_identifies_codex_cli() {
    let mut headers = HeaderMap::new();
    header(
        &mut headers,
        "user-agent",
        "codex_app_server_daemon/1.2.3 (Linux 6.8.0; x86_64) codex_cli_rs/1.2.3",
    );
    assert_eq!(client_source_from_headers(&headers, None), "codex-cli");
    let mut headers = HeaderMap::new();
    header(&mut headers, "user-agent", "codex_cli_rs/1.2.3");
    assert_eq!(client_source_from_headers(&headers, None), "codex-cli");
}

#[test]
fn unknown_user_agent_falls_back_to_unknown() {
    let mut headers = HeaderMap::new();
    header(&mut headers, "user-agent", "curl/8.7.1");
    assert_eq!(client_source_from_headers(&headers, None), "unknown");
}

#[test]
fn no_client_headers_at_all_returns_unknown() {
    let headers = HeaderMap::new();
    assert_eq!(client_source_from_headers(&headers, None), "unknown");
}

#[test]
fn known_client_user_agent_handles_product_with_paren_suffix() {
    assert_eq!(
        known_client_user_agent("kimi-code-cli/1.2.3 (web)"),
        Some("kimi-code-cli")
    );
    assert_eq!(
        known_client_user_agent("Anthropic/Python 0.39 (foo bar)"),
        Some("anthropic-sdk")
    );
    assert_eq!(known_client_user_agent(""), None);
    assert_eq!(known_client_user_agent("(no product)"), None);
}

#[test]
fn user_agent_identifies_kimi_code_vscode() {
    let mut headers = HeaderMap::new();
    header(&mut headers, "user-agent", "kimi-code/0.39.0");
    assert_eq!(client_source_from_headers(&headers, None), "kimi-code");
}

#[test]
fn user_agent_identifies_kimi_code_underscore() {
    let mut headers = HeaderMap::new();
    header(&mut headers, "user-agent", "kimi_code/0.39.0");
    assert_eq!(client_source_from_headers(&headers, None), "kimi-code");
}

#[test]
fn kimi_code_cli_takes_precedence_over_kimi_code() {
    let mut headers = HeaderMap::new();
    header(&mut headers, "user-agent", "kimi-code-cli/1.2.3 (web)");
    assert_eq!(client_source_from_headers(&headers, None), "kimi-code-cli");
}

#[test]
fn static_api_key_provides_default_client_source() {
    let headers = HeaderMap::new();
    let auth = AuthIdentity::StaticApiKey;
    assert_eq!(
        client_source_from_headers(&headers, Some(&auth)),
        "static_api_key"
    );
}

#[test]
fn virtual_key_provides_name_as_default_client_source() {
    let headers = HeaderMap::new();
    let auth = AuthIdentity::VirtualKey {
        id: 1,
        name: "my-app".into(),
        prefix: "gw_abc123".into(),
    };
    assert_eq!(client_source_from_headers(&headers, Some(&auth)), "my-app");
}

#[test]
fn virtual_key_falls_back_to_prefix() {
    let headers = HeaderMap::new();
    let auth = AuthIdentity::VirtualKey {
        id: 1,
        name: String::new(),
        prefix: "gw_abc123".into(),
    };
    assert_eq!(
        client_source_from_headers(&headers, Some(&auth)),
        "gw_abc123"
    );
}

#[test]
fn explicit_header_overrides_auth_identity() {
    let mut headers = HeaderMap::new();
    header(&mut headers, "x-client-source", "custom-client");
    let auth = AuthIdentity::VirtualKey {
        id: 1,
        name: "my-app".into(),
        prefix: "gw_abc123".into(),
    };
    assert_eq!(
        client_source_from_headers(&headers, Some(&auth)),
        "custom-client"
    );
}

#[test]
fn auth_identity_overrides_user_agent() {
    let mut headers = HeaderMap::new();
    header(&mut headers, "user-agent", "kimi-code-cli/1.2.3");
    let auth = AuthIdentity::VirtualKey {
        id: 1,
        name: "my-app".into(),
        prefix: "gw_abc123".into(),
    };
    assert_eq!(client_source_from_headers(&headers, Some(&auth)), "my-app");
}

#[test]
fn known_client_user_agent_does_not_match_unrelated_brands() {
    // Anthropic-Oxide / OpenAI-Compat must not be matched by the
    // `Anthropic/` / `OpenAI/` prefixes (those require a trailing `/`).
    assert_eq!(known_client_user_agent("Anthropic-Oxide/0.1"), None);
    assert_eq!(known_client_user_agent("OpenAI-Compat/1.0"), None);
}
