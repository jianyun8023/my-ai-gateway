use crate::{
    config::{AccountConfig, ProviderConfig},
    protocol::Protocol,
    usage::{usage_for_json_response, UsageReport},
};
use axum::{
    body::Body,
    http::{HeaderMap, Response, StatusCode},
};
use bytes::Bytes;
use futures_util::TryStreamExt;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug)]
pub enum TransportError {
    MissingEndpoint,
    Request(String),
}

impl TransportError {
    pub fn message(&self) -> &str {
        match self {
            Self::MissingEndpoint => "provider endpoint is not configured",
            Self::Request(message) => message,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PreparedModelRequest {
    pub body: Bytes,
    pub upstream_model_id: String,
}

/// Prepare the JSON body for one concrete upstream attempt.
///
/// The resolved Binding or account-level mapping only replaces the top-level
/// `model` field. Unmapped requests retain their original bytes exactly, and a
/// malformed or non-object body is left untouched so the recorded model always
/// matches what was actually sent.
pub fn prepare_model_request(
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

pub async fn forward(
    client: &Client,
    provider: &ProviderConfig,
    account: &AccountConfig,
    credential: Option<&str>,
    protocol: Protocol,
    request_headers: &HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, TransportError> {
    let endpoint = provider
        .endpoints
        .get(&protocol)
        .ok_or(TransportError::MissingEndpoint)?;
    let url = format!("{}{}", provider.base_url.trim_end_matches('/'), endpoint);
    forward_url(
        client,
        &url,
        account,
        credential,
        protocol,
        request_headers,
        body,
    )
    .await
}

pub async fn forward_url(
    client: &Client,
    url: &str,
    account: &AccountConfig,
    credential: Option<&str>,
    protocol: Protocol,
    request_headers: &HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, TransportError> {
    let request_payload = body.clone();
    let is_streaming = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("stream").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    let mut request = client.post(url).body(body);
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
    let upstream = request
        .send()
        .await
        .map_err(|error| TransportError::Request(error.to_string()))?;
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let upstream_headers = upstream.headers().clone();
    // Buffer non-streaming responses so the usage object is available to the
    // request lifecycle while preserving a normal response body. Streaming
    // responses stay a live byte stream to retain TTFT and backpressure.
    if !is_streaming {
        let bytes = upstream
            .bytes()
            .await
            .map_err(|error| TransportError::Request(error.to_string()))?;
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

/// Retrieve usage metadata attached by [`forward_url`].
pub fn usage_from_response(response: &Response<Body>) -> Option<UsageReport> {
    response.extensions().get::<UsageReport>().cloned()
}

pub fn client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        extract::Request,
        http::{header, HeaderValue},
        Router,
    };
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
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

    fn account() -> AccountConfig {
        AccountConfig {
            id: "account".into(),
            provider_id: "provider".into(),
            display_name: "Native account".into(),
            credential_env: None,
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

    #[tokio::test]
    async fn native_forward_preserves_http_contract_and_replaces_sensitive_auth_headers() {
        let (url, recorded) = spawn_mock_upstream().await;
        let client = client().expect("native transport client");
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
}
