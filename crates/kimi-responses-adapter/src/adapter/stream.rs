use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngCore;
use serde_json::{Map, Value, json};

use crate::adapter::config::Config;
use crate::adapter::reasoning::{encode_reasoning, encode_redacted_reasoning};
use crate::adapter::types::{
    AnthropicContent, AnthropicDelta, AnthropicStreamEvent, ResponsesRequest,
};

pub fn rand_id(prefix: &str) -> String {
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    let mut s = String::with_capacity(prefix.len() + 32);
    s.push_str(prefix);
    for byte in b {
        let _ = write!(s, "{byte:02x}");
    }
    s
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Converts Anthropic usage to Responses usage. Anthropic reports cached
/// tokens separately from input_tokens, while Responses expects input_tokens
/// to be the total with cached_tokens as a subset detail.
pub fn usage_map(input: i64, output: i64, cache_create: i64, cached: i64, reasoning: i64) -> Value {
    let total_in = input + cached + cache_create;
    json!({
        "input_tokens": total_in,
        "input_tokens_details": {"cached_tokens": cached},
        "output_tokens": output,
        "output_tokens_details": {"reasoning_tokens": reasoning},
        "total_tokens": total_in + output,
    })
}

/// Pulls URLs out of a web_search_tool_result content array.
pub fn extract_sources(content: Option<&Value>) -> Vec<Value> {
    let mut sources = Vec::new();
    let Some(Value::Array(arr)) = content else {
        return sources;
    };
    for c in arr {
        if c.get("type").and_then(Value::as_str) == Some("web_search_result") {
            if let Some(u) = c.get("url").and_then(Value::as_str) {
                if !u.is_empty() {
                    sources.push(json!({"type": "url", "url": u}));
                }
            }
        }
    }
    sources
}

/// Reads an Anthropic SSE stream from `reader` and emits Responses SSE
/// frames through `sink` (one frame per event; callers flush per frame).
// Used by the canned-SSE tests; the server drives StreamTranslator over an
// async line stream instead.
#[allow(dead_code)]
pub fn translate_sse<F: FnMut(String)>(
    cfg: &Config,
    req: &ResponsesRequest,
    mut reader: impl std::io::BufRead,
    sink: F,
) -> std::io::Result<()> {
    let mut t = StreamTranslator::new(cfg, req, sink);
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let line = line.strip_suffix('\n').unwrap_or(&line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        t.feed_line(line);
        if t.is_done() {
            break;
        }
    }
    t.finish_eof();
    Ok(())
}

/// Converts a Kimi Anthropic SSE stream into an OpenAI Responses SSE stream.
/// It is a pure state machine over upstream events.
pub struct StreamTranslator<F: FnMut(String)> {
    marker: String,
    model: String,
    sink: F,
    seq: i64,

    response_id: String,
    created_at: i64,
    output: Vec<Value>,
    usage_in: i64,
    usage_out: i64,
    usage_cached: i64,
    usage_cache_create: i64,
    usage_think: i64,
    stop_reason: String,

    // current content block state
    block_type: String,
    output_index: usize,
    item_id: String,
    call_id: String,
    tool_name: String,
    args_buf: String,
    think_buf: String,
    sig_buf: String,
    redacted_data: String,
    text_buf: String,
    text_hold: bool,
    text_started: bool,
    sources: Vec<Value>,
    search_query: String,
    salvaged_query: String,
    // A closed text block matching the search-status marker is held until the
    // next block starts: dropped if that block is server_tool_use(web_search),
    // emitted as normal text otherwise (mirrors sub2api#5166).
    pending_status: String,
    pending_status_ready: bool,
    open_item: bool,
    open_search: bool,
    terminated: bool,

    // SSE frame accumulation
    data_lines: Vec<String>,
    done: bool,
}

impl<F: FnMut(String)> StreamTranslator<F> {
    pub fn new(cfg: &Config, req: &ResponsesRequest, sink: F) -> Self {
        StreamTranslator {
            marker: cfg.search_status_prefix.clone(),
            model: req.model.clone(),
            sink,
            seq: 0,
            response_id: String::new(),
            created_at: 0,
            output: Vec::new(),
            usage_in: 0,
            usage_out: 0,
            usage_cached: 0,
            usage_cache_create: 0,
            usage_think: 0,
            stop_reason: String::new(),
            block_type: String::new(),
            output_index: 0,
            item_id: String::new(),
            call_id: String::new(),
            tool_name: String::new(),
            args_buf: String::new(),
            think_buf: String::new(),
            sig_buf: String::new(),
            redacted_data: String::new(),
            text_buf: String::new(),
            text_hold: false,
            text_started: false,
            sources: Vec::new(),
            search_query: String::new(),
            salvaged_query: String::new(),
            pending_status: String::new(),
            pending_status_ready: false,
            open_item: false,
            open_search: false,
            terminated: false,
            data_lines: Vec::new(),
            done: false,
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Feeds one raw SSE line (without the trailing newline) into the parser.
    pub fn feed_line(&mut self, line: &str) {
        if self.done {
            return;
        }
        if line.is_empty() {
            self.dispatch();
        } else if let Some(rest) = line.strip_prefix("data:") {
            self.data_lines.push(rest.trim().to_string());
        }
    }

    fn dispatch(&mut self) {
        if self.data_lines.is_empty() {
            return;
        }
        let payload = self.data_lines.join("\n");
        self.data_lines.clear();
        let Ok(ev) = serde_json::from_str::<AnthropicStreamEvent>(&payload) else {
            return;
        };
        let is_stop = ev.r#type == "message_stop";
        self.handle_event(&ev);
        if is_stop {
            self.done = true;
        }
    }

    /// Called at end of the upstream stream. Anthropic streams always end
    /// with message_stop; anything else means the upstream (or the
    /// connection) died mid-flight.
    pub fn finish_eof(&mut self) {
        self.dispatch();
        if !self.done {
            self.fail(
                "upstream_error",
                "upstream stream ended before message_stop",
            );
            return;
        }
        self.finish();
    }

    fn emit(&mut self, event_type: &str, payload: Value) {
        let obj = payload.as_object().expect("event payload is an object");
        let mut map = obj.clone();
        map.insert("type".to_string(), json!(event_type));
        map.insert("sequence_number".to_string(), json!(self.seq));
        self.seq += 1;
        let data = serde_json::to_string(&Value::Object(map)).expect("event serializes");
        (self.sink)(format!("event: {event_type}\ndata: {data}\n\n"));
    }

    fn response_shell(&self, status: &str) -> Value {
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": status,
            "error": Value::Null,
            "incomplete_details": Value::Null,
            "model": self.model,
            "output": self.output,
            "parallel_tool_calls": true,
            "tool_choice": "auto",
            "tools": [],
            "metadata": {},
        })
    }

    fn handle_event(&mut self, ev: &AnthropicStreamEvent) {
        match ev.r#type.as_str() {
            "message_start" => {
                self.response_id = rand_id("resp_");
                self.created_at = now_unix();
                if let Some(m) = &ev.message {
                    if let Some(u) = &m.usage {
                        self.usage_in = u.input_tokens;
                        self.usage_cached = u.cache_read_input_tokens;
                        self.usage_cache_create = u.cache_creation_input_tokens;
                    }
                }
                self.emit(
                    "response.created",
                    json!({"response": self.response_shell("in_progress")}),
                );
                self.emit(
                    "response.in_progress",
                    json!({"response": self.response_shell("in_progress")}),
                );
            }
            "content_block_start" => {
                if let Some(cb) = &ev.content_block {
                    self.block_start(cb);
                }
            }
            "content_block_delta" => {
                if let Some(d) = &ev.delta {
                    self.block_delta(d);
                }
            }
            "content_block_stop" => self.block_stop(),
            "message_delta" => {
                if let Some(u) = &ev.usage {
                    self.usage_out = u.output_tokens;
                    if u.cache_read_input_tokens > 0 {
                        self.usage_cached = u.cache_read_input_tokens;
                    }
                    if u.cache_creation_input_tokens > 0 {
                        self.usage_cache_create = u.cache_creation_input_tokens;
                    }
                    if let Some(d) = &u.output_tokens_details {
                        self.usage_think = d.thinking_tokens;
                    }
                }
                if let Some(d) = &ev.delta {
                    if !d.stop_reason.is_empty() {
                        self.stop_reason = d.stop_reason.clone();
                    }
                }
            }
            "message_stop" => self.finish(),
            "error" => {
                let mut msg = "upstream error".to_string();
                let mut code = "upstream_error";
                if let Some(e) = &ev.error {
                    if !e.r#type.is_empty() {
                        code = &e.r#type;
                    }
                    if !e.message.is_empty() {
                        msg = e.message.clone();
                    }
                }
                self.fail(code, &msg);
            }
            _ => {}
        }
    }

    fn block_start(&mut self, cb: &AnthropicContent) {
        // Arbitrate any held search-status text against the new block.
        if self.pending_status_ready {
            if cb.r#type == "server_tool_use" && cb.name == "web_search" {
                // Kimi does not stream server_tool_use input, so the status
                // text is the only place the query appears: salvage it before
                // dropping.
                if self.pending_status.starts_with(&self.marker) {
                    self.salvaged_query =
                        self.pending_status[self.marker.len()..].trim().to_string();
                }
                self.pending_status.clear();
                self.pending_status_ready = false;
            } else {
                self.flush_pending_status_text();
            }
        }

        self.block_type = cb.r#type.clone();
        self.args_buf.clear();
        self.think_buf.clear();
        self.sig_buf.clear();
        self.text_buf.clear();
        self.text_hold = false;
        self.text_started = false;
        self.redacted_data.clear();
        self.output_index = self.output.len();

        match cb.r#type.as_str() {
            "text" => {
                // Hold deltas until we know whether this block is a Kimi
                // web-search status preamble ("Search results for query: ...").
                self.text_hold = true;
            }
            "thinking" => {
                self.sig_buf.push_str(&cb.signature);
                self.item_id = rand_id("rs_");
                self.open_item = true;
                self.emit(
                    "response.output_item.added",
                    json!({
                        "output_index": self.output_index,
                        "item": {"id": self.item_id, "type": "reasoning", "summary": []},
                    }),
                );
                self.emit(
                    "response.reasoning_summary_part.added",
                    json!({
                        "item_id": self.item_id,
                        "output_index": self.output_index,
                        "summary_index": 0,
                        "part": {"type": "summary_text", "text": ""},
                    }),
                );
            }
            "redacted_thinking" => {
                self.item_id = rand_id("rs_");
                self.redacted_data = cb.data.clone();
                self.open_item = true;
                self.emit(
                    "response.output_item.added",
                    json!({
                        "output_index": self.output_index,
                        "item": {"id": self.item_id, "type": "reasoning", "summary": []},
                    }),
                );
            }
            "tool_use" => {
                self.item_id = rand_id("fc_");
                self.call_id = cb.id.clone();
                self.tool_name = cb.name.clone();
                self.open_item = true;
                self.emit(
                    "response.output_item.added",
                    json!({
                        "output_index": self.output_index,
                        "item": {
                            "id": self.item_id,
                            "type": "function_call",
                            "call_id": self.call_id,
                            "name": self.tool_name,
                            "arguments": "",
                            "status": "in_progress",
                        },
                    }),
                );
            }
            "server_tool_use" => {
                self.item_id = rand_id("ws_");
                self.open_item = true;
                self.open_search = true;
                self.search_query.clear();
                self.sources = Vec::new();
                self.emit(
                    "response.output_item.added",
                    json!({
                        "output_index": self.output_index,
                        "item": {"id": self.item_id, "type": "web_search_call", "status": "in_progress"},
                    }),
                );
                self.emit(
                    "response.web_search_call.in_progress",
                    json!({"output_index": self.output_index, "item_id": self.item_id}),
                );
            }
            "web_search_tool_result" => {
                self.sources = extract_sources(cb.content.as_ref());
            }
            _ => {}
        }
    }

    fn block_delta(&mut self, d: &AnthropicDelta) {
        match d.r#type.as_str() {
            "text_delta" => self.text_delta(&d.text),
            "thinking_delta" => {
                self.think_buf.push_str(&d.thinking);
                self.emit(
                    "response.reasoning_summary_text.delta",
                    json!({
                        "item_id": self.item_id,
                        "output_index": self.output_index,
                        "summary_index": 0,
                        "delta": d.thinking,
                    }),
                );
            }
            "signature_delta" => self.sig_buf.push_str(&d.signature),
            "input_json_delta" => {
                self.args_buf.push_str(&d.partial_json);
                if self.block_type == "tool_use" {
                    self.emit(
                        "response.function_call_arguments.delta",
                        json!({
                            "item_id": self.item_id,
                            "output_index": self.output_index,
                            "delta": d.partial_json,
                        }),
                    );
                }
            }
            _ => {}
        }
    }

    fn text_delta(&mut self, s: &str) {
        self.text_buf.push_str(s);
        if self.text_hold {
            let cur = self.text_buf.clone();
            if cur.len() < self.marker.len() && self.marker.starts_with(&cur) {
                return; // still ambiguous, keep buffering
            }
            if cur.starts_with(&self.marker) {
                return; // status preamble confirmed, hold until block stop then drop
            }
            // Not a status preamble: flush the buffer and stream live from here.
            self.text_hold = false;
            self.start_text_item();
            self.emit(
                "response.output_text.delta",
                json!({
                    "item_id": self.item_id,
                    "output_index": self.output_index,
                    "content_index": 0,
                    "delta": cur,
                }),
            );
            return;
        }
        if !self.text_started {
            self.start_text_item();
        }
        self.emit(
            "response.output_text.delta",
            json!({
                "item_id": self.item_id,
                "output_index": self.output_index,
                "content_index": 0,
                "delta": s,
            }),
        );
    }

    fn start_text_item(&mut self) {
        self.item_id = rand_id("msg_");
        self.text_started = true;
        self.open_item = true;
        self.emit(
            "response.output_item.added",
            json!({
                "output_index": self.output_index,
                "item": {
                    "id": self.item_id,
                    "type": "message",
                    "status": "in_progress",
                    "role": "assistant",
                    "content": [],
                },
            }),
        );
        self.emit(
            "response.content_part.added",
            json!({
                "item_id": self.item_id,
                "output_index": self.output_index,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": []},
            }),
        );
    }

    fn block_stop(&mut self) {
        match self.block_type.as_str() {
            "text" => self.text_stop(),
            "thinking" => {
                let full = self.think_buf.clone();
                self.emit(
                    "response.reasoning_summary_part.done",
                    json!({
                        "item_id": self.item_id,
                        "output_index": self.output_index,
                        "summary_index": 0,
                        "part": {"type": "summary_text", "text": full},
                    }),
                );
                let item = json!({
                    "id": self.item_id,
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": full}],
                    "encrypted_content": encode_reasoning(&full, &self.sig_buf),
                });
                self.emit(
                    "response.output_item.done",
                    json!({"output_index": self.output_index, "item": item}),
                );
                self.output.push(item);
                self.open_item = false;
            }
            "redacted_thinking" => {
                let item = json!({
                    "id": self.item_id,
                    "type": "reasoning",
                    "summary": [],
                    "encrypted_content": encode_redacted_reasoning(&self.redacted_data),
                });
                self.emit(
                    "response.output_item.done",
                    json!({"output_index": self.output_index, "item": item}),
                );
                self.output.push(item);
                self.open_item = false;
            }
            "tool_use" => {
                let mut args = self.args_buf.clone();
                if args.is_empty() || serde_json::from_str::<Value>(&args).is_err() {
                    args = "{}".to_string();
                }
                self.emit(
                    "response.function_call_arguments.done",
                    json!({
                        "item_id": self.item_id,
                        "output_index": self.output_index,
                        "arguments": args,
                    }),
                );
                let item = json!({
                    "id": self.item_id,
                    "type": "function_call",
                    "call_id": self.call_id,
                    "name": self.tool_name,
                    "arguments": args,
                    "status": "completed",
                });
                self.emit(
                    "response.output_item.done",
                    json!({"output_index": self.output_index, "item": item}),
                );
                self.output.push(item);
                self.open_item = false;
            }
            "server_tool_use" => {
                let query = serde_json::from_str::<Value>(&self.args_buf)
                    .ok()
                    .and_then(|v| v.get("query").and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_default();
                self.search_query = query;
                self.emit(
                    "response.web_search_call.searching",
                    json!({"output_index": self.output_index, "item_id": self.item_id}),
                );
            }
            "web_search_tool_result" => self.close_search_item("completed"),
            _ => {}
        }
        self.block_type.clear();
    }

    fn text_stop(&mut self) {
        if self.text_hold {
            let cur = std::mem::take(&mut self.text_buf);
            self.text_hold = false;
            if cur.is_empty() {
                return;
            }
            // Blocks matching the marker (or a strict prefix of it, which can
            // only be a truncated status text) are held until the next block
            // decides whether this was a web-search preamble.
            if cur.starts_with(&self.marker) || self.marker.starts_with(&cur) {
                self.pending_status = cur;
                self.pending_status_ready = true;
                return;
            }
            // Ambiguous buffer that turned out not to be a preamble: emit whole.
            self.start_text_item();
            self.emit(
                "response.output_text.delta",
                json!({
                    "item_id": self.item_id,
                    "output_index": self.output_index,
                    "content_index": 0,
                    "delta": cur,
                }),
            );
        }
        if !self.text_started {
            return;
        }
        let full = self.text_buf.clone();
        self.emit(
            "response.content_part.done",
            json!({
                "item_id": self.item_id,
                "output_index": self.output_index,
                "content_index": 0,
                "part": {"type": "output_text", "text": full, "annotations": []},
            }),
        );
        let item = json!({
            "id": self.item_id,
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{"type": "output_text", "text": full, "annotations": []}],
        });
        self.emit(
            "response.output_item.done",
            json!({"output_index": self.output_index, "item": item}),
        );
        self.output.push(item);
        self.open_item = false;
    }

    /// Emits a held status-candidate block as a normal (complete) assistant
    /// message item.
    fn flush_pending_status_text(&mut self) {
        let text = std::mem::take(&mut self.pending_status);
        self.pending_status_ready = false;
        if text.is_empty() {
            return;
        }
        self.output_index = self.output.len();
        self.start_text_item();
        self.emit(
            "response.output_text.delta",
            json!({
                "item_id": self.item_id,
                "output_index": self.output_index,
                "content_index": 0,
                "delta": text,
            }),
        );
        self.emit(
            "response.content_part.done",
            json!({
                "item_id": self.item_id,
                "output_index": self.output_index,
                "content_index": 0,
                "part": {"type": "output_text", "text": text, "annotations": []},
            }),
        );
        let item = json!({
            "id": self.item_id,
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{"type": "output_text", "text": text, "annotations": []}],
        });
        self.emit(
            "response.output_item.done",
            json!({"output_index": self.output_index, "item": item}),
        );
        self.output.push(item);
        self.open_item = false;
    }

    fn close_search_item(&mut self, status: &str) {
        if !self.open_search {
            return;
        }
        self.open_search = false;
        self.open_item = false;
        self.emit(
            "response.web_search_call.completed",
            json!({"output_index": self.output_index, "item_id": self.item_id}),
        );
        let mut action = Map::new();
        action.insert("type".to_string(), json!("search"));
        let mut query = self.search_query.clone();
        if query.is_empty() {
            query = self.salvaged_query.clone();
        }
        if !query.is_empty() {
            action.insert("query".to_string(), json!(query));
        }
        if !self.sources.is_empty() {
            action.insert("sources".to_string(), json!(self.sources));
        }
        let item = json!({
            "id": self.item_id,
            "type": "web_search_call",
            "status": status,
            "action": Value::Object(action),
        });
        self.emit(
            "response.output_item.done",
            json!({"output_index": self.output_index, "item": item}),
        );
        self.output.push(item);
    }

    /// Closes any dangling items. Called on message_stop and at stream end.
    fn finish(&mut self) {
        if self.response_id.is_empty() {
            return;
        }
        if self.pending_status_ready {
            self.flush_pending_status_text();
        }
        if self.open_search {
            self.close_search_item("completed");
        }
        if self.open_item {
            // Upstream ended mid-block; close the item with what we have.
            match self.block_type.as_str() {
                "text" => self.text_stop(),
                "thinking" | "redacted_thinking" | "tool_use" => self.block_stop(),
                _ => {}
            }
            self.open_item = false;
        }
        let mut resp = if self.stop_reason == "max_tokens" {
            let mut r = self.response_shell("incomplete");
            r["incomplete_details"] = json!({"reason": "max_output_tokens"});
            r
        } else {
            self.response_shell("completed")
        };
        resp["usage"] = usage_map(
            self.usage_in,
            self.usage_out,
            self.usage_cache_create,
            self.usage_cached,
            self.usage_think,
        );
        if self.stop_reason == "max_tokens" {
            self.emit("response.incomplete", json!({"response": resp}));
        } else {
            self.emit("response.completed", json!({"response": resp}));
        }
        self.response_id.clear(); // prevent a second terminal event
    }

    fn fail(&mut self, code: &str, message: &str) {
        if self.terminated {
            return;
        }
        self.terminated = true;
        if self.response_id.is_empty() {
            self.response_id = rand_id("resp_");
            self.created_at = now_unix();
        }
        let mut resp = self.response_shell("failed");
        resp["error"] = json!({"code": code, "message": message});
        resp["usage"] = usage_map(
            self.usage_in,
            self.usage_out,
            self.usage_cache_create,
            self.usage_cached,
            self.usage_think,
        );
        self.emit("response.failed", json!({"response": resp}));
        self.response_id.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::reasoning::decode_reasoning;
    use crate::adapter::test_config;

    struct EmittedEvent {
        typ: String,
        data: Value,
    }

    fn parse_events(sse: &str) -> Vec<EmittedEvent> {
        let mut events = Vec::new();
        for chunk in sse.split("\n\n") {
            let chunk = chunk.trim();
            if chunk.is_empty() {
                continue;
            }
            let mut typ = String::new();
            let mut data = Value::Null;
            for line in chunk.split('\n') {
                if let Some(rest) = line.strip_prefix("data:") {
                    let d: Value = serde_json::from_str(rest.strip_prefix(' ').unwrap_or(rest))
                        .unwrap_or_else(|e| panic!("bad event JSON: {e}\n{line}"));
                    typ = d
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    data = d;
                }
            }
            events.push(EmittedEvent { typ, data });
        }
        events
    }

    fn events_of_type<'a>(events: &'a [EmittedEvent], typ: &str) -> Vec<&'a EmittedEvent> {
        events.iter().filter(|e| e.typ == typ).collect()
    }

    // Canned Kimi stream: search status text, server_tool_use, tool result,
    // thinking with signature, final answer text, and a client tool call.
    const KIMI_STREAM: &str = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"k3\",\"usage\":{\"input_tokens\":120,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Search results for query: best\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" pizza places\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"server_tool_use\",\"id\":\"srvtoolu_1\",\"name\":\"web_search\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\": \\\"best pizza\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\" places\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"web_search_tool_result\",\"tool_use_id\":\"srvtoolu_1\",\"content\":[{\"type\":\"web_search_result\",\"url\":\"https://example.com/pizza\",\"title\":\"Pizza\"}]}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":3,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"The user wants pizza. \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"I found results.\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"kimi-sig-\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"xyz\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":3}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":4,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":4,\"delta\":{\"type\":\"text_delta\",\"text\":\"Here are \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":4,\"delta\":{\"type\":\"text_delta\",\"text\":\"the best pizza places.\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":4}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":5,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"shell\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":5,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\":\\\"ls\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":5}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":88}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    fn run_stream(cfg: &Config, upstream: &str) -> Vec<EmittedEvent> {
        let req: ResponsesRequest =
            serde_json::from_str(r#"{"model":"k3","stream":true,"input":"hi"}"#).unwrap();
        let mut out = String::new();
        translate_sse(cfg, &req, upstream.as_bytes(), |s| out.push_str(&s)).unwrap();
        parse_events(&out)
    }

    #[test]
    fn stream_search_status_suppressed() {
        let events = run_stream(&test_config(), KIMI_STREAM);
        let mut text = String::new();
        for e in events_of_type(&events, "response.output_text.delta") {
            text.push_str(e.data["delta"].as_str().unwrap());
        }
        assert!(
            !text.contains("Search results for query:"),
            "search status text leaked into output: {text:?}"
        );
        assert_eq!(text, "Here are the best pizza places.");
    }

    #[test]
    fn stream_web_search_call() {
        let events = run_stream(&test_config(), KIMI_STREAM);
        let mut search_item: Option<&Value> = None;
        for e in events_of_type(&events, "response.output_item.done") {
            let item = &e.data["item"];
            if item["type"] == "web_search_call" {
                search_item = Some(item);
            }
        }
        let search_item = search_item.expect("no web_search_call item emitted");
        assert_eq!(search_item["status"], "completed");
        let action = &search_item["action"];
        assert_eq!(action["query"], "best pizza places");
        let sources = action["sources"].as_array().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0]["url"], "https://example.com/pizza");
    }

    #[test]
    fn stream_reasoning_encrypted_content() {
        let events = run_stream(&test_config(), KIMI_STREAM);
        let mut reasoning: Option<&Value> = None;
        for e in events_of_type(&events, "response.output_item.done") {
            let item = &e.data["item"];
            if item["type"] == "reasoning" {
                reasoning = Some(item);
            }
        }
        let reasoning = reasoning.expect("no reasoning item emitted");
        let enc = reasoning["encrypted_content"].as_str().unwrap();
        let p = decode_reasoning(enc, "").expect("encrypted_content does not decode");
        assert_eq!(p.thinking, "The user wants pizza. I found results.");
        assert_eq!(p.signature, "kimi-sig-xyz");
        let mut summary = String::new();
        for e in events_of_type(&events, "response.reasoning_summary_text.delta") {
            summary.push_str(e.data["delta"].as_str().unwrap());
        }
        assert_eq!(summary, p.thinking);
    }

    #[test]
    fn stream_function_call() {
        let events = run_stream(&test_config(), KIMI_STREAM);
        let done_args = events_of_type(&events, "response.function_call_arguments.done");
        assert_eq!(done_args.len(), 1);
        assert_eq!(done_args[0].data["arguments"], r#"{"cmd":"ls"}"#);
        let mut fc: Option<&Value> = None;
        for e in events_of_type(&events, "response.output_item.done") {
            let item = &e.data["item"];
            if item["type"] == "function_call" {
                fc = Some(item);
            }
        }
        let fc = fc.expect("no function_call item");
        assert_eq!(fc["call_id"], "toolu_1");
        assert_eq!(fc["name"], "shell");
    }

    #[test]
    fn stream_completed_usage() {
        let events = run_stream(&test_config(), KIMI_STREAM);
        let completed = events_of_type(&events, "response.completed");
        assert_eq!(
            completed.len(),
            1,
            "expected exactly one response.completed"
        );
        let resp = &completed[0].data["response"];
        let usage = &resp["usage"];
        assert_eq!(usage["input_tokens"], 120);
        assert_eq!(usage["output_tokens"], 88);
        let output = resp["output"].as_array().unwrap();
        assert_eq!(output.len(), 4, "expected 4 output items: {output:?}");
        assert_eq!(resp["status"], "completed");
    }

    #[test]
    fn stream_max_tokens_incomplete() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":4096}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        assert_eq!(events_of_type(&events, "response.incomplete").len(), 1);
        assert_eq!(events_of_type(&events, "response.completed").len(), 0);
    }

    #[test]
    fn stream_upstream_error() {
        let upstream = concat!(
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        let failed = events_of_type(&events, "response.failed");
        assert_eq!(failed.len(), 1);
        let resp = &failed[0].data["response"];
        assert_eq!(resp["error"]["code"], "overloaded_error");
        assert_eq!(resp["error"]["message"], "Overloaded");
    }

    // Text matching the search-status marker but NOT followed by a
    // server_tool_use block must be emitted as normal output (sub2api#5166).
    #[test]
    fn stream_status_prefix_without_search_is_kept() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Search results for query: test\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"second block\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        let mut text = String::new();
        for e in events_of_type(&events, "response.output_text.delta") {
            text.push_str(e.data["delta"].as_str().unwrap());
        }
        assert!(
            text.contains("Search results for query: test"),
            "non-search text with marker prefix was wrongly suppressed: {text:?}"
        );
        assert!(
            text.contains("second block"),
            "missing second block: {text:?}"
        );
    }

    // A stream that ends without message_stop must surface response.failed.
    #[test]
    fn stream_premature_eof_fails() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        assert_eq!(events_of_type(&events, "response.failed").len(), 1);
        assert_eq!(events_of_type(&events, "response.completed").len(), 0);
    }

    // ---- positive cases ----

    #[test]
    fn stream_sequence_numbers_monotonic() {
        let events = run_stream(&test_config(), KIMI_STREAM);
        for (i, e) in events.iter().enumerate() {
            let seq = e.data["sequence_number"].as_i64().unwrap_or(-1);
            assert_eq!(seq, i as i64, "sequence_number not monotonic at {i}");
        }
    }

    #[test]
    fn stream_marker_split_across_deltas() {
        // The status marker arriving in pieces must still be recognized.
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Search resu\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lts for query: golang\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"server_tool_use\",\"name\":\"web_search\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        assert_eq!(
            events_of_type(&events, "response.output_text.delta").len(),
            0,
            "split-marker status text leaked"
        );
        let mut search_item: Option<&Value> = None;
        for e in events_of_type(&events, "response.output_item.done") {
            let item = &e.data["item"];
            if item["type"] == "web_search_call" {
                search_item = Some(item);
            }
        }
        let search_item = search_item.expect("web_search_call missing");
        assert_eq!(search_item["action"]["query"], "golang");
    }

    #[test]
    fn stream_search_without_result_closed_at_stop() {
        // server_tool_use without a following web_search_tool_result must
        // still be closed at message_stop rather than leak as in_progress.
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"server_tool_use\",\"name\":\"web_search\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        let mut item: Option<&Value> = None;
        for e in events_of_type(&events, "response.output_item.done") {
            if e.data["item"]["type"] == "web_search_call" {
                item = Some(&e.data["item"]);
            }
        }
        let item = item.expect("dangling web_search_call not closed");
        assert_eq!(item["status"], "completed");
    }

    #[test]
    fn stream_redacted_thinking() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"opaque-blob\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        let mut reasoning: Option<&Value> = None;
        for e in events_of_type(&events, "response.output_item.done") {
            if e.data["item"]["type"] == "reasoning" {
                reasoning = Some(&e.data["item"]);
            }
        }
        let reasoning = reasoning.expect("no reasoning item for redacted_thinking");
        let p = decode_reasoning(reasoning["encrypted_content"].as_str().unwrap(), "")
            .expect("redacted payload does not decode");
        assert_eq!(p.redacted, "opaque-blob");
    }

    #[test]
    fn stream_tool_use_empty_args() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"noop\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        let done = events_of_type(&events, "response.function_call_arguments.done");
        assert_eq!(done.len(), 1);
        assert_eq!(
            done[0].data["arguments"], "{}",
            "empty args should become {{}}"
        );
    }

    #[test]
    fn stream_usage_cached_and_thinking_tokens() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":239,\"cache_read_input_tokens\":7680,\"output_tokens\":0}}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":239,\"cache_read_input_tokens\":7680,\"output_tokens\":44,\"output_tokens_details\":{\"thinking_tokens\":28}}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        let completed = events_of_type(&events, "response.completed");
        assert_eq!(completed.len(), 1, "no response.completed");
        let usage = &completed[0].data["response"]["usage"];
        // Responses semantics: input_tokens includes cached tokens.
        assert_eq!(usage["input_tokens"], 7919);
        assert_eq!(usage["input_tokens_details"]["cached_tokens"], 7680);
        assert_eq!(usage["output_tokens_details"]["reasoning_tokens"], 28);
    }

    // ---- negative cases ----

    // Real Kimi payloads carry JSON nulls ("stop_reason":null in
    // message_start, "stop_sequence":null in message_delta). Go's
    // encoding/json tolerates them; the translator must too, or the
    // message_start event fails to parse and no response.created /
    // response.completed is ever emitted (regression test).
    #[test]
    fn stream_null_fields_tolerated() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"k3-256k\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":0,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":7919,\"output_tokens\":0,\"service_tier\":\"standard\",\"inference_geo\":\"not_available\"}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"pong\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":null,\"cache_read_input_tokens\":7919,\"output_tokens\":12,\"output_tokens_details\":null}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        assert_eq!(events_of_type(&events, "response.created").len(), 1);
        assert_eq!(events_of_type(&events, "response.in_progress").len(), 1);
        let completed = events_of_type(&events, "response.completed");
        assert_eq!(
            completed.len(),
            1,
            "response.completed missing: null fields broke parsing"
        );
        let resp = &completed[0].data["response"];
        assert_eq!(resp["status"], "completed");
        assert_eq!(resp["usage"]["input_tokens"], 7919);
        assert_eq!(resp["usage"]["output_tokens"], 12);
        let mut text = String::new();
        for e in events_of_type(&events, "response.output_text.delta") {
            text.push_str(e.data["delta"].as_str().unwrap());
        }
        assert_eq!(text, "pong");
    }

    #[test]
    fn stream_malformed_data_line_skipped() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "data: {not valid json\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        assert_eq!(
            events_of_type(&events, "response.completed").len(),
            1,
            "stream should survive a malformed data line"
        );
    }

    #[test]
    fn stream_error_mid_stream_fails() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"boom\"}}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        assert_eq!(events_of_type(&events, "response.failed").len(), 1);
        assert_eq!(events_of_type(&events, "response.completed").len(), 0);
    }

    #[test]
    fn stream_empty_text_block_emits_nothing() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let events = run_stream(&test_config(), upstream);
        assert_eq!(
            events_of_type(&events, "response.output_item.added").len(),
            0,
            "empty text block should emit no items"
        );
        let completed = events_of_type(&events, "response.completed");
        assert_eq!(completed.len(), 1, "no response.completed");
        let out = completed[0].data["response"]["output"].as_array().unwrap();
        assert!(out.is_empty(), "output should be empty: {out:?}");
    }
}
