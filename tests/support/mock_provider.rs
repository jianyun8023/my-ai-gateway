//! Deterministic Mock Provider for AI Gateway contract testing.
//!
//! Routes requests to fixture responses via the `X-Test-Case` header.
//! Records sanitized request metadata for upstream-conversion assertions.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use bytes::Bytes;
use http_body_util::BodyExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use super::fixtures::CaseFixture;

/// Header used by tests to select a fixture case.
pub const TEST_CASE_HEADER: &str = "x-test-case";

// ---------------------------------------------------------------------------
// RecordedRequest – sanitized metadata captured per upstream call
// ---------------------------------------------------------------------------

/// A single request recorded by the mock provider.
///
/// **Security**: `authorization_present` replaces any raw credential value.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub content_type: Option<String>,
    pub headers: SanitizedHeaders,
    pub body: serde_json::Value,
    pub arrival_order: usize,
}

/// Request headers with sensitive values redacted.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct SanitizedHeaders {
    pub authorization_present: bool,
    pub x_api_key_present: bool,
    pub content_type: Option<String>,
    pub test_case: Option<String>,
    /// Non-sensitive headers preserved for assertions.
    pub extra: HashMap<String, String>,
}

impl SanitizedHeaders {
    fn from_header_map(headers: &HeaderMap) -> Self {
        let mut extra = HashMap::new();
        for (name, value) in headers.iter() {
            let key = name.as_str().to_ascii_lowercase();
            match key.as_str() {
                "authorization" | "x-api-key" | "host" | "content-type" | "content-length"
                | "transfer-encoding" | "connection" => {}
                _ => {
                    if let Ok(v) = value.to_str() {
                        extra.insert(key, v.to_owned());
                    }
                }
            }
        }
        Self {
            authorization_present: headers.contains_key(header::AUTHORIZATION),
            x_api_key_present: headers.contains_key("x-api-key"),
            content_type: headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned()),
            test_case: headers
                .get(TEST_CASE_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned()),
            extra,
        }
    }
}

// ---------------------------------------------------------------------------
// MockProvider
// ---------------------------------------------------------------------------

/// A deterministic mock upstream provider for integration tests.
///
/// # Usage
/// ```ignore
/// let mock = MockProvider::builder()
///     .case("chat.text.basic", fixture)
///     .build()
///     .spawn()
///     .await;
///
/// // Point gateway at mock.base_url()
/// // After test: mock.requests() / mock.take_requests()
/// ```
pub struct MockProvider {
    base_url: String,
    _addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    _server_handle: JoinHandle<()>,
}

impl MockProvider {
    /// Create a builder for configuring case fixtures.
    pub fn builder() -> MockProviderBuilder {
        MockProviderBuilder {
            cases: HashMap::new(),
            default_response: None,
        }
    }

    /// Convenience: spawn with a single set of cases.
    pub async fn spawn(cases: HashMap<String, CaseFixture>) -> Self {
        let mut builder = Self::builder();
        for (id, fixture) in cases {
            builder = builder.case(&id, fixture);
        }
        builder.build().spawn().await
    }

    /// The base URL (http://127.0.0.1:{port}) for upstream configuration.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Snapshot of all recorded requests so far.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("request recorder lock").clone()
    }

    /// Take all recorded requests, clearing the buffer.
    pub fn take_requests(&self) -> Vec<RecordedRequest> {
        let mut guard = self.requests.lock().expect("request recorder lock");
        std::mem::take(&mut *guard)
    }

    /// Number of requests recorded so far.
    pub fn request_count(&self) -> usize {
        self.requests.lock().expect("request recorder lock").len()
    }
}

impl Drop for MockProvider {
    fn drop(&mut self) {
        self._server_handle.abort();
    }
}

// ---------------------------------------------------------------------------
// MockProviderBuilder
// ---------------------------------------------------------------------------

pub struct MockProviderBuilder {
    cases: HashMap<String, CaseFixture>,
    default_response: Option<CaseFixture>,
}

impl MockProviderBuilder {
    /// Register a fixture for a given case ID (e.g. `chat.text.basic`).
    pub fn case(mut self, case_id: &str, fixture: CaseFixture) -> Self {
        self.cases.insert(case_id.to_owned(), fixture);
        self
    }

    /// Set a default response when no `X-Test-Case` header is present or
    /// the case ID is not registered.
    pub fn default_response(mut self, fixture: CaseFixture) -> Self {
        self.default_response = Some(fixture);
        self
    }

    pub fn build(self) -> SpawnableMock {
        SpawnableMock {
            cases: Arc::new(self.cases),
            default_response: self.default_response.map(Arc::new),
        }
    }
}

/// Intermediate type that can be `.spawn().await`-ed.
pub struct SpawnableMock {
    cases: Arc<HashMap<String, CaseFixture>>,
    default_response: Option<Arc<CaseFixture>>,
}

impl SpawnableMock {
    pub async fn spawn(self) -> MockProvider {
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let cases = self.cases;
        let default_response = self.default_response;

        let app = Router::new().route(
            "/{*path}",
            any({
                let requests = requests.clone();
                let cases = cases.clone();
                let default_response = default_response.clone();
                move |request: Request| {
                    let requests = requests.clone();
                    let cases = cases.clone();
                    let default_response = default_response.clone();
                    async move {
                        handle_request(request, &requests, &cases, default_response.as_deref())
                            .await
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock provider");
        let addr = listener.local_addr().expect("mock provider address");

        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock provider server");
        });

        MockProvider {
            base_url: format!("http://127.0.0.1:{}", addr.port()),
            _addr: addr,
            requests,
            _server_handle: server_handle,
        }
    }
}

// ---------------------------------------------------------------------------
// Request handler – case dispatch + recording
// ---------------------------------------------------------------------------

async fn handle_request(
    request: Request,
    requests: &Arc<Mutex<Vec<RecordedRequest>>>,
    cases: &HashMap<String, CaseFixture>,
    default_response: Option<&CaseFixture>,
) -> Response<Body> {
    let (parts, body) = request.into_parts();

    let body_bytes = body
        .collect()
        .await
        .map(|c| c.to_bytes())
        .unwrap_or_default();

    let json_body: serde_json::Value = serde_json::from_slice(&body_bytes)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&body_bytes).into()));

    let arrival_order = {
        let mut guard = requests.lock().expect("request recorder lock");
        let order = guard.len();
        guard.push(RecordedRequest {
            method: parts.method.to_string(),
            path: parts.uri.path().to_owned(),
            content_type: parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned()),
            headers: SanitizedHeaders::from_header_map(&parts.headers),
            body: json_body,
            arrival_order: order,
        });
        order
    };
    let _ = arrival_order;

    let case_id = parts
        .headers
        .get(TEST_CASE_HEADER)
        .and_then(|v| v.to_str().ok());

    let fixture = case_id.and_then(|id| cases.get(id)).or(default_response);

    match fixture {
        Some(f) => build_response(f).await,
        None => {
            let msg = match case_id {
                Some(id) => format!("unknown test case: {id}"),
                None => "missing X-Test-Case header".to_string(),
            };
            Response::builder()
                .status(StatusCode::NOT_IMPLEMENTED)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({"error": {"message": msg}}).to_string(),
                ))
                .expect("error response")
        }
    }
}

async fn build_response(fixture: &CaseFixture) -> Response<Body> {
    if let Some(delay) = fixture.first_byte_delay {
        tokio::time::sleep(delay).await;
    }

    let mut builder = Response::builder().status(fixture.status);
    for (k, v) in &fixture.response_headers {
        builder = builder.header(k.as_str(), v.as_str());
    }

    match &fixture.chunk_plan {
        Some(plan) => {
            let content_type = fixture
                .response_headers
                .get("content-type")
                .cloned()
                .unwrap_or_else(|| "text/event-stream".to_owned());
            builder = builder.header(header::CONTENT_TYPE, content_type);

            let chunks = plan.generate_chunks(&fixture.body);
            let stream = build_chunk_stream(chunks, plan.idle_delay);
            builder
                .body(Body::from_stream(stream))
                .expect("sse response")
        }
        None => {
            if !fixture.response_headers.contains_key("content-type") {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
            }
            builder
                .body(Body::from(fixture.body.clone()))
                .expect("json response")
        }
    }
}

fn build_chunk_stream(
    chunks: Vec<Bytes>,
    idle_delay: Option<std::time::Duration>,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> {
    let delay = idle_delay.unwrap_or(std::time::Duration::from_millis(5));
    futures_util::stream::iter(chunks.into_iter().enumerate().map(move |(i, chunk)| {
        let delay = if i == 0 {
            std::time::Duration::ZERO
        } else {
            delay
        };
        (chunk, delay)
    }))
    .then(|(chunk, delay)| async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        Ok(chunk)
    })
}

use futures_util::StreamExt;
