//! OpenAI Responses streaming contract tests (#117).
//!
//! Case IDs: responses.stream.basic, responses.stream.event_order,
//! responses.stream.text_delta, responses.stream.usage,
//! responses.stream.completed, responses.stream.chunk_split,
//! responses.stream.incomplete.
//!
//! Verifies event type sequence, not just final text concatenation.

use axum::http::StatusCode;
use my_ai_gateway::test_support::test_gateway_router;
use serde_json::json;

use crate::assert_case;
use crate::common::*;

const MODEL: &str = "test-model";
const URI: &str = "/v1/responses";

fn stream_body(model: &str) -> String {
    json!({
        "model": model,
        "input": "Hello",
        "stream": true
    })
    .to_string()
}

// ── responses.stream.basic ──────────────────────────────────────────────────

#[tokio::test]
async fn responses_stream_basic() {
    assert_case!("responses.stream.basic");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "responses.stream.basic", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);
    assert!(!events.is_empty(), "stream must produce events");

    for event in &events {
        assert!(
            event.event_type.is_some(),
            "Responses SSE events must have event: type"
        );
    }

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/responses");
}

// ── responses.stream.event_order ────────────────────────────────────────────

#[tokio::test]
async fn responses_stream_event_order() {
    assert_case!("responses.stream.event_order");
    // Verify the canonical event type sequence that the gateway forwards.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "responses.stream.basic", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);
    let types = event_type_sequence(&events);

    assert!(!types.is_empty(), "must have at least one event type");

    assert_eq!(
        types.first().map(|s| s.as_str()),
        Some("response.created"),
        "stream must start with response.created"
    );

    assert_eq!(
        types.last().map(|s| s.as_str()),
        Some("response.completed"),
        "stream must end with response.completed"
    );

    assert!(
        types.contains(&"response.output_item.added".to_string()),
        "must contain output_item.added"
    );
    assert!(
        types.contains(&"response.output_text.delta".to_string()),
        "must contain text delta events"
    );
    assert!(
        types.contains(&"response.output_item.done".to_string()),
        "must contain output_item.done"
    );

    let created_idx = types.iter().position(|t| t == "response.created").unwrap();
    let completed_idx = types
        .iter()
        .position(|t| t == "response.completed")
        .unwrap();
    assert!(
        created_idx < completed_idx,
        "created must precede completed"
    );
}

// ── responses.stream.text_delta ─────────────────────────────────────────────

#[tokio::test]
async fn responses_stream_text_delta() {
    assert_case!("responses.stream.text_delta");
    // Concatenate all text deltas and verify the final text.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "responses.stream.basic", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let mut text = String::new();
    for event in &events {
        if event.event_type.as_deref() == Some("response.output_text.delta") {
            let data = event.json();
            if let Some(delta) = data["delta"].as_str() {
                text.push_str(delta);
            }
        }
    }
    assert_eq!(text, "Hello world", "concatenated text deltas");

    let done_event = events
        .iter()
        .find(|e| e.event_type.as_deref() == Some("response.output_text.done"))
        .expect("must have output_text.done event");
    let done_data = done_event.json();
    assert_eq!(done_data["text"], "Hello world");
}

// ── responses.stream.usage ──────────────────────────────────────────────────

#[tokio::test]
async fn responses_stream_usage() {
    assert_case!("responses.stream.usage");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "responses.stream.basic", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let completed = events
        .iter()
        .find(|e| e.event_type.as_deref() == Some("response.completed"))
        .expect("must have response.completed event");

    let data = completed.json();
    let usage = &data["response"]["usage"];
    assert!(!usage.is_null(), "completed event must carry usage");
    assert_eq!(usage["input_tokens"], 10);
    assert_eq!(usage["output_tokens"], 2);
    assert_eq!(usage["total_tokens"], 12);
}

// ── responses.stream.completed ──────────────────────────────────────────────

#[tokio::test]
async fn responses_stream_completed() {
    assert_case!("responses.stream.completed");
    // The response.completed event must carry the full response object.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(&router, URI, "responses.stream.basic", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let completed = events
        .iter()
        .find(|e| e.event_type.as_deref() == Some("response.completed"))
        .expect("must have response.completed event");

    let data = completed.json();
    let resp = &data["response"];
    assert_eq!(resp["status"], "completed");
    assert!(resp["id"].is_string(), "completed response has id");
    assert!(resp["output"].is_array(), "completed response has output");
    assert!(!resp["usage"].is_null(), "completed response carries usage");
}

// ── responses.stream.chunk_split ────────────────────────────────────────────

#[tokio::test]
async fn responses_stream_chunk_split() {
    assert_case!("responses.stream.chunk_split");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(
        &router,
        URI,
        "responses.stream.split_lines",
        &stream_body(MODEL),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);
    let types = event_type_sequence(&events);

    assert!(
        types.contains(&"response.created".to_string()),
        "response.created after chunk reassembly"
    );
    assert!(
        types.contains(&"response.completed".to_string()),
        "response.completed after chunk reassembly"
    );
}

// ── responses.stream.incomplete ─────────────────────────────────────────────

#[tokio::test]
async fn responses_stream_incomplete() {
    assert_case!("responses.stream.incomplete");
    // Upstream closes without response.completed — gateway must not
    // report success; an error event should terminate the stream.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response = gateway_post(
        &router,
        URI,
        "responses.stream.incomplete",
        &stream_body(MODEL),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let has_completed = events
        .iter()
        .any(|e| e.event_type.as_deref() == Some("response.completed"));
    assert!(
        !has_completed,
        "incomplete stream must NOT have response.completed from upstream data"
    );

    let has_error_or_gateway = events.iter().any(|e| {
        let is_error_type = e.event_type.as_deref() == Some("error");
        let data_has_error = e.data.contains("gateway_incomplete_stream")
            || e.data.contains("gateway_empty_stream")
            || e.data.contains("gateway_");
        is_error_type || data_has_error
    });
    assert!(
        has_error_or_gateway,
        "gateway must inject an error/gateway event for incomplete Responses stream"
    );
}
