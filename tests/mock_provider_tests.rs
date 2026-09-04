//! Integration tests for the MockProvider infrastructure itself.
//!
//! These tests prove that case dispatch, request recording, SSE chunk
//! split/merge, and fixture loading all work correctly before the mock
//! is consumed by higher-level contract tests (#116/#117).

mod support;

use std::time::Duration;

use axum::http::StatusCode;
use support::fixtures::{catalog, CaseFixture};
use support::mock_provider::{MockProvider, TEST_CASE_HEADER};
use support::sse::SseChunkPlan;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn get(url: &str, case_id: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(url)
        .header(TEST_CASE_HEADER, case_id)
        .send()
        .await
        .expect("HTTP request failed")
}

async fn post_json(url: &str, case_id: &str, body: &serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(url)
        .header(TEST_CASE_HEADER, case_id)
        .header("content-type", "application/json")
        .header("authorization", "Bearer sk-test-secret-key-12345")
        .json(body)
        .send()
        .await
        .expect("HTTP request failed")
}

// ===========================================================================
// 1. Case dispatch
// ===========================================================================

#[tokio::test]
async fn case_dispatch_returns_registered_fixture() {
    let mock = MockProvider::builder()
        .case(
            "chat.text.basic",
            CaseFixture::json(
                "chat.text.basic",
                StatusCode::OK,
                r#"{"id":"test-123","object":"chat.completion"}"#,
            ),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "chat.text.basic").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "test-123");
}

#[tokio::test]
async fn case_dispatch_returns_501_for_unknown_case() {
    let mock = MockProvider::builder().build().spawn().await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "nonexistent.case").await;

    assert_eq!(resp.status(), 501);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("nonexistent.case"));
}

#[tokio::test]
async fn case_dispatch_returns_501_without_header() {
    let mock = MockProvider::builder().build().spawn().await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status(), 501);
}

#[tokio::test]
async fn default_response_used_when_case_not_found() {
    let mock = MockProvider::builder()
        .default_response(CaseFixture::json(
            "default",
            StatusCode::OK,
            r#"{"default":true}"#,
        ))
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/test", mock.base_url());
    let resp = get(&url, "any.case").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["default"], true);
}

#[tokio::test]
async fn case_dispatch_supports_multiple_cases() {
    let mock = MockProvider::builder()
        .case(
            "chat.text.basic",
            CaseFixture::json("chat.text.basic", StatusCode::OK, r#"{"case":"chat"}"#),
        )
        .case(
            "common.error.429",
            CaseFixture::error(
                "common.error.429",
                StatusCode::TOO_MANY_REQUESTS,
                r#"{"error":"rate_limit"}"#,
            ),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());

    let resp1 = get(&url, "chat.text.basic").await;
    assert_eq!(resp1.status(), 200);

    let resp2 = get(&url, "common.error.429").await;
    assert_eq!(resp2.status(), 429);
}

// ===========================================================================
// 2. Request recording
// ===========================================================================

#[tokio::test]
async fn request_recorder_captures_method_path_and_body() {
    let mock = MockProvider::builder()
        .case(
            "chat.text.basic",
            CaseFixture::json("chat.text.basic", StatusCode::OK, r#"{"ok":true}"#),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let body = serde_json::json!({"model": "test-model", "messages": [{"role": "user", "content": "hello"}]});
    let _resp = post_json(&url, "chat.text.basic", &body).await;

    let requests = mock.requests();
    assert_eq!(requests.len(), 1);

    let req = &requests[0];
    assert_eq!(req.method, "POST");
    assert_eq!(req.path, "/v1/chat/completions");
    assert_eq!(req.body["model"], "test-model");
    assert_eq!(req.arrival_order, 0);
}

#[tokio::test]
async fn request_recorder_sanitizes_authorization() {
    let mock = MockProvider::builder()
        .case(
            "chat.text.basic",
            CaseFixture::json("chat.text.basic", StatusCode::OK, r#"{"ok":true}"#),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let body = serde_json::json!({"model":"m"});
    let _resp = post_json(&url, "chat.text.basic", &body).await;

    let requests = mock.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].headers.authorization_present);
    assert!(!requests[0].headers.extra.contains_key("authorization"));
}

#[tokio::test]
async fn request_recorder_tracks_arrival_order() {
    let mock = MockProvider::builder()
        .case(
            "chat.text.basic",
            CaseFixture::json("chat.text.basic", StatusCode::OK, r#"{"ok":true}"#),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    for _ in 0..3 {
        get(&url, "chat.text.basic").await;
    }

    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].arrival_order, 0);
    assert_eq!(requests[1].arrival_order, 1);
    assert_eq!(requests[2].arrival_order, 2);
}

#[tokio::test]
async fn take_requests_clears_the_buffer() {
    let mock = MockProvider::builder()
        .case(
            "chat.text.basic",
            CaseFixture::json("chat.text.basic", StatusCode::OK, r#"{"ok":true}"#),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    get(&url, "chat.text.basic").await;

    let taken = mock.take_requests();
    assert_eq!(taken.len(), 1);

    let remaining = mock.requests();
    assert!(remaining.is_empty());
}

// ===========================================================================
// 3. SSE streaming + chunk strategies
// ===========================================================================

#[tokio::test]
async fn sse_stream_delivers_complete_event_data() {
    let sse_body =
        "data: {\"content\":\"hello\"}\n\ndata: {\"content\":\"world\"}\n\ndata: [DONE]\n\n";
    let mock = MockProvider::builder()
        .case(
            "chat.stream.basic",
            CaseFixture::sse(
                "chat.stream.basic",
                StatusCode::OK,
                sse_body,
                SseChunkPlan::per_event(),
            ),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "chat.stream.basic").await;

    assert_eq!(resp.status(), 200);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("text/event-stream"));

    let body = resp.text().await.unwrap();
    assert!(body.contains("data: {\"content\":\"hello\"}"));
    assert!(body.contains("data: [DONE]"));
}

#[tokio::test]
async fn sse_split_lines_reassembles_correctly() {
    let sse_body =
        "data: {\"id\":\"test\",\"content\":\"long data line for split testing\"}\n\ndata: [DONE]\n\n";
    let mock = MockProvider::builder()
        .case(
            "chat.stream.split",
            CaseFixture::sse(
                "chat.stream.split",
                StatusCode::OK,
                sse_body,
                SseChunkPlan::split_data_lines(3),
            ),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "chat.stream.split").await;
    let body = resp.text().await.unwrap();

    assert_eq!(body, sse_body, "reassembled body must match original");
}

#[tokio::test]
async fn sse_merge_events_delivers_combined_chunk() {
    let sse_body = "data: {\"a\":1}\n\ndata: {\"b\":2}\n\ndata: {\"c\":3}\n\n";
    let mock = MockProvider::builder()
        .case(
            "chat.stream.merge",
            CaseFixture::sse(
                "chat.stream.merge",
                StatusCode::OK,
                sse_body,
                SseChunkPlan::merge_events(2),
            ),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "chat.stream.merge").await;
    let body = resp.text().await.unwrap();

    assert_eq!(body, sse_body, "all events must be present");
}

// ===========================================================================
// 4. Error and fault scenarios
// ===========================================================================

#[tokio::test]
async fn error_fixtures_return_correct_status_codes() {
    let cases = vec![
        ("common.error.400", 400u16),
        ("common.error.401", 401),
        ("common.error.403", 403),
        ("common.error.404", 404),
        ("common.error.429", 429),
        ("common.error.500", 500),
        ("common.error.502", 502),
        ("common.error.503", 503),
    ];

    let mock = MockProvider::builder()
        .case("common.error.400", catalog::error_400())
        .case("common.error.401", catalog::error_401())
        .case("common.error.403", catalog::error_403())
        .case("common.error.404", catalog::error_404())
        .case("common.error.429", catalog::error_429())
        .case("common.error.500", catalog::error_500())
        .case("common.error.502", catalog::error_502())
        .case("common.error.503", catalog::error_503())
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    for (case_id, expected_status) in cases {
        let resp = get(&url, case_id).await;
        assert_eq!(
            resp.status().as_u16(),
            expected_status,
            "case {case_id} should return {expected_status}"
        );
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(
            body.get("error").is_some(),
            "case {case_id} should have error envelope"
        );
    }
}

#[tokio::test]
async fn error_429_includes_retry_after_header() {
    let mock = MockProvider::builder()
        .case("common.error.429", catalog::error_429())
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "common.error.429").await;

    assert_eq!(resp.status(), 429);
    assert_eq!(
        resp.headers().get("retry-after").unwrap().to_str().unwrap(),
        "1"
    );
}

#[tokio::test]
async fn invalid_json_fixture_returns_malformed_body() {
    let mock = MockProvider::builder()
        .case("common.error.invalid_json", catalog::error_invalid_json())
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "common.error.invalid_json").await;

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        serde_json::from_str::<serde_json::Value>(&body).is_err(),
        "body should be invalid JSON"
    );
}

#[tokio::test]
async fn empty_body_fixture_returns_empty_response() {
    let mock = MockProvider::builder()
        .case("common.error.empty_body", catalog::error_empty_body())
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "common.error.empty_body").await;

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.is_empty(), "body should be empty");
}

#[tokio::test]
async fn incomplete_stream_has_no_done_marker() {
    let mock = MockProvider::builder()
        .case(
            "common.error.incomplete_stream",
            catalog::error_incomplete_stream(),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "common.error.incomplete_stream").await;

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("data:"), "should have some SSE data");
    assert!(
        !body.contains("[DONE]"),
        "incomplete stream should not have [DONE]"
    );
}

#[tokio::test]
async fn timeout_fixture_delays_first_byte() {
    let mock = MockProvider::builder()
        .case(
            "common.error.timeout",
            CaseFixture::json(
                "common.error.timeout",
                StatusCode::OK,
                r#"{"id":"timeout"}"#,
            )
            .with_first_byte_delay(Duration::from_millis(500)),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let start = std::time::Instant::now();
    let resp = get(&url, "common.error.timeout").await;
    let elapsed = start.elapsed();

    assert_eq!(resp.status(), 200);
    assert!(
        elapsed >= Duration::from_millis(400),
        "should have delayed at least 400ms, took {elapsed:?}"
    );
}

// ===========================================================================
// 5. Fixture catalog loading (file-based fixtures)
// ===========================================================================

#[tokio::test]
async fn catalog_chat_text_basic_loads_and_serves() {
    let mock = MockProvider::builder()
        .case("chat.text.basic", catalog::chat_text_basic())
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "chat.text.basic").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(body["usage"]["prompt_tokens"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn catalog_responses_text_basic_loads_and_serves() {
    let mock = MockProvider::builder()
        .case("responses.text.basic", catalog::responses_text_basic())
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/responses", mock.base_url());
    let resp = get(&url, "responses.text.basic").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "response");
    assert_eq!(body["status"], "completed");
}

#[tokio::test]
async fn catalog_messages_text_basic_loads_and_serves() {
    let mock = MockProvider::builder()
        .case("messages.text.basic", catalog::messages_text_basic())
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/messages", mock.base_url());
    let resp = get(&url, "messages.text.basic").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "message");
    assert_eq!(body["stop_reason"], "end_turn");
}

#[tokio::test]
async fn catalog_chat_stream_has_done_terminator() {
    let mock = MockProvider::builder()
        .case("chat.stream.basic", catalog::chat_text_basic_stream())
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "chat.stream.basic").await;
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("data: [DONE]"),
        "Chat stream must end with [DONE]"
    );
}

#[tokio::test]
async fn catalog_responses_stream_has_completed_terminator() {
    let mock = MockProvider::builder()
        .case(
            "responses.stream.basic",
            catalog::responses_text_basic_stream(),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/responses", mock.base_url());
    let resp = get(&url, "responses.stream.basic").await;
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("event: response.completed"),
        "Responses stream must end with response.completed"
    );
}

#[tokio::test]
async fn catalog_messages_stream_has_message_stop_terminator() {
    let mock = MockProvider::builder()
        .case(
            "messages.stream.basic",
            catalog::messages_text_basic_stream(),
        )
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/messages", mock.base_url());
    let resp = get(&url, "messages.stream.basic").await;
    let body = resp.text().await.unwrap();

    assert!(
        body.contains("event: message_stop"),
        "Messages stream must end with message_stop"
    );
}

// ===========================================================================
// 6. Tool fixtures
// ===========================================================================

#[tokio::test]
async fn catalog_tool_fixtures_have_fixed_call_ids() {
    let mock = MockProvider::builder()
        .case("chat.tool.single", catalog::chat_tool_single())
        .case("chat.tool.parallel", catalog::chat_tool_parallel())
        .case("responses.tool.single", catalog::responses_tool_single())
        .case("messages.tool.single", catalog::messages_tool_single())
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());

    let resp = get(&url, "chat.tool.single").await;
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["choices"][0]["message"]["tool_calls"][0]["id"],
        "call_test_fixed_001"
    );

    let resp = get(&url, "chat.tool.parallel").await;
    let body: serde_json::Value = resp.json().await.unwrap();
    let tool_calls = body["choices"][0]["message"]["tool_calls"]
        .as_array()
        .unwrap();
    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_calls[0]["id"], "call_test_fixed_par_001");
    assert_eq!(tool_calls[1]["id"], "call_test_fixed_par_002");

    let url = format!("{}/v1/responses", mock.base_url());
    let resp = get(&url, "responses.tool.single").await;
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["output"][0]["call_id"], "call_test_fixed_001");

    let url = format!("{}/v1/messages", mock.base_url());
    let resp = get(&url, "messages.tool.single").await;
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["content"][0]["id"], "toolu_test_fixed_001");
}

// ===========================================================================
// 7. Reasoning / Thinking fixtures
// ===========================================================================

#[tokio::test]
async fn catalog_reasoning_and_thinking_fixtures_have_correct_shape() {
    let mock = MockProvider::builder()
        .case("chat.reasoning.basic", catalog::chat_reasoning())
        .case("responses.reasoning.basic", catalog::responses_reasoning())
        .case("messages.thinking.basic", catalog::messages_thinking())
        .build()
        .spawn()
        .await;

    let url = format!("{}/v1/chat/completions", mock.base_url());
    let resp = get(&url, "chat.reasoning.basic").await;
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["usage"]["completion_tokens_details"]["reasoning_tokens"]
            .as_u64()
            .unwrap()
            > 0
    );

    let url = format!("{}/v1/responses", mock.base_url());
    let resp = get(&url, "responses.reasoning.basic").await;
    let body: serde_json::Value = resp.json().await.unwrap();
    let output = body["output"].as_array().unwrap();
    assert!(
        output.iter().any(|o| o["type"] == "reasoning"),
        "should have reasoning output"
    );
    assert!(
        output.iter().any(|o| o["type"] == "message"),
        "should have message output after reasoning"
    );

    let url = format!("{}/v1/messages", mock.base_url());
    let resp = get(&url, "messages.thinking.basic").await;
    let body: serde_json::Value = resp.json().await.unwrap();
    let content = body["content"].as_array().unwrap();
    assert!(
        content.iter().any(|c| c["type"] == "thinking"),
        "should have thinking content block"
    );
    assert!(
        content.iter().any(|c| c["type"] == "text"),
        "should have text content block after thinking"
    );
}

// ===========================================================================
// 8. All first-batch fixtures load without panic
// ===========================================================================

#[tokio::test]
async fn all_first_batch_fixtures_load_and_spawn_successfully() {
    let cases = catalog::all_first_batch();
    assert!(
        cases.len() >= 25,
        "expected at least 25 fixture cases, got {}",
        cases.len()
    );

    let mock = MockProvider::spawn(cases).await;
    assert!(!mock.base_url().is_empty());
    assert_eq!(mock.request_count(), 0);
}

// ===========================================================================
// 9. Path-agnostic routing
// ===========================================================================

#[tokio::test]
async fn mock_accepts_any_path() {
    let mock = MockProvider::builder()
        .case(
            "test.path",
            CaseFixture::json("test.path", StatusCode::OK, r#"{"path":"ok"}"#),
        )
        .build()
        .spawn()
        .await;

    for path in [
        "/v1/chat/completions",
        "/v1/responses",
        "/v1/messages",
        "/any/arbitrary/path",
    ] {
        let url = format!("{}{}", mock.base_url(), path);
        let resp = get(&url, "test.path").await;
        assert_eq!(resp.status(), 200, "path {path} should succeed");
    }

    assert_eq!(mock.request_count(), 4);
    let requests = mock.requests();
    assert_eq!(requests[0].path, "/v1/chat/completions");
    assert_eq!(requests[1].path, "/v1/responses");
    assert_eq!(requests[2].path, "/v1/messages");
    assert_eq!(requests[3].path, "/any/arbitrary/path");
}
