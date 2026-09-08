use super::{
    stream::{self, StreamConfig, StreamTermination},
    usage::{usage_for_json_response, UsageReport},
};
use crate::{
    domain::{
        config::{AccountConfig, ProviderConfig},
        protocol::Protocol,
    },
    http::SourceHttpClient,
    source_url::{reqwest_error_is_policy_violation, SourceUrlPolicyError},
};
use axum::{
    body::Body,
    http::{HeaderMap, Response, StatusCode},
};
use bytes::Bytes;
use futures_util::TryStreamExt;
use serde_json::Value;

#[derive(Debug)]
pub(crate) enum TransportError {
    MissingEndpoint,
    SourceUrlBlocked,
    Request,
    Timeout(StreamTermination),
}

impl TransportError {
    pub(crate) fn message(&self) -> &str {
        match self {
            Self::MissingEndpoint => "provider endpoint is not configured",
            Self::SourceUrlBlocked => "upstream source URL is blocked by server policy",
            Self::Request => "upstream request failed",
            Self::Timeout(termination) => termination.message(),
        }
    }

    pub(crate) fn status_code(&self) -> i32 {
        match self {
            Self::Timeout(termination) => termination.status_code(),
            Self::MissingEndpoint | Self::SourceUrlBlocked | Self::Request => 599,
        }
    }
}

impl From<SourceUrlPolicyError> for TransportError {
    fn from(_: SourceUrlPolicyError) -> Self {
        Self::SourceUrlBlocked
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedModelRequest {
    pub(crate) body: Bytes,
    pub(crate) upstream_model_id: String,
}

/// Prepare the JSON body for one concrete upstream attempt.
///
/// The resolved Binding or account-level mapping only replaces the top-level
/// `model` field. Unmapped requests retain their original bytes exactly, and a
/// malformed or non-object body is left untouched so the recorded model always
/// matches what was actually sent.
pub(crate) fn prepare_model_request(
    body: &Bytes,
    requested_model: &str,
    upstream_model_id: &str,
) -> PreparedModelRequest {
    if upstream_model_id == requested_model {
        return PreparedModelRequest {
            body: body.clone(),
            upstream_model_id: requested_model.to_owned(),
        };
    }
    let Ok(mut payload) = serde_json::from_slice::<Value>(body) else {
        return PreparedModelRequest {
            body: body.clone(),
            upstream_model_id: requested_model.to_owned(),
        };
    };
    let Some(object) = payload.as_object_mut() else {
        return PreparedModelRequest {
            body: body.clone(),
            upstream_model_id: requested_model.to_owned(),
        };
    };
    object.insert("model".into(), Value::String(upstream_model_id.to_owned()));
    let Ok(serialized_body) = serde_json::to_vec(&payload) else {
        return PreparedModelRequest {
            body: body.clone(),
            upstream_model_id: requested_model.to_owned(),
        };
    };
    PreparedModelRequest {
        body: Bytes::from(serialized_body),
        upstream_model_id: upstream_model_id.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Thinking-parameter sanitization
// ---------------------------------------------------------------------------
//
// MiniMax M3 / DeepSeek reasoning mode requires provider-specific reasoning
// fields to be replayed on every prior assistant turn.  Most OpenAI-compatible
// clients (Kimi Code, Cursor, Aider, …) rebuild message or input history
// through a converter that only keeps standard fields and silently drops the
// reasoning content.  The next request then fails with HTTP 400.
//
// Rather than maintaining a stateful reasoning cache, the gateway detects the
// mismatch and strips thinking parameters so the upstream processes the
// request in normal (non-thinking) mode.  This trades reasoning depth for a
// working conversation.

/// Known top-level Chat Completions parameters that enable thinking.
const CHAT_THINKING_PARAMS: &[&str] = &["thinking", "reasoning_effort", "reasoning_split"];

/// Known assistant-message fields that carry reasoning content (Chat Completions).
const CHAT_REASONING_FIELDS: &[&str] =
    &["reasoning_content", "reasoning_details", "reasoning_text"];

/// Unified entry point: strip thinking / reasoning parameters from a request
/// body when the conversation history is missing the required reasoning fields.
/// Works for both Chat Completions and OpenAI Responses protocol shapes.
///
/// Returns `true` if any parameters were removed (caller should log).
pub(crate) fn sanitize_thinking_params(body: &mut Bytes, protocol: Protocol) -> bool {
    match protocol {
        Protocol::OpenAiChatCompletions => sanitize_chat_thinking(body),
        Protocol::OpenAiResponses => sanitize_responses_reasoning(body),
        _ => false,
    }
}

/// Returns `true` when the `thinking` parameter explicitly disables reasoning
/// (`{"type": "disabled"}`).  In this mode assistant responses naturally omit
/// reasoning fields, so the sanitizer must not strip the parameter.
fn thinking_is_disabled(object: &serde_json::Map<String, Value>) -> bool {
    object
        .get("thinking")
        .and_then(Value::as_object)
        .and_then(|t| t.get("type"))
        .and_then(Value::as_str)
        == Some("disabled")
}

/// Chat Completions: strip `thinking`, `reasoning_effort`, `reasoning_split`
/// when assistant messages lack reasoning fields.
fn sanitize_chat_thinking(body: &mut Bytes) -> bool {
    let Ok(mut payload) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let Some(object) = payload.as_object_mut() else {
        return false;
    };

    let has_thinking = CHAT_THINKING_PARAMS
        .iter()
        .any(|key| object.contains_key(*key));
    if !has_thinking {
        return false;
    }

    // When `thinking.type` is `"disabled"`, the client explicitly opted out
    // of reasoning.  Assistant messages from such turns naturally lack
    // reasoning fields, so stripping `thinking: {type: "disabled"}` would
    // cause providers that default to thinking mode (e.g. DeepSeek v4) to
    // re-enable reasoning and reject the request when `tool_choice` is set.
    if thinking_is_disabled(object) {
        return false;
    }

    let messages = match object.get("messages").and_then(Value::as_array) {
        Some(m) => m,
        None => return false,
    };

    let mut found_assistant_without_reasoning = false;
    for msg in messages {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let has_reasoning_field = CHAT_REASONING_FIELDS
            .iter()
            .any(|field| msg.get(*field).is_some_and(|v| !v.is_null()));
        let has_reasoning_in_content = msg
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|c| c.contains("<reasoning_content>"));
        if !has_reasoning_field && !has_reasoning_in_content {
            found_assistant_without_reasoning = true;
            break;
        }
    }

    if !found_assistant_without_reasoning {
        return false;
    }

    let mut removed = false;
    for key in CHAT_THINKING_PARAMS {
        if object.remove(*key).is_some() {
            removed = true;
        }
    }
    if removed {
        if let Ok(serialized) = serde_json::to_vec(&payload) {
            *body = Bytes::from(serialized);
        }
    }
    removed
}

/// OpenAI Responses: strip the top-level `reasoning` object when the `input`
/// array contains assistant output items but no accompanying `type: "reasoning"`
/// items.
///
/// In the Responses protocol, reasoning output appears as separate items with
/// `"type": "reasoning"` in the `input` / `output` arrays.  When the client
/// drops these items but keeps `"reasoning": {"effort": "..."}` in the
/// request, MiniMax returns HTTP 400 asking for `reasoning_text` to be passed
/// back.
fn sanitize_responses_reasoning(body: &mut Bytes) -> bool {
    let Ok(mut payload) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let Some(object) = payload.as_object_mut() else {
        return false;
    };

    // Fast path: no reasoning parameter → nothing to do.
    if !object.contains_key("reasoning") {
        return false;
    }

    let input = match object.get("input") {
        // `input` can be a plain string (single-turn shorthand) → no history
        Some(Value::Array(arr)) => arr,
        _ => return false,
    };

    // Look for assistant output items (messages with role=assistant, or
    // items whose type is a known assistant output type).
    let has_assistant_output = input.iter().any(|item| {
        let role = item.get("role").and_then(Value::as_str);
        let item_type = item.get("type").and_then(Value::as_str);
        role == Some("assistant")
            || matches!(
                item_type,
                Some(
                    "output_text"
                        | "function_call"
                        | "function_call_output"
                        | "web_search_call"
                        | "file_search_call"
                        | "computer_call"
                )
            )
    });

    if !has_assistant_output {
        return false;
    }

    // Check if there is at least one reasoning item.
    let has_reasoning_items = input
        .iter()
        .any(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"));

    if has_reasoning_items {
        return false;
    }

    // Strip reasoning parameters.
    let removed = object.remove("reasoning").is_some();
    if removed {
        if let Ok(serialized) = serde_json::to_vec(&payload) {
            *body = Bytes::from(serialized);
        }
    }
    removed
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn forward_with_config(
    client: &SourceHttpClient,
    provider: &ProviderConfig,
    account: &AccountConfig,
    credential: Option<&str>,
    protocol: Protocol,
    request_headers: &HeaderMap,
    body: Bytes,
    stream_config: &StreamConfig,
    request_started: std::time::Instant,
) -> Result<Response<Body>, TransportError> {
    let endpoint = provider
        .endpoints
        .get(&protocol)
        .ok_or(TransportError::MissingEndpoint)?;
    let url = format!("{}{}", provider.base_url.trim_end_matches('/'), endpoint);
    forward_url_with_config(
        client,
        &url,
        account,
        credential,
        protocol,
        request_headers,
        body,
        stream_config,
        request_started,
    )
    .await
}

#[cfg(test)]
pub(crate) async fn forward_url(
    client: &SourceHttpClient,
    url: &str,
    account: &AccountConfig,
    credential: Option<&str>,
    protocol: Protocol,
    request_headers: &HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, TransportError> {
    let config = StreamConfig::from_env();
    let request_started = std::time::Instant::now();
    forward_url_with_config(
        client,
        url,
        account,
        credential,
        protocol,
        request_headers,
        body,
        &config,
        request_started,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn forward_url_with_config(
    client: &SourceHttpClient,
    url: &str,
    account: &AccountConfig,
    credential: Option<&str>,
    protocol: Protocol,
    request_headers: &HeaderMap,
    body: Bytes,
    stream_config: &StreamConfig,
    request_started: std::time::Instant,
) -> Result<Response<Body>, TransportError> {
    let request_payload = body.clone();
    let is_streaming = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("stream").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    let mut request = client.post(url).map_err(TransportError::from)?.body(body);
    for (name, value) in request_headers {
        if !matches!(
            name.as_str(),
            "host" | "content-length" | "authorization" | "x-api-key"
        ) {
            request = request.header(name, value);
        }
    }
    if let Some(credential) = credential.or(account.credential.as_deref()) {
        if protocol == Protocol::AnthropicMessages {
            request = request.header("x-api-key", credential);
        } else {
            request = request.header("authorization", format!("Bearer {credential}"));
        }
    }
    let send_timeout = earliest_send_timeout(request_started, stream_config);
    let upstream = match send_timeout {
        None => request.send().await.map_err(map_reqwest_error)?,
        Some((duration, termination)) if duration.is_zero() => {
            return Err(TransportError::Timeout(termination));
        }
        Some((duration, termination)) => match tokio::time::timeout(duration, request.send()).await
        {
            Ok(result) => result.map_err(map_reqwest_error)?,
            Err(_) => return Err(TransportError::Timeout(termination)),
        },
    };
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let upstream_headers = upstream.headers().clone();
    // Buffer non-streaming responses so the usage object is available to the
    // request lifecycle while preserving a normal response body. Streaming
    // responses stay a live byte stream to retain TTFT and backpressure.
    if !is_streaming {
        let bytes = if let Some(deadline) = total_deadline(request_started, stream_config) {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match tokio::time::timeout(remaining, upstream.bytes()).await {
                Ok(result) => result.map_err(map_reqwest_error)?,
                Err(_) => {
                    return Err(TransportError::Timeout(StreamTermination::TotalTimeout));
                }
            }
        } else {
            upstream.bytes().await.map_err(map_reqwest_error)?
        };
        let report = usage_for_json_response(status.is_success(), &request_payload, &bytes);
        let body = Body::from(bytes);
        let mut response = Response::new(body);
        *response.status_mut() = status;
        for (name, value) in &upstream_headers {
            if !matches!(
                name.as_str(),
                "connection" | "keep-alive" | "transfer-encoding" | "content-length"
            ) {
                response.headers_mut().insert(name, value.clone());
            }
        }
        response.extensions_mut().insert(report);
        return Ok(response);
    }
    let stream = upstream.bytes_stream();
    let body = Body::from_stream(stream.map_err(|error| std::io::Error::other(error.to_string())));
    let body = stream::wrap_native_body(body, protocol, stream_config.clone(), request_started);
    let mut response = Response::new(body);
    *response.status_mut() = status;
    for (name, value) in &upstream_headers {
        if !matches!(
            name.as_str(),
            "connection" | "keep-alive" | "transfer-encoding" | "content-length"
        ) {
            response.headers_mut().insert(name, value.clone());
        }
    }
    Ok(response)
}

fn total_deadline(
    request_started: std::time::Instant,
    config: &StreamConfig,
) -> Option<std::time::Instant> {
    (!config.total_timeout.is_zero())
        .then(|| request_started.checked_add(config.total_timeout))
        .flatten()
}

fn earliest_send_timeout(
    request_started: std::time::Instant,
    config: &StreamConfig,
) -> Option<(std::time::Duration, StreamTermination)> {
    let now = std::time::Instant::now();
    let total = (!config.total_timeout.is_zero())
        .then(|| {
            request_started
                .checked_add(config.total_timeout)
                .map(|deadline| (deadline, StreamTermination::TotalTimeout))
        })
        .flatten();
    let connection = (!config.connection_timeout.is_zero())
        .then(|| {
            now.checked_add(config.connection_timeout)
                .map(|deadline| (deadline, StreamTermination::ConnectionTimeout))
        })
        .flatten();
    match (total, connection) {
        (None, None) => None,
        (Some((deadline, termination)), None) | (None, Some((deadline, termination))) => {
            Some((deadline.saturating_duration_since(now), termination))
        }
        (Some((total_deadline, total_term)), Some((connection_deadline, connection_term))) => {
            if total_deadline <= connection_deadline {
                Some((total_deadline.saturating_duration_since(now), total_term))
            } else {
                Some((
                    connection_deadline.saturating_duration_since(now),
                    connection_term,
                ))
            }
        }
    }
}

/// Retrieve usage metadata attached by [`forward_url`].
pub(crate) fn usage_from_response(response: &Response<Body>) -> Option<UsageReport> {
    response.extensions().get::<UsageReport>().cloned()
}

fn map_reqwest_error(error: reqwest::Error) -> TransportError {
    if reqwest_error_is_policy_violation(&error) {
        TransportError::SourceUrlBlocked
    } else if error.is_timeout() {
        TransportError::Timeout(StreamTermination::ConnectionTimeout)
    } else {
        let _ = error;
        TransportError::Request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_client;
    use axum::{
        body::to_bytes,
        extract::Request,
        http::{header, HeaderValue},
        routing::get,
        Router,
    };
    use futures_util::stream;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        headers: HeaderMap,
        body: Vec<u8>,
    }

    async fn spawn_mock_upstream() -> (String, Arc<Mutex<Vec<RecordedRequest>>>) {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new().fallback({
            let recorded = recorded.clone();
            move |request: Request| {
                let recorded = recorded.clone();
                async move {
                    let (parts, body) = request.into_parts();
                    let body = to_bytes(body, 1024 * 1024).await.unwrap();
                    recorded.lock().unwrap().push(RecordedRequest {
                        headers: parts.headers,
                        body: body.to_vec(),
                    });
                    Response::builder()
                        .status(StatusCode::IM_A_TEAPOT)
                        .header(header::CONTENT_TYPE, "application/json")
                        .header("x-upstream-response", "preserved")
                        .body(Body::from(
                            r#"{"error":{"message":"upstream rejected the request"}}"#,
                        ))
                        .unwrap()
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind native transport mock");
        let address = listener.local_addr().expect("native transport address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve native transport mock")
        });
        (format!("http://{address}/native"), recorded)
    }

    async fn spawn_router(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind redirect mock");
        let address = listener.local_addr().expect("redirect mock address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve redirect mock")
        });
        format!("http://{address}")
    }

    fn account() -> AccountConfig {
        AccountConfig {
            id: "account".into(),
            provider_id: "provider".into(),
            display_name: "Native account".into(),
            credential_env: None,
            credential_ciphertext: None,
            credential: None,
            enabled: true,
            weight: 100,
            protocol_capabilities: HashMap::new(),
            capabilities: None,
            model_overrides: HashMap::new(),
            model_map: HashMap::new(),
        }
    }

    #[test]
    fn resolved_model_only_rewrites_the_top_level_model_for_all_protocol_shapes() {
        let payloads = [
            serde_json::json!({
                "model": "logical-model",
                "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}],
                "tools": [{"type": "function", "function": {"name": "lookup"}}],
                "stream": true,
                "provider_extension": {"keep": [1, 2, 3]}
            }),
            serde_json::json!({
                "model": "logical-model",
                "input": [{"role": "user", "content": [{"type": "input_text", "text": "hello"}]}],
                "reasoning": {"effort": "high"},
                "tools": [{"type": "web_search_preview"}],
                "stream": true
            }),
            serde_json::json!({
                "model": "logical-model",
                "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}],
                "max_tokens": 128,
                "thinking": {"type": "enabled", "budget_tokens": 32},
                "stream": true
            }),
        ];

        for original in payloads {
            let body = Bytes::from(serde_json::to_vec(&original).unwrap());
            let prepared = prepare_model_request(&body, "logical-model", "provider-model");
            assert_eq!(prepared.upstream_model_id, "provider-model");
            let mut expected = original;
            expected["model"] = Value::String("provider-model".into());
            assert_eq!(
                serde_json::from_slice::<Value>(&prepared.body).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn unchanged_model_preserves_exact_body_bytes() {
        let body = Bytes::from_static(br#"{ "model": "logical-model", "input": "hello" }"#);
        let prepared = prepare_model_request(&body, "logical-model", "logical-model");
        assert_eq!(prepared.upstream_model_id, "logical-model");
        assert_eq!(prepared.body, body);
    }

    #[test]
    fn default_client_rejects_an_initial_loopback_url_without_network_access() {
        let client =
            crate::http::client(Arc::new(crate::source_url::SourceUrlPolicy::default())).unwrap();
        let error = client
            .get("http://127.0.0.1:8787/private")
            .expect_err("loopback URL must be rejected before request construction");
        assert_eq!(error, SourceUrlPolicyError::DisallowedTarget);
        assert!(!error.to_string().contains("127.0.0.1"));
    }

    #[tokio::test]
    async fn redirects_are_limited_to_the_validated_origin() {
        let same_origin = Router::new()
            .route(
                "/start",
                get(|| async {
                    Response::builder()
                        .status(StatusCode::TEMPORARY_REDIRECT)
                        .header(header::LOCATION, "/final")
                        .body(Body::empty())
                        .unwrap()
                }),
            )
            .route(
                "/final",
                get(|| async { Response::new(Body::from("same-origin")) }),
            );
        let same_origin = spawn_router(same_origin).await;
        let client = test_client().unwrap();
        let response = client
            .get(&format!("{same_origin}/start"))
            .unwrap()
            .send()
            .await
            .expect("same-origin redirect");
        assert_eq!(response.text().await.unwrap(), "same-origin");

        let other_origin = spawn_router(Router::new().route(
            "/final",
            get(|| async { Response::new(Body::from("must-not-follow")) }),
        ))
        .await;
        let location = format!("{other_origin}/final");
        let cross_origin = spawn_router(Router::new().route(
            "/start",
            get(move || {
                let location = location.clone();
                async move {
                    Response::builder()
                        .status(StatusCode::TEMPORARY_REDIRECT)
                        .header(header::LOCATION, location)
                        .body(Body::empty())
                        .unwrap()
                }
            }),
        ))
        .await;
        let error = client
            .get(&format!("{cross_origin}/start"))
            .unwrap()
            .send()
            .await
            .expect_err("cross-origin redirect must be rejected");
        assert!(reqwest_error_is_policy_violation(&error));
        assert_eq!(
            map_reqwest_error(error).message(),
            "upstream source URL is blocked by server policy"
        );
    }

    #[tokio::test]
    async fn redirect_to_metadata_is_rejected_without_exposing_the_target() {
        let redirect = spawn_router(Router::new().route(
            "/start",
            get(|| async {
                Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header(header::LOCATION, "http://169.254.169.254/latest/meta-data")
                    .body(Body::empty())
                    .unwrap()
            }),
        ))
        .await;
        let error = test_client()
            .unwrap()
            .get(&format!("{redirect}/start"))
            .unwrap()
            .send()
            .await
            .expect_err("metadata redirect must be rejected");
        let error = map_reqwest_error(error);
        assert_eq!(
            error.message(),
            "upstream source URL is blocked by server policy"
        );
        assert!(!error.message().contains("169.254.169.254"));
    }

    #[tokio::test]
    async fn native_forward_preserves_http_contract_and_replaces_sensitive_auth_headers() {
        let (url, recorded) = spawn_mock_upstream().await;
        let client = test_client().expect("native transport client");
        let protocols = [
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ];

        for protocol in protocols {
            let body = Bytes::from(format!(r#"{{"protocol":"{protocol}"}}"#));
            let mut headers = HeaderMap::new();
            headers.insert(
                header::AUTHORIZATION,
                HeaderValue::from_static("Bearer downstream-secret"),
            );
            headers.insert("x-api-key", HeaderValue::from_static("downstream-secret"));
            headers.insert("x-request-id", HeaderValue::from_static("request-header"));

            let response = forward_url(
                &client,
                &url,
                &account(),
                Some("upstream-secret"),
                protocol,
                &headers,
                body.clone(),
            )
            .await
            .expect("native forward response");

            assert_eq!(response.status(), StatusCode::IM_A_TEAPOT);
            assert_eq!(
                response.headers().get("x-upstream-response"),
                Some(&HeaderValue::from_static("preserved"))
            );
            let usage = usage_from_response(&response).expect("explicit missing usage report");
            assert_eq!(usage, UsageReport::missing());
            let response_body = to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("native response body");
            assert_eq!(
                response_body.as_ref(),
                br#"{"error":{"message":"upstream rejected the request"}}"#
            );
        }

        let recorded = recorded.lock().unwrap();
        assert_eq!(recorded.len(), protocols.len());
        for ((protocol, request), expected_body) in protocols
            .into_iter()
            .zip(recorded.iter())
            .zip(protocols.map(|protocol| format!(r#"{{"protocol":"{protocol}"}}"#)))
        {
            assert_eq!(request.body, expected_body.as_bytes());
            assert_eq!(
                request
                    .headers
                    .get("x-request-id")
                    .and_then(|value| value.to_str().ok()),
                Some("request-header")
            );
            if protocol == Protocol::AnthropicMessages {
                assert_eq!(
                    request
                        .headers
                        .get("x-api-key")
                        .and_then(|value| value.to_str().ok()),
                    Some("upstream-secret")
                );
                assert!(request.headers.get(header::AUTHORIZATION).is_none());
            } else {
                assert_eq!(
                    request
                        .headers
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok()),
                    Some("Bearer upstream-secret")
                );
                assert!(request.headers.get("x-api-key").is_none());
            }
        }
    }

    #[tokio::test]
    async fn connection_timeout_is_reported_before_a_stream_response_exists() {
        let delayed = spawn_router(Router::new().fallback(|| async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Response::new(Body::from("late"))
        }))
        .await;
        let config = StreamConfig {
            heartbeat_interval: Duration::ZERO,
            connection_timeout: Duration::from_millis(10),
            first_event_timeout: Duration::from_secs(1),
            idle_timeout: Duration::from_secs(1),
            total_timeout: Duration::from_secs(1),
        };
        let result = forward_url_with_config(
            &test_client().unwrap(),
            &format!("{delayed}/stream"),
            &account(),
            None,
            Protocol::OpenAiResponses,
            &HeaderMap::new(),
            Bytes::from_static(br#"{"model":"m","stream":true}"#),
            &config,
            std::time::Instant::now(),
        )
        .await;
        assert!(matches!(
            result,
            Err(TransportError::Timeout(
                StreamTermination::ConnectionTimeout
            ))
        ));
    }

    // -----------------------------------------------------------------------
    // sanitize_thinking_params — Chat Completions
    // -----------------------------------------------------------------------

    #[test]
    fn chat_sanitize_strips_thinking_when_assistant_lacks_reasoning() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "world"},
                {"role": "user", "content": "next"}
            ],
            "thinking": {"type": "enabled", "budget_tokens": 4096},
            "reasoning_effort": "high",
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(result.get("thinking").is_none());
        assert!(result.get("reasoning_effort").is_none());
        assert_eq!(result["model"], "MiniMax-M3");
        assert_eq!(result["stream"], true);
    }

    #[test]
    fn chat_sanitize_strips_reasoning_split() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "world"},
                {"role": "user", "content": "follow up"}
            ],
            "reasoning_split": true,
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(result.get("reasoning_split").is_none());
    }

    #[test]
    fn chat_sanitize_preserves_thinking_when_no_assistant() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "messages": [{"role": "user", "content": "first turn"}],
            "thinking": {"type": "enabled", "budget_tokens": 4096},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(result.get("thinking").is_some());
    }

    #[test]
    fn chat_sanitize_preserves_when_reasoning_content_present() {
        let payload = serde_json::json!({
            "model": "deepseek-reasoner",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "answer", "reasoning_content": "let me think..."},
                {"role": "user", "content": "next"}
            ],
            "thinking": {"type": "enabled", "budget_tokens": 4096},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(result.get("thinking").is_some());
    }

    #[test]
    fn chat_sanitize_preserves_when_reasoning_details_present() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "answer", "reasoning_details": [{"text": "step 1"}]},
                {"role": "user", "content": "next"}
            ],
            "reasoning_split": true,
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
    }

    #[test]
    fn chat_sanitize_preserves_when_reasoning_text_present() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "answer", "reasoning_text": "thought process"},
                {"role": "user", "content": "next"}
            ],
            "thinking": {"type": "enabled"},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
    }

    #[test]
    fn chat_sanitize_preserves_when_content_has_reasoning_tags() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "<reasoning_content>thinking...</reasoning_content>visible answer"},
                {"role": "user", "content": "next"}
            ],
            "thinking": {"type": "enabled"},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
    }

    #[test]
    fn chat_sanitize_noop_without_thinking_params() {
        let payload = serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "world"}
            ],
            "stream": true
        });
        let original = serde_json::to_vec(&payload).unwrap();
        let mut body = Bytes::from(original.clone());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
        assert_eq!(body.as_ref(), original.as_slice());
    }

    #[test]
    fn chat_sanitize_handles_null_reasoning_field() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "answer", "reasoning_content": null},
                {"role": "user", "content": "next"}
            ],
            "thinking": {"type": "enabled"},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(result.get("thinking").is_none());
    }

    #[test]
    fn chat_sanitize_strips_when_only_some_assistants_lack_reasoning() {
        let payload = serde_json::json!({
            "model": "deepseek-reasoner",
            "messages": [
                {"role": "user", "content": "turn 1"},
                {"role": "assistant", "content": "a1", "reasoning_content": "think1"},
                {"role": "user", "content": "turn 2"},
                {"role": "assistant", "content": "a2"},
                {"role": "user", "content": "turn 3"}
            ],
            "thinking": {"type": "enabled"},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(result.get("thinking").is_none());
    }

    // -----------------------------------------------------------------------
    // sanitize_thinking_params — OpenAI Responses
    // -----------------------------------------------------------------------

    #[test]
    fn responses_sanitize_strips_reasoning_when_assistant_output_lacks_reasoning_items() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hello"}]},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "world"}]},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next"}]}
            ],
            "reasoning": {"effort": "high"},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiResponses
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(result.get("reasoning").is_none());
        assert_eq!(result["model"], "MiniMax-M3");
        assert_eq!(result["stream"], true);
    }

    #[test]
    fn responses_sanitize_preserves_reasoning_when_no_assistant_output() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "first turn"}]}
            ],
            "reasoning": {"effort": "high"},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiResponses
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(result.get("reasoning").is_some());
    }

    #[test]
    fn responses_sanitize_preserves_reasoning_when_reasoning_items_present() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hello"}]},
                {"type": "reasoning", "id": "rs_001", "summary": [{"type": "summary_text", "text": "thought"}]},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "world"}]},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next"}]}
            ],
            "reasoning": {"effort": "high"},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiResponses
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(result.get("reasoning").is_some());
    }

    #[test]
    fn responses_sanitize_strips_when_function_call_output_present_but_no_reasoning() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "search for X"}]},
                {"type": "function_call", "id": "fc_001", "name": "search", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "fc_001", "output": "found X"},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "thanks"}]}
            ],
            "reasoning": {"effort": "medium"},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiResponses
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(result.get("reasoning").is_none());
    }

    #[test]
    fn responses_sanitize_noop_without_reasoning_param() {
        let payload = serde_json::json!({
            "model": "gpt-4o",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hello"}]},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "world"}]}
            ],
            "stream": true
        });
        let original = serde_json::to_vec(&payload).unwrap();
        let mut body = Bytes::from(original.clone());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiResponses
        ));
        assert_eq!(body.as_ref(), original.as_slice());
    }

    #[test]
    fn responses_sanitize_noop_for_string_input() {
        let payload = serde_json::json!({
            "model": "MiniMax-M3",
            "input": "just a single question",
            "reasoning": {"effort": "high"},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiResponses
        ));
    }

    #[test]
    fn sanitize_handles_invalid_json_any_protocol() {
        let mut body = Bytes::from_static(b"not json at all");
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
        let mut body = Bytes::from_static(b"not json at all");
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiResponses
        ));
    }

    #[test]
    fn chat_sanitize_preserves_thinking_disabled_first_turn_with_tools() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-flash",
            "messages": [{"role":"user","content":"Call lookup_weather for Tokyo. Do not answer directly."}],
            "tools": [{"type":"function","function":{"name":"lookup_weather","description":"Look up weather","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}}],
            "tool_choice": "required",
            "thinking": {"type": "disabled"},
            "max_tokens": 256,
            "stream": false
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        let stripped = sanitize_thinking_params(&mut body, Protocol::OpenAiChatCompletions);
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(
            !stripped,
            "thinking:disabled on first turn without assistants must NOT be stripped, but was: {result}"
        );
        assert!(
            result.get("thinking").is_some(),
            "thinking key must be preserved"
        );
    }

    #[test]
    fn chat_sanitize_preserves_thinking_disabled_second_turn_with_tool_result() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-flash",
            "messages": [
                {"role": "user", "content": "Call lookup_weather for Tokyo."},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {"name": "lookup_weather", "arguments": "{\"city\":\"Tokyo\"}"}
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_abc",
                    "content": "{\"temperature_c\": 22}"
                }
            ],
            "tools": [{"type":"function","function":{"name":"lookup_weather","description":"Look up weather","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}}],
            "tool_choice": "none",
            "thinking": {"type": "disabled"},
            "max_tokens": 256,
            "stream": false
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        let stripped = sanitize_thinking_params(&mut body, Protocol::OpenAiChatCompletions);
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(
            !stripped,
            "thinking:disabled on second turn with tool result must NOT be stripped: {result}"
        );
        assert!(
            result.get("thinking").is_some(),
            "thinking key must be preserved when type is disabled"
        );
    }

    #[test]
    fn chat_sanitize_strips_thinking_enabled_when_assistant_lacks_reasoning() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-flash",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "world"},
                {"role": "user", "content": "next"}
            ],
            "thinking": {"type": "enabled", "budget_tokens": 4096},
            "stream": true
        });
        let mut body = Bytes::from(serde_json::to_vec(&payload).unwrap());
        assert!(sanitize_thinking_params(
            &mut body,
            Protocol::OpenAiChatCompletions
        ));
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert!(
            result.get("thinking").is_none(),
            "thinking:enabled should be stripped when assistant lacks reasoning"
        );
    }

    #[test]
    fn sanitize_noop_for_anthropic_protocol() {
        let payload = serde_json::json!({
            "model": "claude-4",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "world"}
            ],
            "thinking": {"type": "enabled", "budget_tokens": 4096}
        });
        let original = serde_json::to_vec(&payload).unwrap();
        let mut body = Bytes::from(original.clone());
        assert!(!sanitize_thinking_params(
            &mut body,
            Protocol::AnthropicMessages
        ));
        assert_eq!(body.as_ref(), original.as_slice());
    }

    #[tokio::test]
    async fn native_stream_contract_adds_heartbeats_for_each_protocol_without_reordering() {
        let upstream = spawn_router(Router::new().fallback(|| async {
            let chunks = stream::once(async {
                tokio::time::sleep(Duration::from_millis(25)).await;
                Ok::<Bytes, std::io::Error>(Bytes::from_static(
                    b"data: {\"id\":\"one\"}\n\ndata: [DONE]\n\n",
                ))
            });
            Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(chunks))
                .unwrap()
        }))
        .await;
        let config = StreamConfig {
            heartbeat_interval: Duration::from_millis(5),
            connection_timeout: Duration::from_millis(500),
            first_event_timeout: Duration::from_millis(200),
            idle_timeout: Duration::from_millis(200),
            total_timeout: Duration::from_secs(1),
        };
        for protocol in [
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ] {
            let response = forward_url_with_config(
                &test_client().unwrap(),
                &format!("{upstream}/stream"),
                &account(),
                None,
                protocol,
                &HeaderMap::new(),
                Bytes::from_static(br#"{"model":"m","stream":true}"#),
                &config,
                std::time::Instant::now(),
            )
            .await
            .expect("native streaming response");
            let body = to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("native streaming body");
            let body = String::from_utf8_lossy(&body);
            assert!(
                body.contains(": gateway-heartbeat"),
                "heartbeat missing for {protocol}"
            );
            assert!(body.contains("data: {\"id\":\"one\"}"));
            assert!(body.contains("data: [DONE]"));
        }
    }
}
