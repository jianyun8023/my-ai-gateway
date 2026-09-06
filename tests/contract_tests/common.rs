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

// ── SSE parsing helpers ─────────────────────────────────────────────────────

/// A parsed SSE event from a streaming response.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SseEvent {
    pub event_type: Option<String>,
    pub data: String,
}

#[allow(dead_code)]
impl SseEvent {
    /// Parse the `data` field as JSON.
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.data).unwrap_or_else(|e| {
            panic!(
                "failed to parse SSE data as JSON: {e}\ndata: {}",
                &self.data
            )
        })
    }

    /// Check if this event is the `[DONE]` sentinel.
    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }
}

/// Parse an SSE body into structured events, filtering out comments
/// (e.g. `: gateway-heartbeat`) and empty lines.
#[allow(dead_code)]
pub fn parse_sse_events(text: &str) -> Vec<SseEvent> {
    let mut events = Vec::new();
    let mut current_event_type: Option<String> = None;
    let mut current_data: Vec<String> = Vec::new();

    for line in text.lines() {
        if line.starts_with(':') {
            continue;
        }
        if line.is_empty() {
            if !current_data.is_empty() {
                let data = current_data.join("\n");
                events.push(SseEvent {
                    event_type: current_event_type.take(),
                    data,
                });
                current_data.clear();
            }
            current_event_type = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("event: ") {
            current_event_type = Some(rest.to_string());
        } else if line.starts_with("event:") {
            current_event_type = Some(line[6..].trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data: ") {
            current_data.push(rest.to_string());
        } else if line.starts_with("data:") {
            current_data.push(line[5..].trim().to_string());
        }
    }
    if !current_data.is_empty() {
        events.push(SseEvent {
            event_type: current_event_type,
            data: current_data.join("\n"),
        });
    }

    events
}

/// Collect the response body as raw text (for SSE bodies).
pub async fn text_body(response: Response<Body>) -> String {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Assert `content-type` header starts with `text/event-stream`.
#[allow(dead_code)]
pub fn assert_sse_content_type(response: &Response<Body>) {
    let ct = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("text/event-stream"),
        "expected text/event-stream content-type, got: {ct}"
    );
}

/// Extract event type sequence from parsed SSE events (excluding [DONE]).
#[allow(dead_code)]
pub fn event_type_sequence(events: &[SseEvent]) -> Vec<String> {
    events
        .iter()
        .filter(|e| !e.is_done())
        .map(|e| e.event_type.clone().unwrap_or_else(|| "data".to_string()))
        .collect()
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
