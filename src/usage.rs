//! Token usage extraction shared by native passthrough and protocol adapters.
//!
//! Providers use slightly different field names.  This module deliberately
//! accepts the OpenAI Chat/Responses and Anthropic Messages shapes without
//! attempting to interpret response content.

use axum::body::{Body, Bytes};
use futures_util::{stream, StreamExt};
use serde_json::Value;
use std::time::Instant;

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
}

/// Tee a live response body for usage parsing while forwarding every chunk in
/// the same order. The upstream is polled once per downstream demand, so this
/// does not prefetch the response or alter backpressure. Completion is reported
/// on clean EOF or immediately on a body-stream error.
pub fn observe_stream_body<F>(body: Body, request_started: Instant, on_complete: F) -> Body
where
    F: FnOnce(StreamObservation) + Send + 'static,
{
    type Completion = Box<dyn FnOnce(StreamObservation) + Send>;

    let upstream = body.into_data_stream();
    let completion: Option<Completion> = Some(Box::new(on_complete));
    let stream = stream::unfold(
        (upstream, Vec::new(), None, completion, request_started),
        |(mut upstream, mut captured, mut ttft_ms, mut completion, request_started)| async move {
            completion.as_ref()?;
            match upstream.next().await {
                Some(Ok(chunk)) => {
                    if ttft_ms.is_none() && !chunk.is_empty() {
                        ttft_ms = Some(request_started.elapsed().as_millis() as i64);
                    }
                    captured.extend_from_slice(&chunk);
                    Some((
                        Ok::<Bytes, std::io::Error>(chunk),
                        (upstream, captured, ttft_ms, completion, request_started),
                    ))
                }
                Some(Err(error)) => {
                    if let Some(complete) = completion.take() {
                        complete(StreamObservation {
                            captured: std::mem::take(&mut captured),
                            ttft_ms,
                            failed: true,
                        });
                    }
                    Some((
                        Err(std::io::Error::other(error.to_string())),
                        (upstream, captured, ttft_ms, completion, request_started),
                    ))
                }
                None => {
                    if let Some(complete) = completion.take() {
                        complete(StreamObservation {
                            captured,
                            ttft_ms,
                            failed: false,
                        });
                    }
                    None
                }
            }
        },
    );
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
        assert!(!observation.failed);
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
}
