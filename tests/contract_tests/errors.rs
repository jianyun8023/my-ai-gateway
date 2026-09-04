//! Cross-protocol error envelope regression tests.
//!
//! Validates that gateway-generated errors conform to protocol-specific
//! error envelopes. Includes #84 (Anthropic standard error envelope)
//! automated regression.
//!
//! The error tests in chat.rs / responses.rs / messages.rs test the
//! specific cases; this module provides additional cross-cutting assertions.

use axum::http::StatusCode;
use my_ai_gateway::test_support::test_gateway_router;
use serde_json::json;

use crate::assert_case;
use crate::common::*;

const MODEL: &str = "test-model";

// ── #84 regression: Anthropic error envelope structure ──────────────────────

/// Comprehensive regression for Issue #84: Anthropic Messages error responses
/// must follow the standard Anthropic API error contract.
///
/// Expected structure:
/// ```json
/// {
///   "type": "error",
///   "error": {
///     "type": "<sdk_standard>",    // e.g. invalid_request_error, authentication_error
///     "code": "<gateway_code>",    // e.g. invalid_json, unauthorized
///     "message": "<description>"
///   },
///   "request_id": "<uuid>"
/// }
/// ```
#[tokio::test]
async fn issue_84_anthropic_error_envelope_regression() {
    assert_case!("messages.error.envelope_regression");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    // Test 1: 400 Bad Request (invalid JSON)
    {
        let response = gateway_anthropic_post(&router, "error.400", "{{bad json").await;
        assert_status(&response, StatusCode::BAD_REQUEST);
        let body = json_body(response).await;

        assert_eq!(body["type"], "error", "#84: top-level type must be 'error'");
        assert!(body["error"].is_object(), "#84: error object required");
        assert_eq!(
            body["error"]["type"], "invalid_request_error",
            "#84: error.type must be Anthropic standard"
        );
        assert!(
            body["error"]["message"].is_string(),
            "#84: error.message required"
        );
        assert!(
            body.get("request_id").is_some(),
            "#84: request_id must be present"
        );
    }

    // Test 2: 404 Not Found (unknown route)
    {
        let body = json!({
            "model": "nonexistent-model",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "test"}]
        });
        let response = gateway_anthropic_post(&router, "error.404", &body.to_string()).await;
        assert_status(&response, StatusCode::NOT_FOUND);
        let resp_body = json_body(response).await;

        assert_eq!(
            resp_body["type"], "error",
            "#84: 404 top-level type must be 'error'"
        );
        assert_eq!(resp_body["error"]["type"], "not_found_error");
        assert!(resp_body["error"]["message"].is_string());
        assert!(resp_body.get("request_id").is_some());
    }

    // Test 3: 401 Unauthorized
    {
        std::env::set_var("GATEWAY_API_KEY", "correct-test-key");
        let body = json!({
            "model": MODEL,
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "test"}]
        });
        let response = gateway_post_with_headers(
            &router,
            "/v1/messages",
            "error.401",
            &body.to_string(),
            vec![("authorization", "Bearer wrong-key")],
        )
        .await;
        std::env::remove_var("GATEWAY_API_KEY");

        assert_status(&response, StatusCode::UNAUTHORIZED);
        let resp_body = json_body(response).await;

        assert_eq!(
            resp_body["type"], "error",
            "#84: 401 top-level type must be 'error'"
        );
        assert_eq!(resp_body["error"]["type"], "authentication_error");
        assert!(resp_body["error"]["message"].is_string());
    }

    assert_eq!(
        mock.request_count(),
        0,
        "error responses must not reach upstream"
    );
}

// ── OpenAI error envelope structure ─────────────────────────────────────────

/// Verifies OpenAI Chat Completions and Responses error envelopes.
///
/// Expected structure:
/// ```json
/// {
///   "error": {
///     "code": "<gateway_code>",
///     "type": "<gateway_code>",
///     "message": "<description>"
///   },
///   "request_id": "<uuid>"
/// }
/// ```
#[tokio::test]
async fn openai_error_envelope_structure() {
    assert_case!("openai.error.envelope_structure");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    // Chat Completions: 400
    {
        let response =
            gateway_post(&router, "/v1/chat/completions", "error.400", "{{bad json").await;
        assert_status(&response, StatusCode::BAD_REQUEST);
        let body = json_body(response).await;

        assert!(body["error"].is_object(), "error envelope present");
        assert_eq!(body["error"]["code"], "invalid_json");
        assert_eq!(body["error"]["type"], "invalid_json");
        assert!(body["error"]["message"].is_string());
        assert!(body.get("request_id").is_some(), "request_id present");
        // OpenAI does NOT have top-level "type": "error"
        assert!(
            body.get("type").is_none(),
            "OpenAI envelope must not have top-level type"
        );
    }

    // Responses: 404
    {
        let body = json!({"model": "nonexistent-model", "input": "test"});
        let response = gateway_post(&router, "/v1/responses", "error.404", &body.to_string()).await;
        assert_status(&response, StatusCode::NOT_FOUND);
        let resp_body = json_body(response).await;

        assert_eq!(resp_body["error"]["code"], "route_not_found");
        assert!(resp_body["error"]["message"].is_string());
        assert!(resp_body.get("request_id").is_some());
        assert!(
            resp_body.get("type").is_none(),
            "OpenAI envelope must not have top-level type"
        );
    }

    assert_eq!(mock.request_count(), 0);
}

// ── x-request-id header ────────────────────────────────────────────────────

#[tokio::test]
async fn error_responses_include_x_request_id_header() {
    assert_case!("common.error.x_request_id");
    // All protocol error responses should include x-request-id response header.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    // Chat 404
    let body = json!({"model": "missing", "messages": [{"role":"user","content":"x"}]});
    let response = gateway_post(&router, "/v1/chat/completions", "error", &body.to_string()).await;
    assert!(
        response.headers().get("x-request-id").is_some(),
        "Chat error must include x-request-id header"
    );

    // Responses 404
    let body = json!({"model": "missing", "input": "x"});
    let response = gateway_post(&router, "/v1/responses", "error", &body.to_string()).await;
    assert!(
        response.headers().get("x-request-id").is_some(),
        "Responses error must include x-request-id header"
    );

    // Messages 404
    let body =
        json!({"model": "missing", "max_tokens": 10, "messages": [{"role":"user","content":"x"}]});
    let response = gateway_anthropic_post(&router, "error", &body.to_string()).await;
    assert!(
        response.headers().get("x-request-id").is_some(),
        "Anthropic error must include x-request-id header"
    );
}

// ── No secret leakage in error responses ────────────────────────────────────

#[tokio::test]
async fn error_responses_do_not_leak_secrets() {
    assert_case!("common.error.no_secret_leak");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({"model": "missing", "messages": [{"role":"user","content":"x"}]});
    let response = gateway_post(&router, "/v1/chat/completions", "error", &body.to_string()).await;
    let resp_body = json_body(response).await;
    let serialized = resp_body.to_string();

    // Must not contain provider credentials or internal topology
    for secret in [
        "sk-test-mock-key",
        "GATEWAY_API_KEY",
        "test-provider",
        "test-account",
    ] {
        assert!(
            !serialized.contains(secret),
            "error response must not leak: {secret}"
        );
    }
}
