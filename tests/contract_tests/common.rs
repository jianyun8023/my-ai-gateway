//! Shared helpers for protocol contract tests.

use std::collections::HashMap;

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use my_ai_gateway::test_support::{
    AccountConfig, Capabilities, GatewayConfig, Protocol, ProtocolCapability, ProviderConfig,
    RouteConfig,
};

use crate::support::fixtures::{catalog, CaseFixture};
use crate::support::mock_provider::{MockProvider, MockProviderBuilder, TEST_CASE_HEADER};

// ── Gateway config builders ─────────────────────────────────────────────────

/// Build a `GatewayConfig` for a **native** three-protocol provider.
///
/// The provider `base_url` points at the given mock provider URL, and the
/// model is configured with native support for all three protocols.
pub fn native_config(mock_base_url: &str, model: &str) -> GatewayConfig {
    let provider = ProviderConfig {
        id: "test-provider".into(),
        name: "Test Provider".into(),
        base_url: mock_base_url.into(),
        models: vec![model.into()],
        native_protocols: vec![
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ],
        endpoints: HashMap::from([
            (
                Protocol::OpenAiChatCompletions,
                "/v1/chat/completions".into(),
            ),
            (Protocol::OpenAiResponses, "/v1/responses".into()),
            (Protocol::AnthropicMessages, "/v1/messages".into()),
        ]),
        capabilities: Capabilities::native(),
        protocol_capabilities: HashMap::new(),
        model_overrides: HashMap::new(),
    };
    let account = AccountConfig {
        id: "test-account".into(),
        provider_id: "test-provider".into(),
        display_name: "Test Account".into(),
        credential_env: None,
        credential_ciphertext: None,
        credential: Some("sk-test-mock-key".into()),
        enabled: true,
        weight: 100,
        protocol_capabilities: HashMap::new(),
        capabilities: None,
        model_overrides: HashMap::new(),
        model_map: HashMap::new(),
    };
    let route = RouteConfig {
        id: "test-native-route".into(),
        model: model.into(),
        provider_id: "test-provider".into(),
        protocols: vec![
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ],
        primary_account_id: "test-account".into(),
        fallback_accounts: vec![],
        strategy: "primary_then_weighted_fallback".into(),
        mode: "native".into(),
        adapter: None,
        allow_lossy_conversion: false,
    };
    GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        providers: vec![provider],
        accounts: vec![account],
        routes: vec![route],
    }
}

/// Build a `GatewayConfig` for **Kimi Responses Adapter** (convert path).
///
/// The provider is configured as native for Anthropic Messages and Chat
/// Completions, with Responses handled via `kimi_responses_adapter`.
pub fn kimi_adapter_config(mock_base_url: &str, model: &str) -> GatewayConfig {
    let provider = ProviderConfig {
        id: "kimi-provider".into(),
        name: "Kimi Provider".into(),
        base_url: mock_base_url.into(),
        models: vec![model.into()],
        native_protocols: vec![Protocol::OpenAiChatCompletions, Protocol::AnthropicMessages],
        endpoints: HashMap::from([
            (
                Protocol::OpenAiChatCompletions,
                "/v1/chat/completions".into(),
            ),
            (Protocol::AnthropicMessages, "/v1/messages".into()),
        ]),
        capabilities: Capabilities::native(),
        protocol_capabilities: HashMap::from([(
            Protocol::OpenAiResponses,
            ProtocolCapability::adapter(Protocol::AnthropicMessages, "kimi_responses_adapter"),
        )]),
        model_overrides: HashMap::new(),
    };
    let account = AccountConfig {
        id: "kimi-account".into(),
        provider_id: "kimi-provider".into(),
        display_name: "Kimi Account".into(),
        credential_env: None,
        credential_ciphertext: None,
        credential: Some("sk-test-kimi-key".into()),
        enabled: true,
        weight: 100,
        protocol_capabilities: HashMap::new(),
        capabilities: None,
        model_overrides: HashMap::new(),
        model_map: HashMap::new(),
    };
    let native_route = RouteConfig {
        id: "kimi-native-route".into(),
        model: model.into(),
        provider_id: "kimi-provider".into(),
        protocols: vec![Protocol::OpenAiChatCompletions, Protocol::AnthropicMessages],
        primary_account_id: "kimi-account".into(),
        fallback_accounts: vec![],
        strategy: "primary_then_weighted_fallback".into(),
        mode: "native".into(),
        adapter: None,
        allow_lossy_conversion: false,
    };
    let adapter_route = RouteConfig {
        id: "kimi-adapter-route".into(),
        model: model.into(),
        provider_id: "kimi-provider".into(),
        protocols: vec![Protocol::OpenAiResponses],
        primary_account_id: "kimi-account".into(),
        fallback_accounts: vec![],
        strategy: "primary_then_weighted_fallback".into(),
        mode: "adapter".into(),
        adapter: Some("kimi_responses_adapter".into()),
        allow_lossy_conversion: true,
    };
    GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        providers: vec![provider],
        accounts: vec![account],
        routes: vec![native_route, adapter_route],
    }
}

// ── MockProvider helpers ────────────────────────────────────────────────────

/// Spawn a MockProvider pre-loaded with the first-batch catalog fixtures.
pub async fn spawn_mock_with_catalog() -> MockProvider {
    MockProvider::spawn(catalog::all_first_batch()).await
}

/// Spawn a MockProvider with a single fixture as the default response.
pub async fn spawn_mock_with_default(fixture: CaseFixture) -> MockProvider {
    MockProvider::builder()
        .default_response(fixture)
        .build()
        .spawn()
        .await
}

/// Build a MockProvider with specific case fixtures plus an optional default.
#[allow(dead_code)]
pub fn mock_builder() -> MockProviderBuilder {
    MockProvider::builder()
}

// ── Request helpers ─────────────────────────────────────────────────────────

/// Send a POST request through the gateway Router and return the response.
pub async fn gateway_post(router: &Router, uri: &str, case_id: &str, body: &str) -> Response<Body> {
    gateway_post_with_headers(router, uri, case_id, body, vec![]).await
}

/// Send a POST request with extra headers through the gateway Router.
pub async fn gateway_post_with_headers(
    router: &Router,
    uri: &str,
    case_id: &str,
    body: &str,
    extra_headers: Vec<(&str, &str)>,
) -> Response<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header(TEST_CASE_HEADER, case_id);
    for (name, value) in extra_headers {
        builder = builder.header(name, value);
    }
    let request = builder
        .body(Body::from(body.to_owned()))
        .expect("build test request");
    router
        .clone()
        .oneshot(request)
        .await
        .expect("gateway response")
}

/// Send a POST request to the Anthropic Messages endpoint.
///
/// Anthropic uses `x-api-key` instead of `Authorization: Bearer`.
pub async fn gateway_anthropic_post(router: &Router, case_id: &str, body: &str) -> Response<Body> {
    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .header(TEST_CASE_HEADER, case_id)
        .body(Body::from(body.to_owned()))
        .expect("build anthropic request");
    router
        .clone()
        .oneshot(request)
        .await
        .expect("gateway response")
}

// ── Response helpers ────────────────────────────────────────────────────────

/// Read the response body as a JSON `Value`.
pub async fn json_body(response: Response<Body>) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse response JSON")
}

/// Assert HTTP status code.
pub fn assert_status(response: &Response<Body>, expected: StatusCode) {
    assert_eq!(
        response.status(),
        expected,
        "expected HTTP {expected}, got {}",
        response.status()
    );
}

/// Assert `content-type` header is `application/json`.
pub fn assert_json_content_type(response: &Response<Body>) {
    let ct = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("application/json"),
        "expected application/json content-type, got: {ct}"
    );
}

// ── Case ID macro for stable, searchable case IDs ───────────────────────────

/// Annotate a test with its case ID for traceability to the #114 matrix.
///
/// Usage: `assert_case!("chat.text.basic");` — emits the case ID so
/// `grep -r 'chat.text.basic'` finds the test.
#[macro_export]
macro_rules! assert_case {
    ($case_id:expr) => {
        // The case_id string appears at the call site, making it grep-able.
        // No runtime cost beyond the string literal.
        let _ = $case_id;
    };
}
