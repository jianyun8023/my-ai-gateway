//! Reasoning and thinking contract tests (#117).
//!
//! Case IDs: responses.reasoning.separate_from_text,
//! messages.thinking.separate_from_text, reasoning.roundtrip.multi_turn,
//! reasoning.degraded.explicit, chat.reasoning.stream,
//! responses.reasoning.stream, messages.thinking.stream.
//!
//! Verifies reasoning/thinking is separate from final text, multi-turn
//! round-trip (#88 regression), and streaming reasoning events.

use axum::http::StatusCode;
use my_ai_gateway::test_support::test_gateway_router;
use serde_json::json;

use crate::assert_case;
use crate::common::*;

const MODEL: &str = "test-model";

// ── responses.reasoning.separate_from_text ──────────────────────────────────

#[tokio::test]
async fn responses_reasoning_separate_from_text() {
    assert_case!("responses.reasoning.separate_from_text");
    // Reasoning and text must be separate output items, not mixed.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": "Think step by step about 42"
    });
    let response = gateway_post(
        &router,
        "/v1/responses",
        "responses.reasoning.basic",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;

    let output = resp_body["output"].as_array().expect("output array");
    assert!(output.len() >= 2, "must have reasoning + message items");

    let reasoning_item = output
        .iter()
        .find(|o| o["type"] == "reasoning")
        .expect("reasoning item present");
    assert!(
        reasoning_item["summary"].is_array(),
        "reasoning has summary"
    );

    let message_item = output
        .iter()
        .find(|o| o["type"] == "message")
        .expect("message item present");
    let text = message_item["content"][0]["text"].as_str().unwrap_or("");
    assert_eq!(text, "The answer is 42.");

    let reasoning_text = reasoning_item["summary"][0]["text"].as_str().unwrap_or("");
    assert!(!reasoning_text.is_empty(), "reasoning summary has text");
    assert_ne!(reasoning_text, text, "reasoning and final text must differ");
}

// ── messages.thinking.separate_from_text ────────────────────────────────────

#[tokio::test]
async fn messages_thinking_separate_from_text() {
    assert_case!("messages.thinking.separate_from_text");
    // Thinking and text must be separate content blocks.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Think about 42"}],
        "max_tokens": 256
    });
    let response =
        gateway_anthropic_post(&router, "messages.thinking.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;

    let content = resp_body["content"].as_array().expect("content array");
    assert!(content.len() >= 2, "must have thinking + text blocks");

    let thinking_block = content
        .iter()
        .find(|c| c["type"] == "thinking")
        .expect("thinking block present");
    assert!(
        thinking_block["thinking"].is_string(),
        "thinking has content"
    );

    let text_block = content
        .iter()
        .find(|c| c["type"] == "text")
        .expect("text block present");
    assert_eq!(text_block["text"], "The answer is 42.");

    assert_ne!(
        thinking_block["thinking"].as_str().unwrap(),
        text_block["text"].as_str().unwrap(),
        "thinking and final text must differ"
    );
}

// ── chat.reasoning.stream ───────────────────────────────────────────────────

#[tokio::test]
async fn chat_reasoning_stream() {
    assert_case!("chat.reasoning.stream");
    // Chat streaming: reasoning tokens tracked in usage, not separate events.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Think step by step"}],
        "stream": true
    });
    let response = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.reasoning.stream",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body_text = text_body(response).await;
    let events = parse_sse_events(&body_text);
    let json_events: Vec<_> = events.iter().filter(|e| !e.is_done()).collect();

    let mut content = String::new();
    for event in &json_events {
        let data = event.json();
        if let Some(c) = data["choices"][0]["delta"]["content"].as_str() {
            content.push_str(c);
        }
    }
    assert_eq!(content, "The answer is 42.");

    let usage_event = json_events
        .iter()
        .find(|e| !e.json()["usage"].is_null())
        .expect("usage event present");
    let usage = &usage_event.json()["usage"];
    assert_eq!(
        usage["completion_tokens_details"]["reasoning_tokens"], 35,
        "reasoning_tokens in streaming usage"
    );
}

// ── responses.reasoning.stream ──────────────────────────────────────────────

#[tokio::test]
async fn responses_reasoning_stream() {
    assert_case!("responses.reasoning.stream");
    // Streaming reasoning must produce separate reasoning events before text.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": "Think step by step about 42",
        "stream": true
    });
    let response = gateway_post(
        &router,
        "/v1/responses",
        "responses.reasoning.stream",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body_text = text_body(response).await;
    let events = parse_sse_events(&body_text);
    let types = event_type_sequence(&events);

    assert!(
        types.contains(&"response.reasoning_summary_text.delta".to_string()),
        "must have reasoning delta events"
    );
    assert!(
        types.contains(&"response.output_text.delta".to_string()),
        "must have text delta events"
    );

    let first_reasoning_idx = types
        .iter()
        .position(|t| t == "response.reasoning_summary_text.delta")
        .unwrap();
    let first_text_idx = types
        .iter()
        .position(|t| t == "response.output_text.delta")
        .unwrap();
    assert!(
        first_reasoning_idx < first_text_idx,
        "reasoning events must precede text events"
    );

    let mut reasoning_text = String::new();
    let mut content_text = String::new();
    for event in &events {
        match event.event_type.as_deref() {
            Some("response.reasoning_summary_text.delta") => {
                if let Some(d) = event.json()["delta"].as_str() {
                    reasoning_text.push_str(d);
                }
            }
            Some("response.output_text.delta") => {
                if let Some(d) = event.json()["delta"].as_str() {
                    content_text.push_str(d);
                }
            }
            _ => {}
        }
    }
    assert_eq!(reasoning_text, "Let me think step by step...");
    assert_eq!(content_text, "The answer is 42.");
    assert_ne!(reasoning_text, content_text, "reasoning ≠ text");
}

// ── messages.thinking.stream ────────────────────────────────────────────────

#[tokio::test]
async fn messages_thinking_stream() {
    assert_case!("messages.thinking.stream");
    // Streaming thinking: thinking_delta events separate from text_delta.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Think about 42"}],
        "max_tokens": 256,
        "stream": true
    });
    let response =
        gateway_anthropic_post(&router, "messages.thinking.stream", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body_text = text_body(response).await;
    let events = parse_sse_events(&body_text);

    let block_starts: Vec<_> = events
        .iter()
        .filter(|e| e.event_type.as_deref() == Some("content_block_start"))
        .collect();
    assert!(
        block_starts.len() >= 2,
        "must have at least thinking + text content blocks"
    );

    let first_block = block_starts[0].json();
    assert_eq!(
        first_block["content_block"]["type"], "thinking",
        "first content block is thinking"
    );

    let second_block = block_starts[1].json();
    assert_eq!(
        second_block["content_block"]["type"], "text",
        "second content block is text"
    );

    let mut thinking_text = String::new();
    let mut content_text = String::new();
    for event in &events {
        if event.event_type.as_deref() == Some("content_block_delta") {
            let data = event.json();
            match data["delta"]["type"].as_str() {
                Some("thinking_delta") => {
                    if let Some(t) = data["delta"]["thinking"].as_str() {
                        thinking_text.push_str(t);
                    }
                }
                Some("text_delta") => {
                    if let Some(t) = data["delta"]["text"].as_str() {
                        content_text.push_str(t);
                    }
                }
                _ => {}
            }
        }
    }
    assert_eq!(thinking_text, "Let me think about this step by step...");
    assert_eq!(content_text, "The answer is 42.");
    assert_ne!(thinking_text, content_text, "thinking ≠ text");

    let has_message_stop = events
        .iter()
        .any(|e| e.event_type.as_deref() == Some("message_stop"));
    assert!(has_message_stop, "message_stop present");
}

// ── reasoning.roundtrip.multi_turn (#88 regression) ─────────────────────────

#[tokio::test]
async fn reasoning_multi_turn_regression_88() {
    assert_case!("reasoning.roundtrip.multi_turn");
    // #88: Multi-turn reasoning must be forwarded in the conversation history.
    // Verifies that reasoning/thinking content from previous turns is included
    // in the upstream request for the next turn.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    // Messages protocol: multi-turn with thinking in history
    let body = json!({
        "model": MODEL,
        "messages": [
            {"role": "user", "content": "Think about 42"},
            {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "Previous thinking content..."},
                    {"type": "text", "text": "The answer is 42."}
                ]
            },
            {"role": "user", "content": "Are you sure?"}
        ],
        "max_tokens": 256
    });
    let response =
        gateway_anthropic_post(&router, "messages.thinking.basic", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);

    let upstream_body = &requests[0].body;
    let messages = upstream_body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3, "all three messages forwarded");

    let assistant_msg = &messages[1];
    assert_eq!(assistant_msg["role"], "assistant");
    let content = assistant_msg["content"].as_array().unwrap();

    let has_thinking = content.iter().any(|c| c["type"] == "thinking");
    assert!(
        has_thinking,
        "#88 regression: thinking content must be preserved in multi-turn history"
    );

    let has_text = content.iter().any(|c| c["type"] == "text");
    assert!(has_text, "text content preserved alongside thinking");
}

// ── reasoning.roundtrip.multi_turn — Responses ──────────────────────────────

#[tokio::test]
async fn responses_reasoning_multi_turn() {
    assert_case!("reasoning.roundtrip.multi_turn");
    // Responses protocol: reasoning items in previous turn input.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": [
            {"type": "message", "role": "user", "content": "Think about 42"},
            {
                "type": "reasoning",
                "id": "rs_prev_001",
                "summary": [{"type": "summary_text", "text": "Previous reasoning..."}]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "The answer is 42."}]
            },
            {"type": "message", "role": "user", "content": "Are you sure?"}
        ]
    });
    let response = gateway_post(
        &router,
        "/v1/responses",
        "responses.reasoning.basic",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);

    let upstream_body = &requests[0].body;
    let input = upstream_body["input"].as_array().unwrap();
    let has_reasoning = input.iter().any(|i| i["type"] == "reasoning");
    assert!(
        has_reasoning,
        "#88 regression: reasoning items must be preserved in Responses multi-turn input"
    );
}

// ── Chat reasoning shape in non-streaming ───────────────────────────────────

#[tokio::test]
async fn chat_reasoning_usage_tokens() {
    assert_case!("chat.reasoning.usage_tokens");
    // Verify reasoning tokens are tracked in usage for non-streaming Chat.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Think step by step"}]
    });
    let response = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.reasoning.basic",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;

    assert_eq!(resp_body["usage"]["completion_tokens"], 50);
    assert_eq!(
        resp_body["usage"]["completion_tokens_details"]["reasoning_tokens"], 35,
        "reasoning_tokens in non-streaming usage"
    );

    assert_eq!(
        resp_body["choices"][0]["message"]["content"], "The answer is 42.",
        "final text content preserved"
    );
}
