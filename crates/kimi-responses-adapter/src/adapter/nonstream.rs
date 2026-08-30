use serde_json::{Map, Value, json};

use crate::adapter::config::Config;
use crate::adapter::reasoning::{encode_reasoning, encode_redacted_reasoning};
use crate::adapter::stream::{extract_sources, now_unix, rand_id, usage_map};
use crate::adapter::types::{AnthropicMessageObj, ResponsesRequest};

/// Converts a non-streaming Anthropic message into an OpenAI Responses
/// response object.
pub fn anthropic_to_response(
    cfg: &Config,
    req: &ResponsesRequest,
    msg: &AnthropicMessageObj,
) -> Value {
    let mut output: Vec<Value> = Vec::new();
    let mut pending_search_id = String::new();
    let mut pending_query = String::new();

    macro_rules! flush_search {
        ($sources:expr) => {
            if !pending_search_id.is_empty() {
                let mut action = Map::new();
                action.insert("type".to_string(), json!("search"));
                if !pending_query.is_empty() {
                    action.insert("query".to_string(), json!(pending_query.clone()));
                }
                let sources: Vec<Value> = $sources;
                if !sources.is_empty() {
                    action.insert("sources".to_string(), json!(sources));
                }
                output.push(json!({
                    "id": pending_search_id.clone(),
                    "type": "web_search_call",
                    "status": "completed",
                    "action": Value::Object(action),
                }));
                pending_search_id.clear();
                pending_query.clear();
            }
        };
    }

    for b in &msg.content {
        match b.r#type.as_str() {
            "text" => {
                // Suppress Kimi's web-search status preamble, same as streaming.
                if b.text.starts_with(&cfg.search_status_prefix) {
                    continue;
                }
                output.push(json!({
                    "id": rand_id("msg_"),
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": b.text, "annotations": []}],
                }));
            }
            "thinking" => {
                output.push(json!({
                    "id": rand_id("rs_"),
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": b.thinking}],
                    "encrypted_content": encode_reasoning(&b.thinking, &b.signature),
                }));
            }
            "redacted_thinking" => {
                output.push(json!({
                    "id": rand_id("rs_"),
                    "type": "reasoning",
                    "summary": [],
                    "encrypted_content": encode_redacted_reasoning(&b.data),
                }));
            }
            "tool_use" => {
                let args = b
                    .input
                    .as_ref()
                    .map(|r| r.get().to_string())
                    .unwrap_or_else(|| "{}".to_string());
                output.push(json!({
                    "id": rand_id("fc_"),
                    "type": "function_call",
                    "call_id": b.id,
                    "name": b.name,
                    "arguments": args,
                    "status": "completed",
                }));
            }
            "server_tool_use" => {
                pending_search_id = rand_id("ws_");
                pending_query = b
                    .input
                    .as_ref()
                    .and_then(|r| serde_json::from_str::<Value>(r.get()).ok())
                    .and_then(|v| v.get("query").and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_default();
            }
            "web_search_tool_result" => {
                flush_search!(extract_sources(b.content.as_ref()));
            }
            _ => {}
        }
    }
    flush_search!(Vec::new());

    let mut resp = json!({
        "id": rand_id("resp_"),
        "object": "response",
        "created_at": now_unix(),
        "status": "completed",
        "error": Value::Null,
        "incomplete_details": Value::Null,
        "model": req.model,
        "output": output,
        "parallel_tool_calls": true,
        "tool_choice": "auto",
        "tools": [],
        "metadata": {},
    });
    if msg.stop_reason == "max_tokens" {
        resp["status"] = json!("incomplete");
        resp["incomplete_details"] = json!({"reason": "max_output_tokens"});
    }
    if let Some(u) = &msg.usage {
        let thinking = u
            .output_tokens_details
            .as_ref()
            .map(|d| d.thinking_tokens)
            .unwrap_or(0);
        resp["usage"] = usage_map(
            u.input_tokens,
            u.output_tokens,
            u.cache_creation_input_tokens,
            u.cache_read_input_tokens,
            thinking,
        );
    }
    resp
}
