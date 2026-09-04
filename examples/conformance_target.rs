//! Test-only conformance target: MockProvider + Gateway on ephemeral ports.
//!
//! Usage:
//!   cargo run --example conformance-target --features test-support
//!
//! Starts an in-process deterministic mock upstream **and** a fully wired
//! Gateway `Router` (no PostgreSQL, no real Provider).  Both bind to
//! `127.0.0.1:0` (OS-assigned ports).  On startup the process prints
//! machine-readable key=value lines; external runners (Node scripts) parse
//! these to discover the gateway address.
//!
//! Stays alive until the process is killed (SIGTERM/SIGINT).
//!
//! This binary only compiles with `--features test-support` and is excluded
//! from the production build path.

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::Value;
use tokio::net::TcpListener;

use my_ai_gateway::test_support::*;

const MODEL: &str = "conformance-test-model";

#[tokio::main]
async fn main() {
    // ── 1. Start deterministic mock upstream ────────────────────────────
    let mock_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let mock_addr = mock_listener.local_addr().expect("mock address");
    let mock_url = format!("http://127.0.0.1:{}", mock_addr.port());

    let mock_app = Router::new().route("/{*path}", any(mock_handler));
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app)
            .await
            .expect("mock upstream serve");
    });

    // ── 2. Build gateway config ────────────────────────────────────────
    let config = conformance_config(&mock_url);

    // ── 3. Create gateway router (in-memory, no DB) ────────────────────
    let gateway_router = test_gateway_router(config);

    // ── 4. Bind gateway ────────────────────────────────────────────────
    let gw_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind gateway");
    let gw_addr = gw_listener.local_addr().expect("gateway address");

    // ── 5. Print readiness for Node runners ────────────────────────────
    println!(
        "CONFORMANCE_GATEWAY_URL=http://127.0.0.1:{}",
        gw_addr.port()
    );
    println!("CONFORMANCE_MOCK_URL={mock_url}");
    println!("CONFORMANCE_MODEL={MODEL}");
    println!("CONFORMANCE_READY=true");

    // ── 6. Serve until killed ──────────────────────────────────────────
    axum::serve(gw_listener, gateway_router)
        .await
        .expect("gateway serve");
}

// ===========================================================================
// Gateway configuration
// ===========================================================================

fn conformance_config(mock_base_url: &str) -> GatewayConfig {
    let provider = ProviderConfig {
        id: "mock-provider".into(),
        name: "Conformance Mock Provider".into(),
        base_url: mock_base_url.into(),
        models: vec![MODEL.into()],
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
        id: "mock-account".into(),
        provider_id: "mock-provider".into(),
        display_name: "Mock Account".into(),
        credential_env: None,
        credential_ciphertext: None,
        credential: Some("sk-conformance-mock".into()),
        enabled: true,
        weight: 100,
        protocol_capabilities: HashMap::new(),
        capabilities: None,
        model_overrides: HashMap::new(),
        model_map: HashMap::new(),
    };

    let route = RouteConfig {
        id: "conformance-route".into(),
        model: MODEL.into(),
        provider_id: "mock-provider".into(),
        protocols: vec![
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ],
        primary_account_id: "mock-account".into(),
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

// ===========================================================================
// Deterministic mock upstream handler
// ===========================================================================

async fn mock_handler(request: Request) -> Response<Body> {
    let path = request.uri().path().to_owned();
    let (parts, body) = request.into_parts();

    let body_bytes = body
        .collect()
        .await
        .map(|c| c.to_bytes())
        .unwrap_or_default();

    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap_or_else(|_| Value::Null);

    let model = body_json
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or(MODEL);
    let is_stream = body_json
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let has_tools = body_json.get("tools").is_some_and(|v| v.is_array());
    let has_response_format = body_json.get("response_format").is_some();

    let _ = parts;

    match path.as_str() {
        p if p.ends_with("/chat/completions") => {
            if has_tools {
                chat_tool_response(model)
            } else if has_response_format {
                chat_structured_response(model)
            } else if is_stream {
                chat_stream_response(model)
            } else {
                chat_basic_response(model)
            }
        }
        p if p.ends_with("/responses") => {
            if has_tools {
                responses_tool_response(model)
            } else if is_stream {
                responses_stream_response(model)
            } else {
                responses_basic_response(model)
            }
        }
        p if p.ends_with("/messages") => {
            if has_tools {
                messages_tool_response(model)
            } else if is_stream {
                messages_stream_response(model)
            } else {
                messages_basic_response(model)
            }
        }
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"error":{"message":"unknown endpoint","type":"invalid_request_error"}}"#,
            ))
            .expect("404 response"),
    }
}

// ── Chat Completions responses ──────────────────────────────────────────────

fn chat_basic_response(model: &str) -> Response<Body> {
    let body = serde_json::json!({
        "id": "chatcmpl-conformance-001",
        "object": "chat.completion",
        "created": 1700000000_u64,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "This is a deterministic conformance test response."
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 8,
            "total_tokens": 18
        }
    });
    json_response(StatusCode::OK, &body)
}

fn chat_stream_response(model: &str) -> Response<Body> {
    let events = vec![
        serde_json::json!({"id":"chatcmpl-conformance-s01","object":"chat.completion.chunk","created":1700000000_u64,"model":model,"choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-conformance-s01","object":"chat.completion.chunk","created":1700000000_u64,"model":model,"choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-conformance-s01","object":"chat.completion.chunk","created":1700000000_u64,"model":model,"choices":[{"index":0,"delta":{"content":" from"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-conformance-s01","object":"chat.completion.chunk","created":1700000000_u64,"model":model,"choices":[{"index":0,"delta":{"content":" conformance"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-conformance-s01","object":"chat.completion.chunk","created":1700000000_u64,"model":model,"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}),
    ];
    sse_response(events, true)
}

fn chat_tool_response(model: &str) -> Response<Body> {
    let body = serde_json::json!({
        "id": "chatcmpl-conformance-tool-001",
        "object": "chat.completion",
        "created": 1700000000_u64,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_conformance_001",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"San Francisco\",\"unit\":\"celsius\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 30,
            "completion_tokens": 20,
            "total_tokens": 50
        }
    });
    json_response(StatusCode::OK, &body)
}

fn chat_structured_response(model: &str) -> Response<Body> {
    let body = serde_json::json!({
        "id": "chatcmpl-conformance-struct-001",
        "object": "chat.completion",
        "created": 1700000000_u64,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "{\"name\":\"test\",\"value\":42}",
                "refusal": null
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 15,
            "completion_tokens": 10,
            "total_tokens": 25
        }
    });
    json_response(StatusCode::OK, &body)
}

// ── Responses responses ─────────────────────────────────────────────────────

fn responses_basic_response(model: &str) -> Response<Body> {
    let body = serde_json::json!({
        "id": "resp_conformance_001",
        "object": "response",
        "created_at": 1700000000_u64,
        "status": "completed",
        "model": model,
        "output": [{
            "type": "message",
            "id": "msg_conformance_001",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": "This is a deterministic conformance test response.",
                "annotations": []
            }]
        }],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 8,
            "total_tokens": 18
        }
    });
    json_response(StatusCode::OK, &body)
}

fn responses_stream_response(model: &str) -> Response<Body> {
    let events = vec![
        serde_json::json!({"type":"response.created","response":{"id":"resp_conformance_s01","object":"response","created_at":1700000000_u64,"status":"in_progress","model":model,"output":[],"usage":null},"sequence_number":0}),
        serde_json::json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_conformance_s01","status":"in_progress","role":"assistant","content":[]},"sequence_number":1}),
        serde_json::json!({"type":"response.content_part.added","item_id":"msg_conformance_s01","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]},"sequence_number":2}),
        serde_json::json!({"type":"response.output_text.delta","item_id":"msg_conformance_s01","output_index":0,"content_index":0,"delta":"Hello from conformance","sequence_number":3}),
        serde_json::json!({"type":"response.output_text.done","item_id":"msg_conformance_s01","output_index":0,"content_index":0,"text":"Hello from conformance","sequence_number":4}),
        serde_json::json!({"type":"response.content_part.done","item_id":"msg_conformance_s01","output_index":0,"content_index":0,"part":{"type":"output_text","text":"Hello from conformance","annotations":[]},"sequence_number":5}),
        serde_json::json!({"type":"response.output_item.done","output_index":0,"item":{"type":"message","id":"msg_conformance_s01","status":"completed","role":"assistant","content":[{"type":"output_text","text":"Hello from conformance","annotations":[]}]},"sequence_number":6}),
        serde_json::json!({"type":"response.completed","response":{"id":"resp_conformance_s01","object":"response","created_at":1700000000_u64,"status":"completed","model":model,"output":[{"type":"message","id":"msg_conformance_s01","status":"completed","role":"assistant","content":[{"type":"output_text","text":"Hello from conformance","annotations":[]}]}],"usage":{"input_tokens":10,"output_tokens":3,"total_tokens":13}},"sequence_number":7}),
    ];
    sse_typed_response(events)
}

fn responses_tool_response(model: &str) -> Response<Body> {
    let body = serde_json::json!({
        "id": "resp_conformance_tool_001",
        "object": "response",
        "created_at": 1700000000_u64,
        "status": "completed",
        "model": model,
        "output": [{
            "type": "function_call",
            "id": "fc_conformance_001",
            "call_id": "call_conformance_001",
            "name": "get_weather",
            "arguments": "{\"location\":\"San Francisco\",\"unit\":\"celsius\"}",
            "status": "completed"
        }],
        "usage": {
            "input_tokens": 30,
            "output_tokens": 20,
            "total_tokens": 50
        }
    });
    json_response(StatusCode::OK, &body)
}

// ── Anthropic Messages responses ────────────────────────────────────────────

fn messages_basic_response(model: &str) -> Response<Body> {
    let body = serde_json::json!({
        "id": "msg_conformance_001",
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{
            "type": "text",
            "text": "This is a deterministic conformance test response."
        }],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 10,
            "output_tokens": 8
        }
    });
    json_response(StatusCode::OK, &body)
}

fn messages_stream_response(model: &str) -> Response<Body> {
    let events = vec![
        serde_json::json!({"type":"message_start","message":{"id":"msg_conformance_s01","type":"message","role":"assistant","model":model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":0}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello from conformance"}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}),
        serde_json::json!({"type":"message_stop"}),
    ];
    sse_typed_response(events)
}

fn messages_tool_response(model: &str) -> Response<Body> {
    let body = serde_json::json!({
        "id": "msg_conformance_tool_001",
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{
            "type": "tool_use",
            "id": "toolu_conformance_001",
            "name": "get_weather",
            "input": {
                "location": "San Francisco",
                "unit": "celsius"
            }
        }],
        "stop_reason": "tool_use",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 30,
            "output_tokens": 20
        }
    });
    json_response(StatusCode::OK, &body)
}

// ── Response builders ───────────────────────────────────────────────────────

fn json_response(status: StatusCode, body: &Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("json response")
}

/// Build an SSE response for Chat Completions (data-only, with [DONE]).
fn sse_response(events: Vec<Value>, with_done: bool) -> Response<Body> {
    let mut payload = String::new();
    for event in events {
        payload.push_str(&format!("data: {}\n\n", event));
    }
    if with_done {
        payload.push_str("data: [DONE]\n\n");
    }
    let chunks: Vec<Bytes> = payload
        .split_inclusive("\n\n")
        .map(|s| Bytes::from(s.to_owned()))
        .collect();
    let delay = std::time::Duration::from_millis(5);
    let stream =
        futures_util::stream::iter(chunks.into_iter().enumerate().map(move |(i, chunk)| {
            let d = if i == 0 {
                std::time::Duration::ZERO
            } else {
                delay
            };
            (chunk, d)
        }))
        .then(|(chunk, d)| async move {
            if !d.is_zero() {
                tokio::time::sleep(d).await;
            }
            Ok::<_, std::io::Error>(chunk)
        });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream))
        .expect("sse response")
}

/// Build an SSE response for Responses/Messages (event-typed, no [DONE]).
fn sse_typed_response(events: Vec<Value>) -> Response<Body> {
    let mut payload = String::new();
    for event in &events {
        let event_type = event
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("message");
        payload.push_str(&format!("event: {event_type}\ndata: {event}\n\n"));
    }
    let chunks: Vec<Bytes> = payload
        .split_inclusive("\n\n")
        .map(|s| Bytes::from(s.to_owned()))
        .collect();
    let delay = std::time::Duration::from_millis(5);
    let stream =
        futures_util::stream::iter(chunks.into_iter().enumerate().map(move |(i, chunk)| {
            let d = if i == 0 {
                std::time::Duration::ZERO
            } else {
                delay
            };
            (chunk, d)
        }))
        .then(|(chunk, d)| async move {
            if !d.is_zero() {
                tokio::time::sleep(d).await;
            }
            Ok::<_, std::io::Error>(chunk)
        });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream))
        .expect("sse typed response")
}
