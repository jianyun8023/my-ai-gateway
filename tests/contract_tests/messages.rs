//! Anthropic Messages contract tests.
//!
//! Case IDs: messages.text.basic, messages.system.basic,
//! messages.multi_turn.basic, messages.usage.basic,
//! messages.thinking.shape,
//! messages.error.400, messages.error.401, messages.error.404.
//!
//! Protocol: protocol_in=anthropic_messages,
//!           protocol_upstream=anthropic_messages, mode=native.

use axum::http::StatusCode;
use my_ai_gateway::test_support::test_gateway_router;
use serde_json::json;

use crate::assert_case;
use crate::common::*;

const MODEL: &str = "test-model";
const URI: &str = "/v1/messages";

// ── messages.text.basic ─────────────────────────────────────────────────────

#[tokio::test]
async fn messages_text_basic() {
    assert_case!("messages.text.basic");
    // protocol_in=anthropic_messages, protocol_upstream=anthropic_messages, mode=native

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let response = gateway_anthropic_post(&router, "messages.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    assert_json_content_type(&response);

    let resp_body = json_body(response).await;
    // Anthropic Messages envelope
    assert_eq!(resp_body["type"], "message");
    assert_eq!(resp_body["role"], "assistant");
    assert!(resp_body["id"].is_string(), "response must have id");

    // Content blocks
    let content = resp_body["content"].as_array().expect("content array");
    assert!(!content.is_empty());
    assert_eq!(content[0]["type"], "text");
    assert_eq!(
        content[0]["text"],
        "Hello! This is a deterministic test response from the mock provider."
    );

    // Stop reason
    assert_eq!(resp_body["stop_reason"], "end_turn");

    // Usage
    assert_eq!(resp_body["usage"]["input_tokens"], 10);
    assert_eq!(resp_body["usage"]["output_tokens"], 15);

    // Upstream request mapping
    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    let upstream = &requests[0];
    assert_eq!(upstream.path, "/v1/messages");
    // Anthropic uses x-api-key, not Authorization: Bearer
    assert!(upstream.headers.x_api_key_present, "x-api-key forwarded");
    assert_eq!(upstream.body["model"], MODEL);
    assert_eq!(upstream.body["messages"][0]["role"], "user");
}

// ── messages.system.basic ───────────────────────────────────────────────────

#[tokio::test]
async fn messages_system_basic() {
    assert_case!("messages.system.basic");
    // Verifies system message is forwarded to upstream.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "max_tokens": 1024,
        "system": "You are a helpful assistant.",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let response = gateway_anthropic_post(&router, "messages.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;
    assert_eq!(resp_body["type"], "message");
    assert_eq!(resp_body["role"], "assistant");

    // Upstream must preserve system field
    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body["system"], "You are a helpful assistant.");
}

// ── messages.multi_turn.basic ───────────────────────────────────────────────

#[tokio::test]
async fn messages_multi_turn_basic() {
    assert_case!("messages.multi_turn.basic");
    // Verifies multi-turn conversation forwarding.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "What is 2+2?"},
            {"role": "assistant", "content": "4"},
            {"role": "user", "content": "And 3+3?"}
        ]
    });
    let response = gateway_anthropic_post(&router, "messages.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;
    assert_eq!(resp_body["type"], "message");

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    let messages = requests[0].body["messages"]
        .as_array()
        .expect("messages array");
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"], "And 3+3?");
}

// ── messages.usage.basic ────────────────────────────────────────────────────

#[tokio::test]
async fn messages_usage_basic() {
    assert_case!("messages.usage.basic");
    // Verifies usage fields in Anthropic format (input_tokens/output_tokens).

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Usage test"}]
    });
    let response = gateway_anthropic_post(&router, "messages.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let resp_body = json_body(response).await;
    assert!(resp_body["usage"].is_object(), "usage must be present");
    assert_eq!(resp_body["usage"]["input_tokens"], 10);
    assert_eq!(resp_body["usage"]["output_tokens"], 15);
}

// ── messages.thinking.shape ─────────────────────────────────────────────────

#[tokio::test]
async fn messages_thinking_shape() {
    assert_case!("messages.thinking.shape");
    // Verifies thinking block structure is preserved in native path.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Think step by step"}]
    });
    let response =
        gateway_anthropic_post(&router, "messages.thinking.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let resp_body = json_body(response).await;
    assert_eq!(resp_body["type"], "message");

    let content = resp_body["content"].as_array().expect("content array");
    assert!(content.len() >= 2, "thinking + text blocks expected");

    // First block: thinking
    assert_eq!(content[0]["type"], "thinking");
    assert!(
        content[0]["thinking"].is_string(),
        "thinking content preserved"
    );

    // Second block: text
    assert_eq!(content[1]["type"], "text");
    assert_eq!(content[1]["text"], "The answer is 42.");

    // Usage
    assert_eq!(resp_body["usage"]["input_tokens"], 20);
    assert_eq!(resp_body["usage"]["output_tokens"], 50);
}

// ── messages.error.400 ──────────────────────────────────────────────────────

#[tokio::test]
async fn messages_error_400() {
    assert_case!("messages.error.400");
    // Gateway rejects invalid JSON with Anthropic error envelope.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response =
        gateway_anthropic_post(&router, "messages.error.400", "not valid json {{{").await;

    assert_status(&response, StatusCode::BAD_REQUEST);
    assert_json_content_type(&response);

    let body = json_body(response).await;
    // Anthropic standard error envelope (#84 regression):
    // top-level type=error, error.type, error.message
    assert_eq!(
        body["type"], "error",
        "#84 regression: top-level type=error"
    );
    assert!(
        body["error"].is_object(),
        "#84 regression: error object present"
    );
    assert!(
        body["error"]["type"].is_string(),
        "#84 regression: error.type present"
    );
    assert_eq!(
        body["error"]["type"], "invalid_request_error",
        "#84 regression: error.type matches Anthropic standard"
    );
    assert!(
        body["error"]["message"].is_string(),
        "#84 regression: error.message present"
    );

    assert_eq!(mock.request_count(), 0);
}

// ── messages.error.401 ──────────────────────────────────────────────────────

#[tokio::test]
async fn messages_error_401() {
    assert_case!("messages.error.401");
    // Anthropic 401 error envelope.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    std::env::set_var("GATEWAY_API_KEY", "correct-test-key");
    let body = json!({
        "model": MODEL,
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let response = gateway_post_with_headers(
        &router,
        URI,
        "messages.error.401",
        &body.to_string(),
        vec![("authorization", "Bearer wrong-key")],
    )
    .await;
    std::env::remove_var("GATEWAY_API_KEY");

    assert_status(&response, StatusCode::UNAUTHORIZED);

    let resp_body = json_body(response).await;
    // Anthropic error envelope (#84 regression)
    assert_eq!(
        resp_body["type"], "error",
        "#84 regression: top-level type=error for 401"
    );
    assert!(resp_body["error"].is_object());
    assert_eq!(resp_body["error"]["type"], "authentication_error");
    assert!(resp_body["error"]["message"].is_string());

    assert_eq!(mock.request_count(), 0);
}

// ── messages.error.404 ──────────────────────────────────────────────────────

#[tokio::test]
async fn messages_error_404() {
    assert_case!("messages.error.404");
    // Anthropic 404 error envelope.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": "nonexistent-model",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let response = gateway_anthropic_post(&router, "messages.error.404", &body.to_string()).await;

    assert_status(&response, StatusCode::NOT_FOUND);

    let resp_body = json_body(response).await;
    // Anthropic error envelope (#84 regression)
    assert_eq!(
        resp_body["type"], "error",
        "#84 regression: top-level type=error for 404"
    );
    assert!(resp_body["error"].is_object());
    assert_eq!(resp_body["error"]["type"], "not_found_error");
    assert!(resp_body["error"]["message"].is_string());
    // request_id present
    assert!(
        resp_body.get("request_id").is_some(),
        "request_id in Anthropic error envelope"
    );

    assert_eq!(mock.request_count(), 0);
}
