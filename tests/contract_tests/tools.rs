//! Tool calling contract tests across all three protocols (#117).
//!
//! Case IDs: common.tool.single, common.tool.parallel, common.tool.required,
//! common.tool.named, common.tool.none, common.tool.arguments_json,
//! common.tool.result_roundtrip, common.tool.stream_arguments.
//!
//! Verifies tool name, arguments JSON, call IDs, tool_choice forwarding,
//! parallel calls, two-round result correlation and streaming arg assembly.

use axum::http::StatusCode;
use my_ai_gateway::test_support::test_gateway_router;
use serde_json::json;

use crate::assert_case;
use crate::common::*;

const MODEL: &str = "test-model";

// ── common.tool.single — Chat ───────────────────────────────────────────────

#[tokio::test]
async fn chat_tool_single() {
    assert_case!("common.tool.single");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "What is the weather?"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
            }
        }]
    });
    let response = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.tool.single",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;

    let tool_calls = &resp_body["choices"][0]["message"]["tool_calls"];
    assert!(tool_calls.is_array(), "tool_calls array present");
    assert_eq!(tool_calls.as_array().unwrap().len(), 1);

    let call = &tool_calls[0];
    assert_eq!(call["function"]["name"], "get_weather");
    assert_eq!(call["type"], "function");
    assert!(call["id"].is_string(), "call has id");

    let args: serde_json::Value =
        serde_json::from_str(call["function"]["arguments"].as_str().unwrap())
            .expect("arguments is valid JSON");
    assert_eq!(args["location"], "San Francisco");

    assert_eq!(resp_body["choices"][0]["finish_reason"], "tool_calls");

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].body["tools"].is_array(),
        "tools forwarded to upstream"
    );
}

// ── common.tool.single — Responses ──────────────────────────────────────────

#[tokio::test]
async fn responses_tool_single() {
    assert_case!("common.tool.single");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": "What is the weather?",
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
        }]
    });
    let response = gateway_post(
        &router,
        "/v1/responses",
        "responses.tool.single",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;

    let output = resp_body["output"].as_array().expect("output array");
    assert_eq!(output.len(), 1);

    let fc = &output[0];
    assert_eq!(fc["type"], "function_call");
    assert_eq!(fc["name"], "get_weather");
    assert!(fc["call_id"].is_string(), "function_call has call_id");

    let args: serde_json::Value =
        serde_json::from_str(fc["arguments"].as_str().unwrap()).expect("arguments is valid JSON");
    assert_eq!(args["location"], "San Francisco");
}

// ── common.tool.single — Messages ───────────────────────────────────────────

#[tokio::test]
async fn messages_tool_single() {
    assert_case!("common.tool.single");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "What is the weather?"}],
        "max_tokens": 256,
        "tools": [{
            "name": "get_weather",
            "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}}
        }]
    });
    let response = gateway_anthropic_post(&router, "messages.tool.single", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;

    let content = resp_body["content"].as_array().expect("content array");
    assert_eq!(content.len(), 1);

    let tool_use = &content[0];
    assert_eq!(tool_use["type"], "tool_use");
    assert_eq!(tool_use["name"], "get_weather");
    assert!(tool_use["id"].is_string(), "tool_use has id");
    assert_eq!(tool_use["input"]["location"], "San Francisco");

    assert_eq!(resp_body["stop_reason"], "tool_use");
}

// ── common.tool.parallel — Chat ─────────────────────────────────────────────

#[tokio::test]
async fn chat_tool_parallel() {
    assert_case!("common.tool.parallel");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Weather in SF and Tokyo?"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
            }
        }]
    });
    let response = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.tool.parallel",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;

    let tool_calls = resp_body["choices"][0]["message"]["tool_calls"]
        .as_array()
        .expect("tool_calls array");
    assert_eq!(tool_calls.len(), 2, "two parallel tool calls");

    let ids: Vec<&str> = tool_calls
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_ne!(ids[0], ids[1], "parallel calls have distinct IDs");

    for call in tool_calls {
        assert_eq!(call["function"]["name"], "get_weather");
        let _args: serde_json::Value =
            serde_json::from_str(call["function"]["arguments"].as_str().unwrap())
                .expect("parallel call arguments is valid JSON");
    }
}

// ── common.tool.parallel — Responses ────────────────────────────────────────

#[tokio::test]
async fn responses_tool_parallel() {
    assert_case!("common.tool.parallel");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": "Weather in SF and Tokyo?",
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
        }]
    });
    let response = gateway_post(
        &router,
        "/v1/responses",
        "responses.tool.parallel",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;

    let output = resp_body["output"].as_array().expect("output array");
    assert_eq!(output.len(), 2, "two parallel function_calls");

    let call_ids: Vec<&str> = output
        .iter()
        .map(|o| o["call_id"].as_str().unwrap())
        .collect();
    assert_ne!(call_ids[0], call_ids[1], "distinct call_ids");
}

// ── common.tool.parallel — Messages ─────────────────────────────────────────

#[tokio::test]
async fn messages_tool_parallel() {
    assert_case!("common.tool.parallel");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Weather in SF and Tokyo?"}],
        "max_tokens": 256,
        "tools": [{
            "name": "get_weather",
            "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}}
        }]
    });
    let response =
        gateway_anthropic_post(&router, "messages.tool.parallel", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;

    let content = resp_body["content"].as_array().expect("content array");
    assert_eq!(content.len(), 2, "two parallel tool_use blocks");

    let ids: Vec<&str> = content.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_ne!(ids[0], ids[1], "distinct tool_use IDs");
}

// ── common.tool.required ────────────────────────────────────────────────────

#[tokio::test]
async fn chat_tool_required() {
    assert_case!("common.tool.required");
    // tool_choice="required" must be forwarded to upstream unchanged.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Do something"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
            }
        }],
        "tool_choice": "required"
    });
    let response = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.tool.single",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].body["tool_choice"], "required",
        "tool_choice=required forwarded"
    );
}

// ── common.tool.named ───────────────────────────────────────────────────────

#[tokio::test]
async fn chat_tool_named() {
    assert_case!("common.tool.named");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Get weather"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
            }
        }],
        "tool_choice": {"type": "function", "function": {"name": "get_weather"}}
    });
    let response = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.tool.single",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].body["tool_choice"]["function"]["name"], "get_weather",
        "named tool_choice forwarded"
    );
}

// ── common.tool.none ────────────────────────────────────────────────────────

#[tokio::test]
async fn chat_tool_none() {
    assert_case!("common.tool.none");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
            }
        }],
        "tool_choice": "none"
    });
    let response = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.text.basic",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].body["tool_choice"], "none",
        "tool_choice=none forwarded"
    );
}

// ── common.tool.arguments_json ──────────────────────────────────────────────

#[tokio::test]
async fn chat_tool_arguments_json() {
    assert_case!("common.tool.arguments_json");
    // Verify arguments field is a valid JSON string with expected structure.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Weather?"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "parameters": {"type": "object", "properties": {
                    "location": {"type": "string"},
                    "unit": {"type": "string"}
                }}
            }
        }]
    });
    let response = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.tool.single",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    let resp_body = json_body(response).await;

    let args_str = resp_body["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .expect("arguments is string");
    let args: serde_json::Value = serde_json::from_str(args_str).expect("arguments parses as JSON");
    assert!(args.is_object(), "arguments is a JSON object");
    assert_eq!(args["location"], "San Francisco");
    assert_eq!(args["unit"], "celsius");
}

// ── common.tool.result_roundtrip ────────────────────────────────────────────

#[tokio::test]
async fn chat_tool_result_roundtrip() {
    assert_case!("common.tool.result_roundtrip");
    // Complete two-round tool call:
    // 1. Client request(tools) → gateway → mock returns tool_call
    // 2. Client sends tool_result → gateway → mock returns final text

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    // Round 1: Get tool call
    let body1 = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "What is the weather?"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
            }
        }]
    });
    let response1 = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.tool.single",
        &body1.to_string(),
    )
    .await;

    assert_status(&response1, StatusCode::OK);
    let resp1 = json_body(response1).await;
    let tool_call = &resp1["choices"][0]["message"]["tool_calls"][0];
    let call_id = tool_call["id"].as_str().expect("tool call has id");

    // Round 2: Send tool result with correlated call_id
    let body2 = json!({
        "model": MODEL,
        "messages": [
            {"role": "user", "content": "What is the weather?"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"San Francisco\",\"unit\":\"celsius\"}"
                    }
                }]
            },
            {
                "role": "tool",
                "tool_call_id": call_id,
                "content": "{\"temperature\": 72, \"condition\": \"sunny\"}"
            }
        ]
    });
    let response2 = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.tool.result_final",
        &body2.to_string(),
    )
    .await;

    assert_status(&response2, StatusCode::OK);
    let resp2 = json_body(response2).await;
    assert_eq!(resp2["choices"][0]["message"]["role"], "assistant");
    assert!(
        resp2["choices"][0]["message"]["content"]
            .as_str()
            .unwrap()
            .contains("72°F"),
        "final response references tool result"
    );

    // Verify upstream received correlated tool_call_id
    let requests = mock.take_requests();
    assert_eq!(requests.len(), 2, "two upstream requests");
    let round2_body = &requests[1].body;
    let tool_msg = round2_body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "tool")
        .expect("tool message in round 2");
    assert_eq!(
        tool_msg["tool_call_id"], call_id,
        "tool_call_id correlated in round 2"
    );
}

// ── common.tool.result_roundtrip — Responses ────────────────────────────────

#[tokio::test]
async fn responses_tool_result_roundtrip() {
    assert_case!("common.tool.result_roundtrip");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    // Round 1: Get function_call
    let body1 = json!({
        "model": MODEL,
        "input": "What is the weather?",
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
        }]
    });
    let response1 = gateway_post(
        &router,
        "/v1/responses",
        "responses.tool.single",
        &body1.to_string(),
    )
    .await;

    assert_status(&response1, StatusCode::OK);
    let resp1 = json_body(response1).await;
    let fc = &resp1["output"][0];
    let call_id = fc["call_id"].as_str().expect("function_call has call_id");

    // Round 2: Send function_call_output
    let body2 = json!({
        "model": MODEL,
        "input": [
            {"type": "message", "role": "user", "content": "What is the weather?"},
            {
                "type": "function_call",
                "call_id": call_id,
                "name": "get_weather",
                "arguments": "{\"location\":\"San Francisco\",\"unit\":\"celsius\"}"
            },
            {
                "type": "function_call_output",
                "call_id": call_id,
                "output": "{\"temperature\": 72, \"condition\": \"sunny\"}"
            }
        ]
    });
    let response2 = gateway_post(
        &router,
        "/v1/responses",
        "responses.tool.result_final",
        &body2.to_string(),
    )
    .await;

    assert_status(&response2, StatusCode::OK);
    let resp2 = json_body(response2).await;
    assert_eq!(resp2["status"], "completed");
    let output_text = resp2["output"][0]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(
        output_text.contains("72°F"),
        "final response references tool result"
    );

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 2, "two upstream requests");
}

// ── common.tool.result_roundtrip — Messages ─────────────────────────────────

#[tokio::test]
async fn messages_tool_result_roundtrip() {
    assert_case!("common.tool.result_roundtrip");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    // Round 1: Get tool_use
    let body1 = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "What is the weather?"}],
        "max_tokens": 256,
        "tools": [{
            "name": "get_weather",
            "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}}
        }]
    });
    let response1 =
        gateway_anthropic_post(&router, "messages.tool.single", &body1.to_string()).await;

    assert_status(&response1, StatusCode::OK);
    let resp1 = json_body(response1).await;
    let tool_use = &resp1["content"][0];
    let tool_use_id = tool_use["id"].as_str().expect("tool_use has id");

    // Round 2: Send tool_result
    let body2 = json!({
        "model": MODEL,
        "messages": [
            {"role": "user", "content": "What is the weather?"},
            {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": tool_use_id,
                    "name": "get_weather",
                    "input": {"location": "San Francisco", "unit": "celsius"}
                }]
            },
            {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": "{\"temperature\": 72, \"condition\": \"sunny\"}"
                }]
            }
        ],
        "max_tokens": 256
    });
    let response2 =
        gateway_anthropic_post(&router, "messages.tool.result_final", &body2.to_string()).await;

    assert_status(&response2, StatusCode::OK);
    let resp2 = json_body(response2).await;
    assert_eq!(resp2["stop_reason"], "end_turn");
    let text = resp2["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("72°F"),
        "final response references tool result"
    );

    let requests = mock.take_requests();
    assert_eq!(requests.len(), 2, "two upstream requests");
    let round2_body = &requests[1].body;
    let tool_result_msg = round2_body["messages"].as_array().unwrap().last().unwrap();
    assert_eq!(
        tool_result_msg["content"][0]["tool_use_id"], tool_use_id,
        "tool_use_id correlated in round 2"
    );
}

// ── common.tool.stream_arguments — Chat ─────────────────────────────────────

#[tokio::test]
async fn chat_tool_stream_arguments() {
    assert_case!("common.tool.stream_arguments");
    // Streaming tool arguments: multiple deltas must concatenate to valid JSON
    // matching the fixed fixture.

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Weather?"}],
        "stream": true,
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
            }
        }]
    });
    let response = gateway_post(
        &router,
        "/v1/chat/completions",
        "chat.tool.stream_arguments",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body_text = text_body(response).await;
    let events = parse_sse_events(&body_text);

    let json_events: Vec<_> = events.iter().filter(|e| !e.is_done()).collect();

    let mut tool_name = String::new();
    let mut tool_args = String::new();
    let mut tool_id = String::new();

    for event in &json_events {
        let data = event.json();
        if let Some(tool_calls) = data["choices"][0]["delta"]["tool_calls"].as_array() {
            for tc in tool_calls {
                if let Some(id) = tc["id"].as_str() {
                    tool_id = id.to_string();
                }
                if let Some(name) = tc["function"]["name"].as_str() {
                    tool_name = name.to_string();
                }
                if let Some(args) = tc["function"]["arguments"].as_str() {
                    tool_args.push_str(args);
                }
            }
        }
    }

    assert_eq!(tool_name, "get_weather", "tool name from first chunk");
    assert!(!tool_id.is_empty(), "tool call id present");

    let parsed_args: serde_json::Value =
        serde_json::from_str(&tool_args).expect("concatenated arguments is valid JSON");
    assert_eq!(
        parsed_args,
        json!({"location": "San Francisco"}),
        "assembled arguments match fixture"
    );

    let done_events: Vec<_> = events.iter().filter(|e| e.is_done()).collect();
    assert_eq!(done_events.len(), 1, "[DONE] present");
}

// ── common.tool.stream_arguments — Responses ────────────────────────────────

#[tokio::test]
async fn responses_tool_stream_arguments() {
    assert_case!("common.tool.stream_arguments");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "input": "Weather?",
        "stream": true,
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
        }]
    });
    let response = gateway_post(
        &router,
        "/v1/responses",
        "responses.tool.stream_arguments",
        &body.to_string(),
    )
    .await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body_text = text_body(response).await;
    let events = parse_sse_events(&body_text);

    let mut args = String::new();
    for event in &events {
        if event.event_type.as_deref() == Some("response.function_call_arguments.delta") {
            let data = event.json();
            if let Some(delta) = data["delta"].as_str() {
                args.push_str(delta);
            }
        }
    }

    let parsed_args: serde_json::Value =
        serde_json::from_str(&args).expect("concatenated args is valid JSON");
    assert_eq!(
        parsed_args,
        json!({"location": "San Francisco"}),
        "assembled Responses tool arguments match fixture"
    );

    let has_done_event = events
        .iter()
        .any(|e| e.event_type.as_deref() == Some("response.function_call_arguments.done"));
    assert!(has_done_event, "arguments.done event present");

    let has_completed = events
        .iter()
        .any(|e| e.event_type.as_deref() == Some("response.completed"));
    assert!(has_completed, "response.completed present");
}

// ── common.tool.stream_arguments — Messages ─────────────────────────────────

#[tokio::test]
async fn messages_tool_stream_arguments() {
    assert_case!("common.tool.stream_arguments");

    let mock = spawn_mock_with_catalog().await;
    let config = native_config(mock.base_url(), MODEL);
    let router = test_gateway_router(config);

    let body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Weather?"}],
        "max_tokens": 256,
        "stream": true,
        "tools": [{
            "name": "get_weather",
            "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}}
        }]
    });
    let response =
        gateway_anthropic_post(&router, "messages.tool.stream_arguments", &body.to_string()).await;

    assert_status(&response, StatusCode::OK);
    assert_sse_content_type(&response);

    let body_text = text_body(response).await;
    let events = parse_sse_events(&body_text);

    let mut partial_json = String::new();
    for event in &events {
        if event.event_type.as_deref() == Some("content_block_delta") {
            let data = event.json();
            if data["delta"]["type"] == "input_json_delta" {
                if let Some(pj) = data["delta"]["partial_json"].as_str() {
                    partial_json.push_str(pj);
                }
            }
        }
    }

    let parsed_args: serde_json::Value =
        serde_json::from_str(&partial_json).expect("concatenated partial_json is valid JSON");
    assert_eq!(
        parsed_args,
        json!({"location": "San Francisco"}),
        "assembled Messages tool arguments match fixture"
    );

    let has_message_stop = events
        .iter()
        .any(|e| e.event_type.as_deref() == Some("message_stop"));
    assert!(has_message_stop, "message_stop present");
}
