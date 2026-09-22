//! Chat Completions streaming contract tests (#117).
//!
//! Case IDs: chat.stream.basic, chat.stream.usage, chat.stream.done,
//! chat.stream.chunk_split, chat.stream.incomplete, chat.stream.upstream_error.
//!
//! All streaming tests verify SSE content-type, parseable chunks, delta
//! ordering, finish_reason placement, usage semantics and [DONE] termination.

use axum::http::StatusCode;
use my_ai_gateway::test_support::test_gateway_router;
use serde_json::json;

use crate::assert_case;
use crate::common::*;

const MODEL: &str = "test-model";
const URI: &str = "/v1/chat/completions";

fn stream_body(model: &str) -> String {
    json!({
        "model": model,
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true
    })
    .to_string()
}

// ── chat.stream.basic ───────────────────────────────────────────────────────

#[tokio::test]
async fn chat_stream_basic() {
    assert_case!("chat.stream.basic");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "chat.stream.basic", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);
    assert!(!events.is_empty(), "stream must produce events");

    let json_events: Vec<_> = events.iter().filter(|e| !e.is_done()).collect();
    assert!(
        json_events.len() >= 3,
        "expected >=3 JSON events (role + content + finish), got {}",
        json_events.len()
    );

    for event in &json_events {
        let data = event.json();
        assert_eq!(data["object"], "chat.completion.chunk");
        assert!(data["id"].is_string(), "chunk must have id");
    }

    let first = json_events[0].json();
    assert_eq!(first["choices"][0]["delta"]["role"], "assistant");

    let mut content = String::new();
    for event in &json_events {
        let data = event.json();
        if let Some(c) = data["choices"][0]["delta"]["content"].as_str() {
            content.push_str(c);
        }
    }
    assert_eq!(content, "Hello world", "concatenated deltas");

    let last_json = json_events.last().unwrap().json();
    assert_eq!(
        last_json["choices"][0]["finish_reason"], "stop",
        "finish_reason in final JSON chunk"
    );

    let done_events: Vec<_> = events.iter().filter(|e| e.is_done()).collect();
    assert_eq!(done_events.len(), 1, "exactly one [DONE]");

    let done_idx = events.iter().position(|e| e.is_done()).unwrap();
    assert_eq!(done_idx, events.len() - 1, "[DONE] must be the last event");

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/chat/completions");
    assert!(requests[0].body["stream"] == true, "stream=true forwarded");
}

// ── chat.stream.usage ───────────────────────────────────────────────────────

#[tokio::test]
async fn chat_stream_usage() {
    assert_case!("chat.stream.usage");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "chat.stream.usage", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let json_events: Vec<_> = events.iter().filter(|e| !e.is_done()).collect();

    let usage_event = json_events
        .iter()
        .find(|e| !e.json()["usage"].is_null())
        .expect("at least one event must carry usage");

    let usage = &usage_event.json()["usage"];
    assert_eq!(usage["prompt_tokens"], 10);
    assert_eq!(usage["completion_tokens"], 2);
    assert_eq!(usage["total_tokens"], 12);
    assert_eq!(usage["prompt_tokens_details"]["cached_tokens"], 5);
}

// ── chat.stream.done (#83 regression) ───────────────────────────────────────

#[tokio::test]
async fn chat_stream_done_regression_83() {
    assert_case!("chat.stream.done");
    // #83: MiniMax Chat SSE omits [DONE]. Gateway must inject it so strict
    // OpenAI clients can detect end-of-stream.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "chat.stream.done", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let done_events: Vec<_> = events.iter().filter(|e| e.is_done()).collect();
    assert_eq!(
        done_events.len(),
        1,
        "#83 regression: gateway must inject [DONE] when upstream omits it"
    );

    let done_idx = events.iter().position(|e| e.is_done()).unwrap();
    assert_eq!(done_idx, events.len() - 1, "[DONE] must be the last event");

    let json_events: Vec<_> = events.iter().filter(|e| !e.is_done()).collect();
    let last_json = json_events.last().unwrap().json();
    assert_eq!(
        last_json["choices"][0]["finish_reason"], "stop",
        "finish_reason still present before injected [DONE]"
    );

    let mut content = String::new();
    for event in &json_events {
        let data = event.json();
        if let Some(c) = data["choices"][0]["delta"]["content"].as_str() {
            content.push_str(c);
        }
    }
    assert_eq!(content, "Response without DONE");
}

// ── chat.stream.chunk_split ─────────────────────────────────────────────────

#[tokio::test]
async fn chat_stream_chunk_split() {
    assert_case!("chat.stream.chunk_split");
    // SSE data lines split across TCP body chunks must reassemble correctly.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "chat.stream.split_lines", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let json_events: Vec<_> = events.iter().filter(|e| !e.is_done()).collect();
    assert!(
        json_events.len() >= 3,
        "split chunks must reassemble to same events"
    );

    for event in &json_events {
        let _data = event.json();
    }

    let done_events: Vec<_> = events.iter().filter(|e| e.is_done()).collect();
    assert_eq!(done_events.len(), 1, "[DONE] present after reassembly");
}

// ── chat.stream.incomplete ──────────────────────────────────────────────────

#[tokio::test]
async fn chat_stream_incomplete() {
    assert_case!("chat.stream.incomplete");
    // Incomplete stream (no finish_reason, no [DONE]) must still be handled
    // gracefully, with an error before the closing [DONE] frame.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(
        &router,
        URI,
        "common.error.incomplete_stream",
        &stream_body(MODEL),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);
    assert!(!events.is_empty(), "some events delivered before EOF");

    let json_events: Vec<_> = events.iter().filter(|e| !e.is_done()).collect();
    assert!(!json_events.is_empty(), "at least one content event");

    assert!(
        body.contains("gateway_incomplete_stream"),
        "truncation must not look successful"
    );
    let has_done = events.iter().any(|e| e.is_done());
    assert!(has_done, "gateway closes the error stream with [DONE]");
}

// ── chat.stream.upstream_error ──────────────────────────────────────────────

#[tokio::test]
async fn chat_stream_merged_events() {
    assert_case!("chat.stream.merged");
    // Multiple SSE events merged into a single TCP chunk must parse correctly.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "chat.stream.merged", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let json_events: Vec<_> = events.iter().filter(|e| !e.is_done()).collect();
    assert!(
        json_events.len() >= 3,
        "merged chunks must produce same event count"
    );

    let done_events: Vec<_> = events.iter().filter(|e| e.is_done()).collect();
    assert_eq!(done_events.len(), 1, "[DONE] present after merged chunk");
}
