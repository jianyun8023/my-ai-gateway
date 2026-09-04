//! OpenAI Responses contract tests.
//!
//! Case IDs: responses.text.basic, responses.text.convert,
//! responses.instructions.basic, responses.multi_turn_or_input_array,
//! responses.usage.basic, responses.reasoning.shape,
//! responses.error.400, responses.error.401, responses.error.404.
//!
//! Native path:  protocol_in=openai_responses,
//!               protocol_upstream=openai_responses, mode=native.
//! Convert path: protocol_in=openai_responses,
//!               protocol_upstream=anthropic_messages,
//!               mode=convert (kimi_responses_adapter).

use axum::http::StatusCode;
use my_ai_gateway::test_support::test_gateway_router;
use serde_json::json;

use crate::assert_case;
use crate::common::*;
use crate::support::fixtures::CaseFixture;

const MODEL: &str = "test-model";
const URI: &str = "/v1/responses";

// ── responses.text.basic (native) ───────────────────────────────────────────

#[tokio::test]
async fn responses_text_basic() {
    assert_case!("responses.text.basic");
    // protocol_in=openai_responses, protocol_upstream=openai_responses, mode=native

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": "Hello"
    });
    let response = gateway_post(&router, URI, "responses.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    assert_json_content_type(&response);

    let resp_body = json_body(response).await;
    // Protocol envelope
    assert_eq!(resp_body["object"], "response");
    assert_eq!(resp_body["status"], "completed");
    assert!(resp_body["id"].is_string(), "response must have id");

    // Output structure: message item with output_text content
    let output = resp_body["output"].as_array().expect("output array");
    assert!(!output.is_empty(), "output must not be empty");
    assert_eq!(output[0]["type"], "message");
    assert_eq!(output[0]["role"], "assistant");
    let content = output[0]["content"].as_array().expect("content array");
    assert_eq!(content[0]["type"], "output_text");
    assert_eq!(
        content[0]["text"],
        "Hello! This is a deterministic test response from the mock provider."
    );

    // Usage
    assert_eq!(resp_body["usage"]["input_tokens"], 10);
    assert_eq!(resp_body["usage"]["output_tokens"], 15);
    assert_eq!(resp_body["usage"]["total_tokens"], 25);

    // Upstream request mapping
    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    let upstream = &requests[0];
    assert_eq!(upstream.path, "/v1/responses");
    assert!(
        upstream.headers.authorization_present,
        "credential forwarded"
    );
    assert_eq!(upstream.body["model"], MODEL);
}

// ── responses.text.convert (Kimi adapter) ───────────────────────────────────

#[tokio::test]
async fn responses_text_convert() {
    assert_case!("responses.text.convert");
    // protocol_in=openai_responses, protocol_upstream=anthropic_messages,
    // mode=convert (kimi_responses_adapter)

    // The adapter sends Anthropic Messages upstream; use default_response
    // so the mock returns the messages fixture regardless of x-test-case.
    let messages_fixture = CaseFixture::json(
        "messages.text.basic",
        StatusCode::OK,
        &crate::support::fixtures::load_fixture("messages/text_basic.json"),
    );
    let mock = spawn_mock_with_default(messages_fixture).await;
    let config = kimi_adapter_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": "Hello"
    });
    let response = gateway_post(&router, URI, "responses.text.convert", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    assert_json_content_type(&response);

    let resp_body = json_body(response).await;
    // The adapter converts Anthropic Messages → Responses format
    assert_eq!(resp_body["object"], "response");
    assert_eq!(resp_body["status"], "completed");

    // Output should contain converted message
    let output = resp_body["output"].as_array().expect("output array");
    assert!(!output.is_empty(), "output must not be empty");

    // Usage should be mapped from Anthropic format
    assert!(
        resp_body["usage"].is_object(),
        "usage must be present in convert path"
    );
    assert!(
        resp_body["usage"]["input_tokens"].is_number()
            || resp_body["usage"]["output_tokens"].is_number(),
        "usage tokens must be numeric"
    );

    // Upstream request goes to /v1/messages (Anthropic endpoint).
    // The adapter may also issue model-registry or health requests, so
    // filter to just the POST /v1/messages call.
    let requests = mock.take_requests();
    let messages_requests: Vec<_> = requests
        .iter()
        .filter(|r| r.method == "POST" && r.path == "/v1/messages")
        .collect();
    assert_eq!(
        messages_requests.len(),
        1,
        "adapter must send exactly one POST /v1/messages (got {} total requests)",
        requests.len()
    );
}

// ── responses.instructions.basic ────────────────────────────────────────────

#[tokio::test]
async fn responses_instructions_basic() {
    assert_case!("responses.instructions.basic");
    // Verifies `instructions` field is forwarded.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "instructions": "You are a concise assistant.",
        "input": "Hello"
    });
    let response = gateway_post(&router, URI, "responses.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].body["instructions"],
        "You are a concise assistant."
    );
}

// ── responses.multi_turn_or_input_array ─────────────────────────────────────

#[tokio::test]
async fn responses_multi_turn_or_input_array() {
    assert_case!("responses.multi_turn_or_input_array");
    // Verifies input as array is forwarded.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": [
            {"role": "user", "content": "What is 2+2?"},
            {"role": "assistant", "content": "4"},
            {"role": "user", "content": "And 3+3?"}
        ]
    });
    let response = gateway_post(&router, URI, "responses.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    let input = requests[0].body["input"].as_array().expect("input array");
    assert_eq!(input.len(), 3);
    assert_eq!(input[0]["role"], "user");
    assert_eq!(input[2]["content"], "And 3+3?");
}

// ── responses.usage.basic ───────────────────────────────────────────────────

#[tokio::test]
async fn responses_usage_basic() {
    assert_case!("responses.usage.basic");
    // Verifies usage fields in Responses format (input_tokens/output_tokens).

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": "Test usage"
    });
    let response = gateway_post(&router, URI, "responses.text.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let resp_body = json_body(response).await;
    assert!(resp_body["usage"].is_object(), "usage must be present");
    assert_eq!(resp_body["usage"]["input_tokens"], 10);
    assert_eq!(resp_body["usage"]["output_tokens"], 15);
    assert_eq!(resp_body["usage"]["total_tokens"], 25);
}

// ── responses.reasoning.shape ───────────────────────────────────────────────

#[tokio::test]
async fn responses_reasoning_shape() {
    assert_case!("responses.reasoning.shape");
    // Verifies reasoning output block structure is preserved.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": "Think step by step"
    });
    let response = gateway_post(&router, URI, "responses.reasoning.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let resp_body = json_body(response).await;
    let output = resp_body["output"].as_array().expect("output array");

    // First item should be reasoning, second message
    assert!(output.len() >= 2, "reasoning + message blocks expected");
    assert_eq!(output[0]["type"], "reasoning");
    assert_eq!(output[1]["type"], "message");

    // Usage preserved
    assert_eq!(resp_body["usage"]["input_tokens"], 20);
    assert_eq!(resp_body["usage"]["output_tokens"], 50);
}

// ── responses.error.400 ─────────────────────────────────────────────────────

#[tokio::test]
async fn responses_error_400() {
    assert_case!("responses.error.400");
    // Gateway rejects invalid JSON.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "responses.error.400", "not valid json {{{").await;

    assert_status(&response, StatusCode::BAD_REQUEST);
    assert_json_content_type(&response);

    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "invalid_json");
    assert!(body["error"]["message"].is_string());
    assert_eq!(mock.request_count(), 0);
}

// ── responses.error.401 ─────────────────────────────────────────────────────

#[tokio::test]
async fn responses_error_401() {
    assert_case!("responses.error.401");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    std::env::set_var("GATEWAY_API_KEY", "correct-test-key");
    let body = json!({"model": MODEL, "input": "Hello"});
    let response = gateway_post_with_headers(
        &router,
        URI,
        "responses.error.401",
        &body.to_string(),
        vec![("authorization", "Bearer wrong-key")],
    )
    .await;
    std::env::remove_var("GATEWAY_API_KEY");

    assert_status(&response, StatusCode::UNAUTHORIZED);
    let resp_body = json_body(response).await;
    assert_eq!(resp_body["error"]["code"], "unauthorized");
    assert_eq!(mock.request_count(), 0);
}

// ── responses.error.404 ─────────────────────────────────────────────────────

#[tokio::test]
async fn responses_error_404() {
    assert_case!("responses.error.404");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({"model": "nonexistent-model", "input": "Hello"});
    let response = gateway_post(&router, URI, "responses.error.404", &body.to_string()).await;

    assert_status(&response, StatusCode::NOT_FOUND);
    let resp_body = json_body(response).await;
    assert_eq!(resp_body["error"]["code"], "route_not_found");
    assert!(resp_body.get("request_id").is_some());
    assert_eq!(mock.request_count(), 0);
}
