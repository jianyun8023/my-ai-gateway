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
    let cached = usage
        .get("input_tokens_details")
        .map(|v| i64_at(v, "cached_tokens"))
        .or_else(|| {
            usage
                .get("prompt_tokens_details")
                .map(|v| i64_at(v, "cached_tokens"))
        })
        .unwrap_or_else(|| {
            i64_at(usage, "cached_tokens")
                + i64_at(usage, "cache_read_input_tokens")
                + i64_at(usage, "cache_creation_input_tokens")
        });
    let total = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(input + output);
    let report = UsageReport {
        input_tokens: input,
        output_tokens: output,
        reasoning_tokens: reasoning,
        cached_tokens: cached,
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
pub fn usage_for_sse_response(success: bool, request: &[u8], captured: &[u8]) -> UsageReport {
    extract_sse(&String::from_utf8_lossy(captured)).unwrap_or_else(|| {
        if success {
            estimate(request, captured)
        } else {
            UsageReport::missing()
        }
    })
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
