//! Anthropic Messages streaming contract tests (#117).
//!
//! Case IDs: messages.stream.basic, messages.stream.event_order,
//! messages.stream.usage, messages.stream.message_stop,
//! messages.stream.chunk_split, messages.stream.incomplete.
//!
//! Verifies the message_start → content_block_start/delta/stop →
//! message_delta → message_stop state machine.

use axum::http::StatusCode;
use my_ai_gateway::test_support::test_gateway_router;
use serde_json::json;

use crate::assert_case;
use crate::common::*;

const MODEL: &str = "test-model";

fn stream_body(model: &str) -> String {
    json!({
        "model": model,
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 256,
        "stream": true
    })
    .to_string()
}

// ── messages.stream.basic ───────────────────────────────────────────────────

#[tokio::test]
async fn messages_stream_basic() {
    assert_case!("messages.stream.basic");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response =
        gateway_anthropic_post(&router, "messages.stream.basic", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);
    assert!(!events.is_empty(), "stream must produce events");

    for event in &events {
        assert!(
            event.event_type.is_some(),
            "Messages SSE events must have event: type"
        );
    }

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/messages");
}

// ── messages.stream.event_order ─────────────────────────────────────────────

#[tokio::test]
async fn messages_stream_event_order() {
    assert_case!("messages.stream.event_order");
    // Verify the Anthropic SSE state machine:
    // message_start → content_block_start → content_block_delta* →
    // content_block_stop → message_delta → message_stop

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response =
        gateway_anthropic_post(&router, "messages.stream.basic", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);
    let types = event_type_sequence(&events);

    assert_eq!(
        types.first().map(|s| s.as_str()),
        Some("message_start"),
        "stream must start with message_start"
    );

    assert_eq!(
        types.last().map(|s| s.as_str()),
        Some("message_stop"),
        "stream must end with message_stop"
    );

    let msg_start_idx = types.iter().position(|t| t == "message_start").unwrap();
    let block_start_idx = types
        .iter()
        .position(|t| t == "content_block_start")
        .expect("must have content_block_start");
    let block_stop_idx = types
        .iter()
        .position(|t| t == "content_block_stop")
        .expect("must have content_block_stop");
    let msg_delta_idx = types
        .iter()
        .position(|t| t == "message_delta")
        .expect("must have message_delta");
    let msg_stop_idx = types.iter().position(|t| t == "message_stop").unwrap();

    assert!(
        msg_start_idx < block_start_idx,
        "message_start before content_block_start"
    );
    assert!(
        block_start_idx < block_stop_idx,
        "content_block_start before content_block_stop"
    );
    assert!(
        block_stop_idx < msg_delta_idx,
        "content_block_stop before message_delta"
    );
    assert!(
        msg_delta_idx < msg_stop_idx,
        "message_delta before message_stop"
    );

    let has_delta = types.contains(&"content_block_delta".to_string());
    assert!(has_delta, "must have at least one content_block_delta");

    let first_delta_idx = types
        .iter()
        .position(|t| t == "content_block_delta")
        .unwrap();
    assert!(
        first_delta_idx > block_start_idx && first_delta_idx < block_stop_idx,
        "content_block_delta between start and stop"
    );
}

// ── messages.stream.usage ───────────────────────────────────────────────────

#[tokio::test]
async fn messages_stream_usage() {
    assert_case!("messages.stream.usage");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response =
        gateway_anthropic_post(&router, "messages.stream.basic", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let msg_start = events
        .iter()
        .find(|e| e.event_type.as_deref() == Some("message_start"))
        .expect("must have message_start");
    let start_data = msg_start.json();
    let input_usage = &start_data["message"]["usage"];
    assert_eq!(
        input_usage["input_tokens"], 10,
        "input_tokens in message_start"
    );

    let msg_delta = events
        .iter()
        .find(|e| e.event_type.as_deref() == Some("message_delta"))
        .expect("must have message_delta");
    let delta_data = msg_delta.json();
    let output_usage = &delta_data["usage"];
    assert_eq!(
        output_usage["output_tokens"], 2,
        "output_tokens in message_delta"
    );
}

// ── messages.stream.message_stop ────────────────────────────────────────────

#[tokio::test]
async fn messages_stream_message_stop() {
    assert_case!("messages.stream.message_stop");
    // message_stop is the terminal event — it must be present.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response =
        gateway_anthropic_post(&router, "messages.stream.basic", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let has_message_stop = events
        .iter()
        .any(|e| e.event_type.as_deref() == Some("message_stop"));
    assert!(has_message_stop, "stream must contain message_stop");

    let msg_delta = events
        .iter()
        .find(|e| e.event_type.as_deref() == Some("message_delta"))
        .expect("must have message_delta");
    let delta_data = msg_delta.json();
    assert_eq!(
        delta_data["delta"]["stop_reason"], "end_turn",
        "stop_reason in message_delta"
    );
}

// ── messages.stream.chunk_split ─────────────────────────────────────────────

#[tokio::test]
async fn messages_stream_chunk_split() {
    assert_case!("messages.stream.chunk_split");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response =
        gateway_anthropic_post(&router, "messages.stream.split_lines", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);
    let types = event_type_sequence(&events);

    assert!(
        types.contains(&"message_start".to_string()),
        "message_start after chunk reassembly"
    );
    assert!(
        types.contains(&"message_stop".to_string()),
        "message_stop after chunk reassembly"
    );
}

// ── messages.stream.incomplete ──────────────────────────────────────────────

#[tokio::test]
async fn messages_stream_incomplete() {
    assert_case!("messages.stream.incomplete");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let response =
        gateway_anthropic_post(&router, "messages.stream.incomplete", &stream_body(MODEL)).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body = text_body(response).await;
    let events = parse_sse_events(&body);

    let has_message_stop = events
        .iter()
        .any(|e| e.event_type.as_deref() == Some("message_stop"));
    assert!(
        !has_message_stop,
        "incomplete stream must NOT have message_stop from upstream data"
    );

    let has_error_or_gateway = events.iter().any(|e| {
        let is_error_type = e.event_type.as_deref() == Some("error");
        let data_has_gateway = e.data.contains("gateway_upstream_error")
            || e.data.contains("gateway_empty_stream")
            || e.data.contains("gateway_");
        is_error_type || data_has_gateway
    });
    assert!(
        has_error_or_gateway,
        "gateway must inject error/gateway event for incomplete Messages stream"
    );
}
