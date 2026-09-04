//! OpenAI Chat Completions contract tests.
//!
//! Case IDs: chat.text.basic, chat.system.basic, chat.multi_turn.basic,
//! chat.params.temperature, chat.params.max_tokens, chat.params.stop,
//! chat.usage.basic, chat.reasoning.shape,
//! chat.error.400, chat.error.401, chat.error.404.
//!
//! Protocol: protocol_in=openai_chat_completions,
//!           protocol_upstream=openai_chat_completions, mode=native.

use axum::http::StatusCode;
use my_ai_gateway::test_support::test_gateway_router;
use serde_json::json;

use crate::assert_case;
use crate::common::*;

const MODEL: &str = "test-model";
const URI: &str = "/v1/chat/completions";

// ── chat.text.basic ─────────────────────────────────────────────────────────

#[tokio::test]
async fn chat_text_basic() {
    assert_case!("chat.text.basic");
    // protocol_in=openai_chat_completions, protocol_upstream=openai_chat_completions, mode=native

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let response = gateway_post(&router, URI, "chat.text.basic", &body.to_string()).await;

    // Transport status
    assert_status(&response, StatusCode::OK);
    assert_json_content_type(&response);

    // Protocol envelope + semantic fields
    let body = json_body(response).await;
    assert_eq!(body["object"], "chat.completion");
    assert!(body["id"].is_string(), "response must have id");
    assert_eq!(body["choices"][0]["message"]["role"], "assistant");
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "Hello! This is a deterministic test response from the mock provider."
    );
    assert_eq!(body["choices"][0]["finish_reason"], "stop");

    // Usage
    assert_eq!(body["usage"]["prompt_tokens"], 10);
    assert_eq!(body["usage"]["completion_tokens"], 15);
    assert_eq!(body["usage"]["total_tokens"], 25);

    // Upstream request mapping
    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1, "exactly one upstream request");
    let upstream = &requests[0];
    assert_eq!(upstream.path, "/v1/chat/completions");
    assert_eq!(upstream.method, "POST");
    assert!(
        upstream.headers.authorization_present,
        "credential forwarded"
    );
    let upstream_body = &upstream.body;
    assert_eq!(upstream_body["model"], MODEL);
    assert_eq!(upstream_body["messages"][0]["role"], "user");
    assert_eq!(upstream_body["messages"][0]["content"], "Hello");
}

// ── chat.system.basic ───────────────────────────────────────────────────────

#[tokio::test]
async fn chat_system_basic() {
    assert_case!("chat.system.basic");
    // Verifies system message is forwarded to upstream unchanged.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Hello"}
        ]
    });
    let response = gateway_post(&router, URI, "chat.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;
    assert_eq!(resp_body["choices"][0]["message"]["role"], "assistant");
    assert_eq!(resp_body["choices"][0]["finish_reason"], "stop");

    // Upstream must preserve system message
    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    let upstream_body = &requests[0].body;
    assert_eq!(upstream_body["messages"][0]["role"], "system");
    assert_eq!(
        upstream_body["messages"][0]["content"],
        "You are a helpful assistant."
    );
    assert_eq!(upstream_body["messages"][1]["role"], "user");
}

// ── chat.multi_turn.basic ───────────────────────────────────────────────────

#[tokio::test]
async fn chat_multi_turn_basic() {
    assert_case!("chat.multi_turn.basic");
    // Verifies multi-turn conversation messages are forwarded intact.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [
            {"role": "user", "content": "What is 2+2?"},
            {"role": "assistant", "content": "4"},
            {"role": "user", "content": "And 3+3?"}
        ]
    });
    let response = gateway_post(&router, URI, "chat.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;
    assert_eq!(resp_body["choices"][0]["message"]["role"], "assistant");

    // Upstream must receive all three messages in order
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

// ── chat.params.temperature ─────────────────────────────────────────────────

#[tokio::test]
async fn chat_params_temperature() {
    assert_case!("chat.params.temperature");
    // Verifies temperature parameter is forwarded to upstream.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "temperature": 0.7
    });
    let response = gateway_post(&router, URI, "chat.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    let upstream_body = &requests[0].body;
    assert_eq!(upstream_body["temperature"], 0.7);
}

// ── chat.params.max_tokens ──────────────────────────────────────────────────

#[tokio::test]
async fn chat_params_max_tokens() {
    assert_case!("chat.params.max_tokens");
    // Verifies max_tokens parameter is forwarded to upstream.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 256
    });
    let response = gateway_post(&router, URI, "chat.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    let upstream_body = &requests[0].body;
    assert_eq!(upstream_body["max_tokens"], 256);
}

// ── chat.params.stop ────────────────────────────────────────────────────────

#[tokio::test]
async fn chat_params_stop() {
    assert_case!("chat.params.stop");
    // Verifies stop sequences are forwarded to upstream.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "stop": ["END", "\n\n"]
    });
    let response = gateway_post(&router, URI, "chat.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    let upstream_body = &requests[0].body;
    let stop = upstream_body["stop"].as_array().expect("stop array");
    assert_eq!(stop.len(), 2);
    assert_eq!(stop[0], "END");
    assert_eq!(stop[1], "\n\n");
}

// ── chat.usage.basic ────────────────────────────────────────────────────────

#[tokio::test]
async fn chat_usage_basic() {
    assert_case!("chat.usage.basic");
    // Verifies detailed usage fields including token breakdowns.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Test usage"}]
    });
    let response = gateway_post(&router, URI, "chat.usage.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let resp_body = json_body(response).await;
    assert_eq!(resp_body["usage"]["prompt_tokens"], 50);
    assert_eq!(resp_body["usage"]["completion_tokens"], 100);
    assert_eq!(resp_body["usage"]["total_tokens"], 150);
    // Detailed token breakdowns are preserved
    assert_eq!(
        resp_body["usage"]["prompt_tokens_details"]["cached_tokens"],
        10
    );
    assert_eq!(
        resp_body["usage"]["completion_tokens_details"]["reasoning_tokens"],
        20
    );
}

// ── chat.reasoning.shape ────────────────────────────────────────────────────

#[tokio::test]
async fn chat_reasoning_shape() {
    assert_case!("chat.reasoning.shape");
    // Verifies reasoning token shape is preserved in native path.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Think step by step"}]
    });
    let response = gateway_post(&router, URI, "chat.reasoning.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let resp_body = json_body(response).await;
    assert_eq!(
        resp_body["choices"][0]["message"]["content"],
        "The answer is 42."
    );
    assert_eq!(resp_body["usage"]["prompt_tokens"], 20);
    assert_eq!(resp_body["usage"]["completion_tokens"], 50);
    assert_eq!(
        resp_body["usage"]["completion_tokens_details"]["reasoning_tokens"],
        35
    );
}

// ── chat.error.400 ──────────────────────────────────────────────────────────

#[tokio::test]
async fn chat_error_400() {
    assert_case!("chat.error.400");
    // Gateway rejects invalid JSON with protocol-aware error envelope.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "chat.error.400", "not valid json {{{").await;

    assert_status(&response, StatusCode::BAD_REQUEST);
    assert_json_content_type(&response);

    let body = json_body(response).await;
    // OpenAI error envelope
    assert!(body["error"].is_object(), "error envelope present");
    assert_eq!(body["error"]["code"], "invalid_json");
    assert!(
        body["error"]["message"].is_string(),
        "error message present"
    );

    // Gateway should NOT have contacted upstream
    assert_eq!(mock.request_count(), 0, "no upstream request for bad JSON");
}

// ── chat.error.401 ──────────────────────────────────────────────────────────

#[tokio::test]
async fn chat_error_401() {
    assert_case!("chat.error.401");
    // Gateway rejects unauthorized requests with correct error envelope.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    // Set GATEWAY_API_KEY so auth is enforced, then send wrong key
    std::env::set_var("GATEWAY_API_KEY", "correct-test-key");
    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let response = gateway_post_with_headers(
        &router,
        URI,
        "chat.error.401",
        &body.to_string(),
        vec![("authorization", "Bearer wrong-key")],
    )
    .await;
    std::env::remove_var("GATEWAY_API_KEY");

    assert_status(&response, StatusCode::UNAUTHORIZED);
    assert_json_content_type(&response);

    let resp_body = json_body(response).await;
    assert!(resp_body["error"].is_object(), "error envelope present");
    assert_eq!(resp_body["error"]["code"], "unauthorized");
    assert!(
        resp_body["error"]["message"].is_string(),
        "error message present"
    );

    // No upstream request
    assert_eq!(mock.request_count(), 0);
}

// ── chat.error.404 ──────────────────────────────────────────────────────────

#[tokio::test]
async fn chat_error_404() {
    assert_case!("chat.error.404");
    // Gateway returns 404 with correct envelope when model has no route.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": "nonexistent-model",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let response = gateway_post(&router, URI, "chat.error.404", &body.to_string()).await;

    assert_status(&response, StatusCode::NOT_FOUND);
    assert_json_content_type(&response);

    let resp_body = json_body(response).await;
    assert!(resp_body["error"].is_object(), "error envelope present");
    assert_eq!(resp_body["error"]["code"], "route_not_found");
    assert!(
        resp_body["error"]["message"].is_string(),
        "error message present"
    );
    // x-request-id header is set
    // (OpenAI envelope includes request_id at top level)
    assert!(
        resp_body.get("request_id").is_some(),
        "request_id in OpenAI error envelope"
    );

    assert_eq!(mock.request_count(), 0);
}
