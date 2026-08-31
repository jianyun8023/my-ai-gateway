//! Token usage extraction shared by native passthrough and protocol adapters.
//!
//! Providers use slightly different field names.  This module deliberately
//! accepts the OpenAI Chat/Responses and Anthropic Messages shapes without
//! attempting to interpret response content.

use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageReport {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub total_tokens: i64,
    pub source: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
