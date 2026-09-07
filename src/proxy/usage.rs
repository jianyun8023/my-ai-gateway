//! Token usage extraction shared by native passthrough and protocol adapters.
//!
//! Providers use slightly different field names.  This module deliberately
//! accepts the OpenAI Chat/Responses and Anthropic Messages shapes without
//! attempting to interpret response content.

use axum::body::{Body, Bytes};
use futures_util::{stream, StreamExt};
use serde_json::Value;
use std::time::Instant;

use super::stream::{is_gateway_heartbeat, SseEventTracker, StreamTermination, HEARTBEAT_MARKER};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageReport {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
    pub source: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct StreamObservation {
    pub captured: Vec<u8>,
    pub ttft_ms: Option<i64>,
    pub failed: bool,
    pub termination: StreamTermination,
}

struct ObservationGuard<F>
where
    F: FnOnce(StreamObservation) + Send + 'static,
{
    callback: Option<F>,
    captured: Vec<u8>,
    ttft_ms: Option<i64>,
    termination: Option<StreamTermination>,
    tracker: SseEventTracker,
    request_started: Instant,
}

impl<F> ObservationGuard<F>
where
    F: FnOnce(StreamObservation) + Send + 'static,
{
    fn new(callback: F, request_started: Instant) -> Self {
        Self {
            callback: Some(callback),
            captured: Vec::new(),
            ttft_ms: None,
            termination: None,
            tracker: SseEventTracker::default(),
            request_started,
        }
    }

    fn observe(&mut self, chunk: &Bytes) {
        let activity = self.tracker.feed(chunk);
        let provider_bytes = without_gateway_heartbeats(chunk);
        if !provider_bytes.is_empty() {
            self.captured.extend_from_slice(&provider_bytes);
        }
        if self.ttft_ms.is_none()
            && !(activity.error && !activity.provider_event)
            && has_provider_bytes(&provider_bytes)
        {
            self.ttft_ms = Some(self.request_started.elapsed().as_millis() as i64);
        }
        if activity.error {
            self.termination = Some(
                activity
                    .terminal
                    .filter(|value| value.is_failure())
                    .unwrap_or(StreamTermination::UpstreamError),
            );
        } else if let Some(terminal) = activity.terminal {
            self.termination = Some(terminal);
        }
    }

    fn complete(&mut self, termination: StreamTermination) {
        if self.callback.is_none() {
            return;
        }
        let termination = self.termination.unwrap_or(termination);
        self.termination = Some(termination);
        if let Some(callback) = self.callback.take() {
            tracing::info!(
                stream_termination = termination.code(),
                streamed_bytes = self.captured.len(),
                ttft_ms = ?self.ttft_ms,
                "stream lifecycle"
            );
            callback(StreamObservation {
                captured: std::mem::take(&mut self.captured),
                ttft_ms: self.ttft_ms,
                failed: termination.is_failure(),
                termination,
            });
        }
    }
}

fn has_provider_bytes(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let text = String::from_utf8_lossy(bytes);
    text.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !line.starts_with(':')
    }) || (!text.contains('\n') && !text.trim().is_empty())
}

fn without_gateway_heartbeats(chunk: &Bytes) -> Vec<u8> {
    if is_gateway_heartbeat(chunk) {
        return Vec::new();
    }
    if !String::from_utf8_lossy(chunk).contains(HEARTBEAT_MARKER) {
        return chunk.to_vec();
    }
    let text = String::from_utf8_lossy(chunk);
    let mut filtered = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if content.trim() == HEARTBEAT_MARKER {
            continue;
        }
        filtered.push_str(line);
    }
    if !text.ends_with('\n')
        && text
            .rsplit_once('\n')
            .is_some_and(|(_, tail)| tail.trim() == HEARTBEAT_MARKER)
    {
        filtered = filtered.trim_end_matches(HEARTBEAT_MARKER).to_owned();
    }
    filtered.into_bytes()
}

impl<F> Drop for ObservationGuard<F>
where
    F: FnOnce(StreamObservation) + Send + 'static,
{
    fn drop(&mut self) {
        // A response body can be dropped without ever being polled to EOF.
        // That is the reliable signal that the downstream client went away;
        // dropping the inner Reqwest body at the same time cancels the read.
        self.complete(StreamTermination::ClientCancelled);
    }
}

/// Tee a live response body for usage parsing while forwarding every chunk in
/// the same order. The upstream is polled once per downstream demand, so this
/// does not prefetch the response or alter backpressure. Completion is reported
/// on clean EOF or immediately on a body-stream error.
pub fn observe_stream_body<F>(body: Body, request_started: Instant, on_complete: F) -> Body
where
    F: FnOnce(StreamObservation) + Send + 'static,
{
    let upstream = body.into_data_stream();
    let guard = ObservationGuard::new(on_complete, request_started);
    let stream = stream::unfold((upstream, guard), |(mut upstream, mut guard)| async move {
        match upstream.next().await {
            Some(Ok(chunk)) => {
                guard.observe(&chunk);
                Some((Ok::<Bytes, std::io::Error>(chunk), (upstream, guard)))
            }
            Some(Err(error)) => {
                guard.complete(StreamTermination::UpstreamError);
                Some((
                    Err(std::io::Error::other(error.to_string())),
                    (upstream, guard),
                ))
            }
            None => {
                let activity = guard.tracker.finish_eof();
                if activity.error {
                    guard.complete(StreamTermination::UpstreamError);
                } else if guard.tracker.saw_provider_event() {
                    let termination = guard.tracker.terminal().unwrap_or_else(|| {
                        if guard.tracker.saw_sse_frame() {
                            StreamTermination::UpstreamError
                        } else {
                            StreamTermination::Completed
                        }
                    });
                    guard.complete(termination);
                } else {
                    guard.complete(StreamTermination::EmptyStream);
                }
                None
            }
        }
    });
    Body::from_stream(stream)
}

/// Conservative fallback estimate used when an upstream omits usage.
/// This is intentionally marked as estimated; it is not a billing value.
pub fn estimate(input: &[u8], output: &[u8]) -> UsageReport {
    let tokenizer = tiktoken_rs::cl100k_base().expect("cl100k tokenizer initialization");
    let input_text = String::from_utf8_lossy(input);
    let output_text = String::from_utf8_lossy(output);
    let input_tokens = tokenizer.encode_with_special_tokens(&input_text).len() as i64;
    let output_tokens = tokenizer.encode_with_special_tokens(&output_text).len() as i64;
    UsageReport {
        input_tokens,
        output_tokens,
        total_tokens: input_tokens + output_tokens,
        source: "estimated".into(),
        ..Default::default()
    }
}

/// Scan a captured OpenAI Chat Completions stream for `<think>...</think>`
/// blocks emitted by MiniMax and return their token count.  MiniMax-M3
/// embeds the model's reasoning directly inside `delta.content` and never
/// reports it under `output_tokens_details.reasoning_tokens`, so the
/// generic `extract_sse` always sees `usage:null` chunks (issue #99).
///
/// The scanner is byte-oriented so it stays allocation-light on large
/// streams; it only materialises the concatenated text between the matching
/// tags and tokenises that substring once.
pub fn minimax_chat_thinking_tokens(captured: &[u8]) -> i64 {
    let tokenizer = match tiktoken_rs::cl100k_base() {
        Ok(t) => t,
        Err(_) => return 0,
    };
    let haystack = String::from_utf8_lossy(captured);
    let mut total: i64 = 0;
    let mut cursor = 0usize;
    while let Some(open_rel) = haystack[cursor..].find("<think>") {
        let open_abs = cursor + open_rel + "<think>".len();
        let Some(close_rel) = haystack[open_abs..].find("</think>") else {
            break;
        };
        let close_abs = open_abs + close_rel;
        let text = &haystack[open_abs..close_abs];
        total += tokenizer.encode_with_special_tokens(text).len() as i64;
        cursor = close_abs + "</think>".len();
    }
    total
}

fn merge_thinking_into_report(report: &mut UsageReport, captured: &[u8]) {
    let reasoning = minimax_chat_thinking_tokens(captured);
    if reasoning <= 0 {
        return;
    }
    if report.reasoning_tokens == 0 {
        report.reasoning_tokens = reasoning;
    }
    // `total_tokens` excludes `reasoning_tokens` per the documented contract
    // (see migrations/0018_document_token_count_semantics.sql), so do not
    // touch the total here.  Downstream billing that wants to count
    // thinking tokens adds `reasoning_tokens` on top of `total_tokens`.
}

impl UsageReport {
    pub fn missing() -> Self {
        Self {
            source: "missing".into(),
            ..Default::default()
        }
    }

    pub fn is_present(&self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.reasoning_tokens > 0
            || self.cached_tokens > 0
            || self.cache_read_tokens > 0
            || self.cache_creation_tokens > 0
            || self.total_tokens > 0
    }
}

fn i64_at(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// Extract usage from a complete JSON response.
pub fn extract_json(value: &Value) -> Option<UsageReport> {
    // OpenAI Chat/Responses put usage at the top level.  Some adapters wrap
    // the actual response under `response`, so inspect that as a fallback.
    let usage = value
        .get("usage")
        .or_else(|| value.get("response").and_then(|v| v.get("usage")))
        .or_else(|| value.get("message").and_then(|v| v.get("usage")))
        .or_else(|| value.get("delta").and_then(|v| v.get("usage")))?;

    let input = if usage.get("input_tokens").is_some() {
        i64_at(usage, "input_tokens")
    } else {
        i64_at(usage, "prompt_tokens")
    };
    let output = if usage.get("output_tokens").is_some() {
        i64_at(usage, "output_tokens")
    } else {
        i64_at(usage, "completion_tokens")
    };
    let reasoning = usage
        .get("output_tokens_details")
        .map(|v| i64_at(v, "reasoning_tokens"))
        .or_else(|| {
            usage
                .get("completion_tokens_details")
                .map(|v| i64_at(v, "reasoning_tokens"))
        })
        .unwrap_or_else(|| i64_at(usage, "reasoning_tokens"));
    let (cache_read, cache_creation) = if let Some(details) = usage
        .get("input_tokens_details")
        .or_else(|| usage.get("prompt_tokens_details"))
    {
        (i64_at(details, "cached_tokens"), 0i64)
    } else {
        let read = i64_at(usage, "cache_read_input_tokens") + i64_at(usage, "cached_tokens");
        let creation = i64_at(usage, "cache_creation_input_tokens");
        (read, creation)
    };
    let cached = cache_read + cache_creation;
    let total = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(input + output);
    let report = UsageReport {
        input_tokens: input,
        output_tokens: output,
        reasoning_tokens: reasoning,
        cached_tokens: cached,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_creation,
        total_tokens: total,
        source: "upstream".into(),
    };
    report.is_present().then_some(report)
}

pub fn extract_json_bytes(bytes: &[u8]) -> Option<UsageReport> {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| extract_json(&value))
}

/// Resolve usage for a completed JSON response. Failed responses without
/// provider-confirmed usage remain explicitly missing and are never estimated.
pub fn usage_for_json_response(success: bool, request: &[u8], response: &[u8]) -> UsageReport {
    extract_json_bytes(response).unwrap_or_else(|| {
        if success {
            estimate(request, response)
        } else {
            UsageReport::missing()
        }
    })
}

/// Extract the last usage-bearing event from an SSE payload.  This handles
/// `data: {...}` and ignores comments/keep-alives and `[DONE]`.
#[allow(dead_code)]
pub fn extract_sse(text: &str) -> Option<UsageReport> {
    let mut latest: Option<UsageReport> = None;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(data) {
            if let Some(report) = extract_json(&value) {
                if let Some(current) = &mut latest {
                    if report.input_tokens > 0 {
                        current.input_tokens = report.input_tokens;
                    }
                    if report.output_tokens > 0 {
                        current.output_tokens = report.output_tokens;
                    }
                    if report.reasoning_tokens > 0 {
                        current.reasoning_tokens = report.reasoning_tokens;
                    }
                    if report.cached_tokens > 0 {
                        current.cached_tokens = report.cached_tokens;
                    }
                    if report.cache_read_tokens > 0 {
                        current.cache_read_tokens = report.cache_read_tokens;
                    }
                    if report.cache_creation_tokens > 0 {
                        current.cache_creation_tokens = report.cache_creation_tokens;
                    }
                    if report.total_tokens > 0 {
                        current.total_tokens = report.total_tokens;
                    }
                } else {
                    latest = Some(report);
                }
            }
        }
    }
    latest.map(|mut report| {
        report.source = "parsed".into();
        report
    })
}

/// Resolve usage after an SSE response ends. A failed stream with no
/// provider-confirmed usage remains missing instead of inventing tokens.
///
/// When `extract_sse` returns `None` and we have to fall back to the
/// tiktoken-based `estimate`, log metadata-only diagnostics for issue #98.
/// Never emit response bytes, reversible previews, or body fingerprints.
pub fn usage_for_sse_response(
    request_id: &str,
    success: bool,
    request: &[u8],
    captured: &[u8],
) -> UsageReport {
    let text = String::from_utf8_lossy(captured);
    let mut report = extract_sse(&text).unwrap_or_else(|| {
        log_sse_extraction_failure(request_id, success, captured);
        if success {
            estimate(request, captured)
        } else {
            UsageReport::missing()
        }
    });
    // MiniMax-M3 emits thinking text inside delta.content wrapped in
    // `<think>...</think>` and never reports reasoning_tokens upstream.
    // Scan the captured stream and credit those tokens so per-vendor
    // reasoning usage is not silently zero (issue #99).
    merge_thinking_into_report(&mut report, captured);
    report
}

fn log_sse_extraction_failure(request_id: &str, success: bool, captured: &[u8]) {
    let text = String::from_utf8_lossy(captured);
    let mut data_line_count = 0usize;
    let mut json_parse_failures = 0usize;
    let mut json_without_usage = 0usize;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        data_line_count += 1;
        match serde_json::from_str::<Value>(data) {
            Ok(value) => {
                if value.get("usage").is_none()
                    && value.get("response").and_then(|v| v.get("usage")).is_none()
                    && value.get("message").and_then(|v| v.get("usage")).is_none()
                    && value.get("delta").and_then(|v| v.get("usage")).is_none()
                {
                    json_without_usage += 1;
                }
            }
            Err(_) => json_parse_failures += 1,
        }
    }
    tracing::warn!(
        request_id = %request_id,
        success,
        captured_len = captured.len(),
        line_count = text.lines().count(),
        data_line_count,
        json_parse_failures,
        json_without_usage,
        usage_source = if success { "estimated" } else { "missing" },
        "SSE usage extraction failed; no provider-confirmed usage"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use futures_util::stream;
    use serde_json::json;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    #[test]
    fn sse_extraction_diagnostics_only_emit_metadata() {
        use std::collections::BTreeMap;
        use std::sync::Mutex;
        use tracing::field::{Field, Visit};
        use tracing_subscriber::{layer::Context, prelude::*, Layer};

        #[derive(Clone, Default)]
        struct Events(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

        struct Fields(BTreeMap<String, String>);
        impl Visit for Fields {
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                self.0.insert(field.name().into(), format!("{value:?}"));
            }
        }
        impl<S: tracing::Subscriber> Layer<S> for Events {
            fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
                let mut fields = Fields(BTreeMap::new());
                event.record(&mut fields);
                self.0.lock().unwrap().push(fields.0);
            }
        }

        let events = Events::default();
        let subscriber = tracing_subscriber::registry().with(events.clone());
        tracing::subscriber::with_default(subscriber, || {
            for success in [true, false] {
                let report = usage_for_sse_response(
                    "diagnostic-request",
                    success,
                    br#"{"input":"PRIVATE-PROMPT"}"#,
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"PRIVATE-RESPONSE\"}}]}\n\ndata: {invalid}\n\ndata: [DONE]\n\n",
                );
                assert_eq!(report.source, if success { "estimated" } else { "missing" });
                if !success {
                    assert_eq!(report.total_tokens, 0);
                }
            }
        });
        let records = events.0.lock().unwrap();
        assert_eq!(records.len(), 2, "diagnostics must actually be captured");
        let allowed = [
            "captured_len",
            "data_line_count",
            "json_parse_failures",
            "json_without_usage",
            "line_count",
            "message",
            "request_id",
            "success",
            "usage_source",
        ];
        for record in records.iter() {
            assert_eq!(
                record.keys().map(String::as_str).collect::<Vec<_>>(),
                allowed
            );
            assert_eq!(record["data_line_count"], "2");
            assert_eq!(record["json_parse_failures"], "1");
            assert_eq!(record["json_without_usage"], "1");
            assert!(!format!("{record:?}").contains("PRIVATE-"));
        }
    }

    #[test]
    fn extracts_openai_chat_usage() {
        let report = extract_json(&json!({
            "usage": {"prompt_tokens": 12, "completion_tokens": 8, "total_tokens": 20,
                "prompt_tokens_details": {"cached_tokens": 3},
                "completion_tokens_details": {"reasoning_tokens": 2}}
        }))
        .unwrap();
        assert_eq!(report.input_tokens, 12);
        assert_eq!(report.output_tokens, 8);
        assert_eq!(report.cached_tokens, 3);
        assert_eq!(report.reasoning_tokens, 2);
        assert_eq!(report.total_tokens, 20);
    }

    #[test]
    fn extracts_anthropic_usage() {
        let report = extract_json(&json!({
            "usage": {"input_tokens": 10, "output_tokens": 5,
                "cache_read_input_tokens": 4, "cache_creation_input_tokens": 2}
        }))
        .unwrap();
        assert_eq!(report.input_tokens, 10);
        assert_eq!(report.output_tokens, 5);
        assert_eq!(report.cached_tokens, 6);
        assert_eq!(report.cache_read_tokens, 4);
        assert_eq!(report.cache_creation_tokens, 2);
        assert_eq!(report.total_tokens, 15);
    }

    #[test]
    fn extracts_openai_cached_tokens_as_cache_read() {
        let report = extract_json(&json!({
            "usage": {"prompt_tokens": 12, "completion_tokens": 8, "total_tokens": 20,
                "prompt_tokens_details": {"cached_tokens": 5}}
        }))
        .unwrap();
        assert_eq!(report.cache_read_tokens, 5);
        assert_eq!(report.cache_creation_tokens, 0);
        assert_eq!(report.cached_tokens, 5);
    }

    #[test]
    fn extract_sse_returns_none_for_empty_captured() {
        assert!(extract_sse("").is_none());
    }

    #[test]
    fn extract_sse_returns_none_for_only_done() {
        assert!(extract_sse("data: [DONE]\n\n").is_none());
    }

    #[test]
    fn extract_sse_returns_none_for_invalid_json() {
        assert!(extract_sse("data: {invalid json}\n\n").is_none());
    }

    #[test]
    fn extract_sse_returns_none_for_valid_json_without_usage() {
        let text = "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n";
        assert!(extract_sse(text).is_none());
    }

    #[test]
    fn extract_sse_handles_cross_chunk_data_lines() {
        // Simulate what would happen if chunks split in the middle of a data line
        // After concatenation, the full text should still parse correctly
        let chunk1 = b"data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\nda";
        let chunk2 = b"ta: {\"id\":\"2\",\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n\n";
        let mut captured = Vec::new();
        captured.extend_from_slice(chunk1);
        captured.extend_from_slice(chunk2);
        let text = String::from_utf8_lossy(&captured);
        let report = extract_sse(&text).expect("should parse cross-chunk SSE");
        assert_eq!(report.input_tokens, 10);
        assert_eq!(report.output_tokens, 5);
        assert_eq!(report.total_tokens, 15);
    }

    #[test]
    fn extracts_last_sse_usage_event() {
        let report = extract_sse("data: {\"type\":\"response.output_text.delta\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":2,\"output_tokens\":3}}}\n\ndata: [DONE]\n").unwrap();
        assert_eq!(report.total_tokens, 5);
        assert_eq!(report.source, "parsed");
    }

    #[test]
    fn extracts_responses_nested_usage() {
        let report = extract_json(&json!({
            "type": "response.completed",
            "response": {"usage": {"input_tokens": 7, "output_tokens": 4,
                "input_tokens_details": {"cached_tokens": 2},
                "output_tokens_details": {"reasoning_tokens": 1}}}
        }))
        .unwrap();
        assert_eq!(report.input_tokens, 7);
        assert_eq!(report.output_tokens, 4);
        assert_eq!(report.reasoning_tokens, 1);
        assert_eq!(report.cached_tokens, 2);
        assert_eq!(report.total_tokens, 11);
    }

    #[test]
    fn extracts_anthropic_stream_nested_usage() {
        let report = extract_sse("event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":9}}}\n\nevent: message_delta\ndata: {\"delta\":{\"usage\":{\"output_tokens\":4}}}\n").unwrap();
        assert_eq!(report.input_tokens, 9);
        assert_eq!(report.output_tokens, 4);
    }

    #[test]
    fn extracts_anthropic_stream_cache_tokens_split() {
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01\",\"type\":\"message\",",
            "\"role\":\"assistant\",\"content\":[],\"model\":\"MiniMax-M3\",",
            "\"usage\":{\"input_tokens\":444,\"cache_creation_input_tokens\":0,",
            "\"cache_read_input_tokens\":295552}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},",
            "\"usage\":{\"output_tokens\":593}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let report = extract_sse(sse).unwrap();
        assert_eq!(report.source, "parsed");
        assert_eq!(report.input_tokens, 444);
        assert_eq!(report.output_tokens, 593);
        assert_eq!(report.cache_read_tokens, 295552);
        assert_eq!(report.cache_creation_tokens, 0);
        assert_eq!(report.cached_tokens, 295552);
    }

    #[test]
    fn extracts_anthropic_stream_cache_creation() {
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{",
            "\"input_tokens\":1200,\"cache_creation_input_tokens\":8000,",
            "\"cache_read_input_tokens\":0}}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},",
            "\"usage\":{\"output_tokens\":50}}\n\n",
        );
        let report = extract_sse(sse).unwrap();
        assert_eq!(report.input_tokens, 1200);
        assert_eq!(report.output_tokens, 50);
        assert_eq!(report.cache_read_tokens, 0);
        assert_eq!(report.cache_creation_tokens, 8000);
        assert_eq!(report.cached_tokens, 8000);
    }

    #[test]
    fn extracts_minimax_anthropic_stream_with_both_cache_fields() {
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_m3\",\"type\":\"message\",",
            "\"role\":\"assistant\",\"content\":[],\"model\":\"MiniMax-M3\",",
            "\"usage\":{\"input_tokens\":500,\"cache_creation_input_tokens\":2000,",
            "\"cache_read_input_tokens\":150000}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"test\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},",
            "\"usage\":{\"output_tokens\":200}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let report = extract_sse(sse).unwrap();
        assert_eq!(report.input_tokens, 500);
        assert_eq!(report.output_tokens, 200);
        assert_eq!(report.cache_read_tokens, 150000);
        assert_eq!(report.cache_creation_tokens, 2000);
        assert_eq!(report.cached_tokens, 152000);
    }

    #[test]
    fn extracts_minimax_real_sse_format_with_zero_message_start() {
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"06e7eb84618f\",\"type\":\"message\",",
            "\"role\":\"assistant\",\"content\":[],\"model\":\"MiniMax-M3\",",
            "\"stop_reason\":null,\"stop_sequence\":null,",
            "\"usage\":{\"input_tokens\":0,\"output_tokens\":0,\"service_tier\":\"standard\"},",
            "\"service_tier\":\"standard\"}}\n\n",
            "event: ping\n",
            "data: {\"type\":\"ping\"}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello!\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},",
            "\"usage\":{\"input_tokens\":842,\"output_tokens\":3,",
            "\"cache_read_input_tokens\":128,\"service_tier\":\"standard\"}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let report = extract_sse(sse).unwrap();
        assert_eq!(report.source, "parsed");
        assert_eq!(report.input_tokens, 842);
        assert_eq!(report.output_tokens, 3);
        assert_eq!(report.cache_read_tokens, 128);
        assert_eq!(report.cache_creation_tokens, 0);
        assert_eq!(report.cached_tokens, 128);
    }

    #[test]
    fn extracts_minimax_real_sse_without_cache() {
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"06e7eb84618f\",\"type\":\"message\",",
            "\"role\":\"assistant\",\"content\":[],\"model\":\"MiniMax-M3\",",
            "\"stop_reason\":null,\"stop_sequence\":null,",
            "\"usage\":{\"input_tokens\":0,\"output_tokens\":0,\"service_tier\":\"standard\"},",
            "\"service_tier\":\"standard\"}}\n\n",
            "event: ping\n",
            "data: {\"type\":\"ping\"}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},",
            "\"usage\":{\"input_tokens\":180,\"output_tokens\":3,\"service_tier\":\"standard\"}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let report = extract_sse(sse).unwrap();
        assert_eq!(report.source, "parsed");
        assert_eq!(report.input_tokens, 180);
        assert_eq!(report.output_tokens, 3);
        assert_eq!(report.cache_read_tokens, 0);
        assert_eq!(report.cached_tokens, 0);
    }

    #[test]
    fn sse_usage_for_response_preserves_cache_split() {
        let sse_bytes = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{",
            "\"input_tokens\":100,\"cache_read_input_tokens\":5000,",
            "\"cache_creation_input_tokens\":0}}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},",
            "\"usage\":{\"output_tokens\":20}}\n\n",
        );
        let report = usage_for_sse_response("test-cache-split", true, b"{}", sse_bytes.as_bytes());
        assert_eq!(report.source, "parsed");
        assert_eq!(report.cache_read_tokens, 5000);
        assert_eq!(report.cache_creation_tokens, 0);
        assert_eq!(report.cached_tokens, 5000);
    }

    #[test]
    fn estimates_with_tiktoken() {
        let report = estimate(br#"{\"input\":\"hello\"}"#, b"hello world");
        assert!(report.input_tokens > 0);
        assert!(report.output_tokens > 0);
        assert_eq!(report.source, "estimated");
    }

    #[test]
    fn usage_without_upstream_fields_is_missing_until_estimated() {
        assert!(extract_json(&json!({"usage": {}})).is_none());
        let report = estimate(b"input", b"output");
        assert_eq!(report.source, "estimated");
        assert!(report.total_tokens >= report.input_tokens + report.output_tokens);
    }

    #[test]
    fn failed_json_without_usage_is_missing_and_has_zero_tokens() {
        let report = usage_for_json_response(
            false,
            br#"{"model":"m","input":"must not be estimated"}"#,
            br#"{"error":{"message":"upstream rejected the request"}}"#,
        );
        assert_eq!(report.source, "missing");
        assert_eq!(report, UsageReport::missing());
    }

    #[test]
    fn failed_sse_without_usage_is_missing_and_has_zero_tokens() {
        let report = usage_for_sse_response(
            "test-failed-sse",
            false,
            br#"{"model":"m","stream":true}"#,
            b"event: error\ndata: {\"error\":{\"message\":\"failed\"}}\n\n",
        );
        assert_eq!(report.source, "missing");
        assert_eq!(report, UsageReport::missing());
    }

    #[test]
    fn parsed_sse_usage_keeps_the_query_contract_source() {
        let report = usage_for_sse_response(
            "test-parsed-sse",
            true,
            br#"{"model":"m","stream":true}"#,
            b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":2,\"output_tokens\":3}}}\n\n",
        );
        assert_eq!(report.source, "parsed");
        assert_eq!(report.total_tokens, 5);
    }

    #[tokio::test]
    async fn stream_observer_is_lazy_and_preserves_chunk_order() {
        let polls = Arc::new(AtomicUsize::new(0));
        let source = stream::iter(["first", "second"]).map({
            let polls = polls.clone();
            move |chunk| {
                polls.fetch_add(1, Ordering::SeqCst);
                Ok::<Bytes, std::io::Error>(Bytes::from_static(chunk.as_bytes()))
            }
        });
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = observe_stream_body(
            Body::from_stream(source),
            Instant::now() - Duration::from_millis(20),
            move |observation| {
                tx.send(observation).expect("stream observation receiver");
            },
        );
        assert_eq!(polls.load(Ordering::SeqCst), 0);

        let forwarded = to_bytes(body, 1024).await.expect("forwarded stream");
        let observation = rx.await.expect("stream observation");
        assert_eq!(forwarded, Bytes::from_static(b"firstsecond"));
        assert_eq!(observation.captured, b"firstsecond");
        assert!(observation.ttft_ms.is_some_and(|value| value >= 20));
        assert!(!observation.failed);
        assert_eq!(polls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn empty_stream_completes_without_inventing_ttft() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = observe_stream_body(Body::empty(), Instant::now(), move |observation| {
            tx.send(observation).expect("stream observation receiver");
        });
        assert!(to_bytes(body, 1024).await.expect("empty stream").is_empty());
        let observation = rx.await.expect("empty stream observation");
        assert_eq!(observation.ttft_ms, None);
        assert!(observation.captured.is_empty());
        assert!(observation.failed);
        assert_eq!(observation.termination, StreamTermination::EmptyStream);
    }

    #[tokio::test]
    async fn stream_error_completes_immediately_with_only_observed_chunks() {
        let source = stream::iter([
            Ok::<Bytes, std::io::Error>(Bytes::from_static(b"first")),
            Err(std::io::Error::other("upstream body failed")),
        ]);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = observe_stream_body(Body::from_stream(source), Instant::now(), move |result| {
            tx.send(result).expect("stream observation receiver");
        });
        assert!(to_bytes(body, 1024).await.is_err());
        let observation = rx.await.expect("failed stream observation");
        assert_eq!(observation.captured, b"first");
        assert!(observation.ttft_ms.is_some());
        assert!(observation.failed);
    }

    #[tokio::test]
    async fn stream_error_before_data_keeps_ttft_absent() {
        let source = stream::iter([Err::<Bytes, _>(std::io::Error::other(
            "upstream body failed",
        ))]);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = observe_stream_body(Body::from_stream(source), Instant::now(), move |result| {
            tx.send(result).expect("stream observation receiver");
        });
        assert!(to_bytes(body, 1024).await.is_err());
        let observation = rx.await.expect("failed stream observation");
        assert!(observation.captured.is_empty());
        assert_eq!(observation.ttft_ms, None);
        assert!(observation.failed);
    }

    #[tokio::test]
    async fn gateway_heartbeat_is_excluded_from_capture_and_ttft() {
        let source = stream::iter([
            Ok::<Bytes, std::io::Error>(Bytes::from_static(b": gateway-heartbeat\n\n")),
            Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"data: {\"usage\":{\"input_tokens\":1}}\n\ndata: [DONE]\n\n",
            )),
        ]);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = observe_stream_body(Body::from_stream(source), Instant::now(), move |result| {
            tx.send(result).expect("heartbeat observation");
        });
        let forwarded = to_bytes(body, 1024).await.expect("heartbeat body");
        let observation = rx.await.expect("heartbeat result");
        assert!(String::from_utf8_lossy(&forwarded).contains(": gateway-heartbeat"));
        assert!(!String::from_utf8_lossy(&observation.captured).contains("gateway-heartbeat"));
        assert_eq!(observation.termination, StreamTermination::Completed);
        assert!(observation.ttft_ms.is_some());
    }

    #[tokio::test]
    async fn coalesced_heartbeat_and_provider_event_only_capture_provider_bytes() {
        let source = stream::iter([Ok::<Bytes, std::io::Error>(Bytes::from_static(
            b": gateway-heartbeat\n\ndata: {\"x\":1}\n\n",
        ))]);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = observe_stream_body(Body::from_stream(source), Instant::now(), move |result| {
            tx.send(result).expect("coalesced heartbeat observation");
        });
        let _ = to_bytes(body, 1024)
            .await
            .expect("coalesced heartbeat body");
        let observation = rx.await.expect("coalesced heartbeat result");
        assert_eq!(observation.captured, b"\ndata: {\"x\":1}\n\n");
        assert!(observation.ttft_ms.is_some());
    }

    #[tokio::test]
    async fn gateway_timeout_frame_keeps_specific_termination_reason() {
        let frame = crate::proxy::stream::gateway_error_frame(
            crate::domain::protocol::Protocol::OpenAiResponses,
            StreamTermination::IdleTimeout,
        );
        let source = stream::iter([Ok::<Bytes, std::io::Error>(frame)]);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = observe_stream_body(Body::from_stream(source), Instant::now(), move |result| {
            tx.send(result).expect("timeout observation");
        });
        let _ = to_bytes(body, 1024).await.expect("timeout body");
        let observation = rx.await.expect("timeout result");
        assert_eq!(observation.termination, StreamTermination::IdleTimeout);
        assert!(observation.failed);
        assert_eq!(observation.ttft_ms, None);
    }

    #[tokio::test]
    async fn provider_stream_without_terminal_event_is_an_upstream_error() {
        let source = stream::iter([Ok::<Bytes, std::io::Error>(Bytes::from_static(
            b"data: {\"delta\":\"partial\"}\n\n",
        ))]);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = observe_stream_body(Body::from_stream(source), Instant::now(), move |result| {
            tx.send(result).expect("unterminated stream observation");
        });
        let _ = to_bytes(body, 1024)
            .await
            .expect("unterminated stream body");
        let observation = rx.await.expect("unterminated stream result");
        assert_eq!(observation.termination, StreamTermination::UpstreamError);
        assert!(observation.failed);
    }

    #[test]
    fn adapter_response_failed_keeps_gateway_timeout_reason() {
        let mut tracker = SseEventTracker::default();
        let activity = tracker.feed(
            br#"event: response.failed
data: {"response":{"error":{"code":"gateway_first_event_timeout"}}}

"#,
        );
        assert!(activity.error);
        assert_eq!(
            activity.terminal,
            Some(StreamTermination::FirstEventTimeout)
        );
    }

    #[test]
    fn chat_gateway_error_followed_by_done_keeps_timeout_reason() {
        let frame = crate::proxy::stream::gateway_error_frame(
            crate::domain::protocol::Protocol::OpenAiChatCompletions,
            StreamTermination::TotalTimeout,
        );
        let mut tracker = SseEventTracker::default();
        let activity = tracker.feed(&frame);
        assert!(activity.error);
        assert_eq!(activity.terminal, Some(StreamTermination::TotalTimeout));
    }

    #[tokio::test]
    async fn dropping_an_unconsumed_body_reports_client_cancellation() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = observe_stream_body(
            Body::from_stream(stream::pending::<Result<Bytes, std::io::Error>>()),
            Instant::now(),
            move |observation| {
                tx.send(observation).expect("cancellation observation");
            },
        );
        drop(body);
        let observation = rx.await.expect("cancellation observation result");
        assert_eq!(observation.termination, StreamTermination::ClientCancelled);
        assert!(observation.failed);
        assert_eq!(observation.ttft_ms, None);
    }
}
