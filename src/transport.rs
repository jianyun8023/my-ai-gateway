use crate::{
    config::{AccountConfig, ProviderConfig},
    protocol::Protocol,
    usage::{extract_json_bytes, UsageReport},
};
use axum::{
    body::Body,
    http::{HeaderMap, Response, StatusCode},
};
use bytes::Bytes;
use futures_util::TryStreamExt;
use reqwest::Client;
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
        let report = extract_json_bytes(&bytes)
            .or_else(|| Some(crate::usage::estimate(&request_payload, &bytes)));
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
        if let Some(report) = report {
            response.extensions_mut().insert(report);
        }
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
