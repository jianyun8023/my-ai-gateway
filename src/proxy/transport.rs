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
    // The gateway parses both JSON usage and SSE events, so it owns upstream
    // content-encoding negotiation. Let the shared client advertise the gzip
    // decoder it actually supports instead of forwarding a caller's br/zstd
    // preferences. Reqwest decodes the body and removes its encoding/length
    // headers before either parsing or forwarding the response.
    for (name, value) in request_headers {
        if !matches!(
            name.as_str(),
            "host" | "content-length" | "authorization" | "x-api-key" | "accept-encoding"
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
    use std::io::Write;
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

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[tokio::test]
    async fn gzip_json_is_decoded_before_usage_parsing_for_all_protocols() {
        for (protocol, usage, accepted_encoding) in [
            (
                Protocol::OpenAiChatCompletions,
                serde_json::json!({"prompt_tokens": 91, "completion_tokens": 48, "total_tokens": 139}),
                "gzip, deflate, br, zstd",
            ),
            (
                Protocol::OpenAiResponses,
                serde_json::json!({"input_tokens": 91, "output_tokens": 48, "total_tokens": 139}),
                "br",
            ),
            (
                Protocol::AnthropicMessages,
                serde_json::json!({"input_tokens": 91, "output_tokens": 48}),
                "identity",
            ),
        ] {
            let payload = serde_json::json!({
                "usage": usage,
                "provider_extension": {"preserved": true},
                "content": [{"type": "thinking", "signature": "synthetic-signature"}],
            })
            .to_string();
            let compressed = gzip(payload.as_bytes());
            let recorded_encoding = Arc::new(Mutex::new(None));
            let encoding = recorded_encoding.clone();
            let upstream = spawn_router(Router::new().fallback(move |headers: HeaderMap| {
                let compressed = compressed.clone();
                *encoding.lock().unwrap() = headers.get(header::ACCEPT_ENCODING).cloned();
                async move {
                    Response::builder()
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::CONTENT_ENCODING, "gzip")
                        .header(header::CONTENT_LENGTH, compressed.len())
                        .header("x-upstream-response", "preserved")
                        .body(Body::from(compressed))
                        .unwrap()
                }
            }))
            .await;
            let headers = HeaderMap::from_iter([(
                header::ACCEPT_ENCODING,
                HeaderValue::from_static(accepted_encoding),
            )]);
            let response = forward_url(
                &test_client().unwrap(),
                &upstream,
                &account(),
                None,
                protocol,
                &headers,
                Bytes::from_static(br#"{"model":"m","stream":false}"#),
            )
            .await
            .expect("decode gzip JSON");
            let report = usage_from_response(&response).unwrap();
            assert_eq!(report.source, "upstream");
            assert_eq!(report.input_tokens, 91);
            assert_eq!(report.output_tokens, 48);
            assert_eq!(report.total_tokens, 139);
            assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
            assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
            assert_eq!(response.headers()["x-upstream-response"], "preserved");
            assert_eq!(
                to_bytes(response.into_body(), 1024 * 1024).await.unwrap(),
                payload.as_bytes(),
            );
            assert_eq!(
                recorded_encoding.lock().unwrap().as_ref(),
                Some(&HeaderValue::from_static("gzip")),
                "upstream encoding negotiation belongs to the gateway",
            );
        }
    }

    #[tokio::test]
    async fn gzip_sse_is_decoded_incrementally_before_tracking_and_usage() {
        use futures_util::StreamExt;

        for (protocol, first, last) in [
            (
                Protocol::OpenAiChatCompletions,
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":91,\"completion_tokens\":48,\"total_tokens\":139}}\n\ndata: [DONE]\n\n",
            ),
            (
                Protocol::OpenAiResponses,
                "event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0}\n\n",
                "event: response.completed\ndata: {\"type\":\"response.completed\",\"sequence_number\":1,\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":91,\"output_tokens\":48,\"total_tokens\":139}}}\n\n",
            ),
            (
                Protocol::AnthropicMessages,
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":91}}}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":48}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ),
        ] {
            let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(first.as_bytes()).unwrap();
            encoder.flush().unwrap();
            let first_len = encoder.get_ref().len();
            encoder.write_all(last.as_bytes()).unwrap();
            let compressed = encoder.finish().unwrap();
            let (sender, receiver) = tokio::sync::mpsc::channel::<Bytes>(2);
            sender.send(Bytes::copy_from_slice(&compressed[..first_len])).await.unwrap();
            let receiver = Arc::new(Mutex::new(Some(receiver)));
            let upstream = spawn_router(Router::new().fallback(move || {
                let receiver = receiver.lock().unwrap().take().unwrap();
                async move {
                    let body = stream::unfold(receiver, |mut receiver| async {
                        receiver.recv().await.map(|bytes| (Ok::<_, std::io::Error>(bytes), receiver))
                    });
                    Response::builder()
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .header(header::CONTENT_ENCODING, "gzip")
                        .body(Body::from_stream(body))
                        .unwrap()
                }
            }))
            .await;
            let response = forward_url(
                &test_client().unwrap(),
                &upstream,
                &account(),
                None,
                protocol,
                &HeaderMap::new(),
                Bytes::from_static(br#"{"model":"m","stream":true}"#),
            )
            .await
            .unwrap();
            assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
            let mut body = response.into_body().into_data_stream();
            let first_chunk = tokio::time::timeout(Duration::from_secs(1), body.next())
                .await
                .expect("a decoded event must arrive before the remaining gzip body")
                .unwrap()
                .unwrap();
            assert_eq!(first_chunk.as_ref(), first.as_bytes());
            sender.send(Bytes::copy_from_slice(&compressed[first_len..])).await.unwrap();
            drop(sender);
            let remaining = tokio::time::timeout(Duration::from_secs(1), body.try_collect::<Vec<_>>())
                .await
                .unwrap()
                .unwrap();
            let mut decoded = first_chunk.to_vec();
            for chunk in remaining { decoded.extend_from_slice(&chunk); }
            assert_eq!(decoded, format!("{first}{last}").as_bytes());
            let report = crate::proxy::usage::usage_for_sse_response("gzip-sse", true, b"{}", &decoded);
            assert_eq!(report.source, "parsed");
            assert_eq!(report.input_tokens, 91);
            assert_eq!(report.output_tokens, 48);
        }
    }

    #[tokio::test]
    async fn truncated_gzip_is_an_error_for_json_and_sse() {
        for streaming in [false, true] {
            let mut compressed = gzip(if streaming {
                b"event: response.created\ndata: {\"type\":\"response.created\"}\n\n"
            } else {
                br#"{"usage":{"input_tokens":91,"output_tokens":48}}"#
            });
            compressed.truncate(compressed.len() - 8);
            let upstream = spawn_router(Router::new().fallback(move || {
                let compressed = compressed.clone();
                async move {
                    Response::builder()
                        .header(
                            header::CONTENT_TYPE,
                            if streaming {
                                "text/event-stream"
                            } else {
                                "application/json"
                            },
                        )
                        .header(header::CONTENT_ENCODING, "gzip")
                        .body(Body::from(compressed))
                        .unwrap()
                }
            }))
            .await;
            let response = forward_url(
                &test_client().unwrap(),
                &upstream,
                &account(),
                None,
                Protocol::OpenAiResponses,
                &HeaderMap::new(),
                Bytes::from(format!(r#"{{"model":"m","stream":{streaming}}}"#)),
            )
            .await;
            if streaming {
                let bytes = to_bytes(response.unwrap().into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                let text = String::from_utf8(bytes.to_vec()).unwrap();
                assert!(text.contains("gateway_upstream_error"));
                assert!(!text.contains("response.completed"));
            } else {
                assert!(matches!(response, Err(TransportError::Request)));
            }
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
