use super::{stream, transport};
use crate::domain::config;
use crate::domain::protocol::Protocol;
use crate::domain::routing::ResolvedRoute;
use crate::http::SourceHttpClient;
use crate::infra::secrets;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Response};
use std::time::Instant;

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(name = "gateway.forward", skip_all, fields(
    source_id = %route.source_id,
    account_id = %account.id,
    upstream_model = %route.upstream_model_id,
))]
pub(super) async fn forward_account(
    secrets: &secrets::SecretResolver,
    http: &SourceHttpClient,
    route: &ResolvedRoute,
    account: &config::AccountConfig,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = secrets.resolve_account_credential(account);
    if route.mode == "adapter" {
        return dispatch_adapter(
            http,
            Some(route.upstream_endpoint.as_str()),
            account,
            credential.as_deref(),
            route.protocol_upstream,
            headers,
            body,
            stream_config,
            request_started,
        )
        .await;
    } else {
        transport::forward_url_with_config(
            http,
            &route.upstream_endpoint,
            account,
            credential.as_deref(),
            route.protocol_upstream,
            headers,
            body,
            stream_config,
            request_started,
        )
        .await
    }
}

/// Execute an adapter-mode upstream call.
///
/// No production adapters are registered (issue #157): configuration and
/// snapshot validation reject unknown adapters before routing, so production
/// traffic can only reach this point through a stale snapshot. Fail closed
/// with a transport error instead of guessing a conversion. Unit tests pass
/// the request through to the source-protocol endpoint unchanged so the
/// adapter framework paths (degraded warnings, early fallback) stay covered
/// without a concrete adapter implementation.
#[allow(clippy::too_many_arguments)]
pub(super) async fn dispatch_adapter(
    http: &SourceHttpClient,
    upstream_endpoint: Option<&str>,
    account: &config::AccountConfig,
    credential: Option<&str>,
    protocol_upstream: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    #[cfg(test)]
    if let Some(endpoint) = upstream_endpoint {
        return transport::forward_url_with_config(
            http,
            endpoint,
            account,
            credential,
            protocol_upstream,
            headers,
            body,
            stream_config,
            request_started,
        )
        .await;
    }
    #[cfg(not(test))]
    let _ = (
        http,
        upstream_endpoint,
        account,
        credential,
        protocol_upstream,
        headers,
        body,
        stream_config,
        request_started,
    );
    Err(transport::TransportError::Request)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn forward_fallback(
    secrets: &secrets::SecretResolver,
    http: &SourceHttpClient,
    provider: &config::ProviderConfig,
    account: &config::AccountConfig,
    protocol: Protocol,
    mode: &str,
    upstream_endpoint: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = secrets.resolve_account_credential(account);
    if mode == "adapter" {
        return dispatch_adapter(
            http,
            upstream_endpoint,
            account,
            credential.as_deref(),
            protocol,
            headers,
            body,
            stream_config,
            request_started,
        )
        .await;
    }
    if let Some(endpoint) = upstream_endpoint {
        return transport::forward_url_with_config(
            http,
            endpoint,
            account,
            credential.as_deref(),
            protocol,
            headers,
            body,
            stream_config,
            request_started,
        )
        .await;
    }
    transport::forward_with_config(
        http,
        provider,
        account,
        credential.as_deref(),
        protocol,
        headers,
        body,
        stream_config,
        request_started,
    )
    .await
}
