//! Fixture definitions and loading for deterministic mock responses.
//!
//! Each fixture describes a complete upstream response: status, headers, body,
//! and optional chunk plan / delay for streaming scenarios.

use std::collections::HashMap;
use std::time::Duration;

use axum::http::StatusCode;

use super::sse::SseChunkPlan;

/// A complete fixture describing one mock upstream response.
#[derive(Debug, Clone)]
pub struct CaseFixture {
    pub case_id: String,
    pub status: StatusCode,
    pub response_headers: HashMap<String, String>,
    pub body: String,
    pub chunk_plan: Option<SseChunkPlan>,
    pub first_byte_delay: Option<Duration>,
}

impl CaseFixture {
    /// Quick JSON response (non-streaming).
    pub fn json(case_id: &str, status: StatusCode, body: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_owned(), "application/json".to_owned());
        Self {
            case_id: case_id.to_owned(),
            status,
            response_headers: headers,
            body: body.to_owned(),
            chunk_plan: None,
            first_byte_delay: None,
        }
    }

    /// SSE streaming response with the given chunk plan.
    pub fn sse(case_id: &str, status: StatusCode, body: &str, plan: SseChunkPlan) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_owned(), "text/event-stream".to_owned());
        Self {
            case_id: case_id.to_owned(),
            status,
            response_headers: headers,
            body: body.to_owned(),
            chunk_plan: Some(plan),
            first_byte_delay: None,
        }
    }

    /// Error response with JSON error envelope.
    pub fn error(case_id: &str, status: StatusCode, error_body: &str) -> Self {
        Self::json(case_id, status, error_body)
    }

    /// Set a first-byte delay (simulates slow upstream).
    pub fn with_first_byte_delay(mut self, delay: Duration) -> Self {
        self.first_byte_delay = Some(delay);
        self
    }

    /// Set custom response headers.
    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.response_headers
            .insert(key.to_owned(), value.to_owned());
        self
    }
}

// ---------------------------------------------------------------------------
// Fixture file loader
// ---------------------------------------------------------------------------

/// Load a fixture body from the `tests/fixtures/` directory.
pub fn load_fixture(relative_path: &str) -> String {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let path = base.join(relative_path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {}: {e}", path.display()))
}

// ===========================================================================
// Built-in fixture catalog — first-batch deterministic cases
// ===========================================================================

pub mod catalog {
    use super::*;
    use crate::support::sse::SseChunkPlan;

    // -----------------------------------------------------------------------
    // OpenAI Chat Completions
    // -----------------------------------------------------------------------

    pub fn chat_text_basic() -> CaseFixture {
        CaseFixture::json(
            "chat.text.basic",
            StatusCode::OK,
            &load_fixture("chat/text_basic.json"),
        )
    }

    pub fn chat_text_basic_stream() -> CaseFixture {
        CaseFixture::sse(
            "chat.stream.basic",
            StatusCode::OK,
            &load_fixture("chat/text_basic_stream.sse"),
            SseChunkPlan::per_event(),
        )
    }

    pub fn chat_usage() -> CaseFixture {
        CaseFixture::json(
            "chat.usage.basic",
            StatusCode::OK,
            &load_fixture("chat/usage.json"),
        )
    }

    pub fn chat_tool_single() -> CaseFixture {
        CaseFixture::json(
            "chat.tool.single",
            StatusCode::OK,
            &load_fixture("chat/tool_single.json"),
        )
    }

    pub fn chat_tool_parallel() -> CaseFixture {
        CaseFixture::json(
            "chat.tool.parallel",
            StatusCode::OK,
            &load_fixture("chat/tool_parallel.json"),
        )
    }

    pub fn chat_reasoning() -> CaseFixture {
        CaseFixture::json(
            "chat.reasoning.basic",
            StatusCode::OK,
            &load_fixture("chat/reasoning.json"),
        )
    }

    pub fn chat_stream_split_lines() -> CaseFixture {
        CaseFixture::sse(
            "chat.stream.split_lines",
            StatusCode::OK,
            &load_fixture("chat/text_basic_stream.sse"),
            SseChunkPlan::split_data_lines(3),
        )
    }

    pub fn chat_stream_merged() -> CaseFixture {
        CaseFixture::sse(
            "chat.stream.merged",
            StatusCode::OK,
            &load_fixture("chat/text_basic_stream.sse"),
            SseChunkPlan::merge_events(2),
        )
    }

    // -----------------------------------------------------------------------
    // OpenAI Responses
    // -----------------------------------------------------------------------

    pub fn responses_text_basic() -> CaseFixture {
        CaseFixture::json(
            "responses.text.basic",
            StatusCode::OK,
            &load_fixture("responses/text_basic.json"),
        )
    }

    pub fn responses_text_basic_stream() -> CaseFixture {
        CaseFixture::sse(
            "responses.stream.basic",
            StatusCode::OK,
            &load_fixture("responses/text_basic_stream.sse"),
            SseChunkPlan::per_event(),
        )
    }

    pub fn responses_tool_single() -> CaseFixture {
        CaseFixture::json(
            "responses.tool.single",
            StatusCode::OK,
            &load_fixture("responses/tool_single.json"),
        )
    }

    pub fn responses_tool_parallel() -> CaseFixture {
        CaseFixture::json(
            "responses.tool.parallel",
            StatusCode::OK,
            &load_fixture("responses/tool_parallel.json"),
        )
    }

    pub fn responses_reasoning() -> CaseFixture {
        CaseFixture::json(
            "responses.reasoning.basic",
            StatusCode::OK,
            &load_fixture("responses/reasoning.json"),
        )
    }

    // -----------------------------------------------------------------------
    // Anthropic Messages
    // -----------------------------------------------------------------------

    pub fn messages_text_basic() -> CaseFixture {
        CaseFixture::json(
            "messages.text.basic",
            StatusCode::OK,
            &load_fixture("messages/text_basic.json"),
        )
    }

    pub fn messages_text_basic_stream() -> CaseFixture {
        CaseFixture::sse(
            "messages.stream.basic",
            StatusCode::OK,
            &load_fixture("messages/text_basic_stream.sse"),
            SseChunkPlan::per_event(),
        )
    }

    pub fn messages_tool_single() -> CaseFixture {
        CaseFixture::json(
            "messages.tool.single",
            StatusCode::OK,
            &load_fixture("messages/tool_single.json"),
        )
    }

    pub fn messages_tool_parallel() -> CaseFixture {
        CaseFixture::json(
            "messages.tool.parallel",
            StatusCode::OK,
            &load_fixture("messages/tool_parallel.json"),
        )
    }

    pub fn messages_thinking() -> CaseFixture {
        CaseFixture::json(
            "messages.thinking.basic",
            StatusCode::OK,
            &load_fixture("messages/thinking.json"),
        )
    }

    // -----------------------------------------------------------------------
    // Error / Fault fixtures
    // -----------------------------------------------------------------------

    pub fn error_400() -> CaseFixture {
        CaseFixture::error(
            "common.error.400",
            StatusCode::BAD_REQUEST,
            &load_fixture("errors/400.json"),
        )
    }

    pub fn error_401() -> CaseFixture {
        CaseFixture::error(
            "common.error.401",
            StatusCode::UNAUTHORIZED,
            &load_fixture("errors/401.json"),
        )
    }

    pub fn error_403() -> CaseFixture {
        CaseFixture::error(
            "common.error.403",
            StatusCode::FORBIDDEN,
            &load_fixture("errors/403.json"),
        )
    }

    pub fn error_404() -> CaseFixture {
        CaseFixture::error(
            "common.error.404",
            StatusCode::NOT_FOUND,
            &load_fixture("errors/404.json"),
        )
    }

    pub fn error_429() -> CaseFixture {
        CaseFixture::error(
            "common.error.429",
            StatusCode::TOO_MANY_REQUESTS,
            &load_fixture("errors/429.json"),
        )
        .with_header("retry-after", "1")
    }

    pub fn error_500() -> CaseFixture {
        CaseFixture::error(
            "common.error.500",
            StatusCode::INTERNAL_SERVER_ERROR,
            &load_fixture("errors/500.json"),
        )
    }

    pub fn error_502() -> CaseFixture {
        CaseFixture::error(
            "common.error.502",
            StatusCode::BAD_GATEWAY,
            &load_fixture("errors/502.json"),
        )
    }

    pub fn error_503() -> CaseFixture {
        CaseFixture::error(
            "common.error.503",
            StatusCode::SERVICE_UNAVAILABLE,
            &load_fixture("errors/503.json"),
        )
    }

    pub fn error_invalid_json() -> CaseFixture {
        CaseFixture {
            case_id: "common.error.invalid_json".to_owned(),
            status: StatusCode::OK,
            response_headers: {
                let mut h = HashMap::new();
                h.insert("content-type".to_owned(), "application/json".to_owned());
                h
            },
            body: load_fixture("errors/invalid_json.txt"),
            chunk_plan: None,
            first_byte_delay: None,
        }
    }

    pub fn error_invalid_sse() -> CaseFixture {
        CaseFixture::sse(
            "common.error.invalid_sse",
            StatusCode::OK,
            &load_fixture("errors/invalid_sse.txt"),
            SseChunkPlan::single_chunk(),
        )
    }

    pub fn error_empty_body() -> CaseFixture {
        CaseFixture {
            case_id: "common.error.empty_body".to_owned(),
            status: StatusCode::OK,
            response_headers: {
                let mut h = HashMap::new();
                h.insert("content-type".to_owned(), "application/json".to_owned());
                h
            },
            body: String::new(),
            chunk_plan: None,
            first_byte_delay: None,
        }
    }

    pub fn error_incomplete_stream() -> CaseFixture {
        CaseFixture::sse(
            "common.error.incomplete_stream",
            StatusCode::OK,
            &load_fixture("errors/incomplete_stream.sse"),
            SseChunkPlan::per_event(),
        )
    }

    pub fn error_timeout() -> CaseFixture {
        CaseFixture::json(
            "common.error.timeout",
            StatusCode::OK,
            r#"{"id":"timeout-test"}"#,
        )
        .with_first_byte_delay(Duration::from_secs(30))
    }

    // -----------------------------------------------------------------------
    // Convenience: build all first-batch cases into a HashMap
    // -----------------------------------------------------------------------

    /// All first-batch fixtures keyed by case ID, ready for `MockProvider::spawn()`.
    pub fn all_first_batch() -> HashMap<String, CaseFixture> {
        let fixtures = vec![
            chat_text_basic(),
            chat_text_basic_stream(),
            chat_usage(),
            chat_tool_single(),
            chat_tool_parallel(),
            chat_reasoning(),
            chat_stream_split_lines(),
            chat_stream_merged(),
            responses_text_basic(),
            responses_text_basic_stream(),
            responses_tool_single(),
            responses_tool_parallel(),
            responses_reasoning(),
            messages_text_basic(),
            messages_text_basic_stream(),
            messages_tool_single(),
            messages_tool_parallel(),
            messages_thinking(),
            error_400(),
            error_401(),
            error_403(),
            error_404(),
            error_429(),
            error_500(),
            error_502(),
            error_503(),
            error_invalid_json(),
            error_invalid_sse(),
            error_empty_body(),
            error_incomplete_stream(),
            error_timeout(),
        ];
        fixtures
            .into_iter()
            .map(|f| (f.case_id.clone(), f))
            .collect()
    }
}
