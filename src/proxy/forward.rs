use super::{stream, transport};
use crate::domain::config;
use crate::domain::protocol::Protocol;
use crate::domain::routing::ResolvedRoute;
use crate::http::SourceHttpClient;
use crate::infra::{events, secrets};
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
    events: &events::EventRepository,
    http: &SourceHttpClient,
    route: &ResolvedRoute,
    account: &config::AccountConfig,
    request_id: &str,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    let credential =
        resolve_credential(secrets, events, &route.source_id, account, request_id).await;
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
    events: &events::EventRepository,
    http: &SourceHttpClient,
    provider: &config::ProviderConfig,
    account: &config::AccountConfig,
    source_id: &str,
    request_id: &str,
    protocol: Protocol,
    mode: &str,
    upstream_endpoint: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = resolve_credential(secrets, events, source_id, account, request_id).await;
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

async fn resolve_credential(
    secrets: &secrets::SecretResolver,
    events: &events::EventRepository,
    source_id: &str,
    account: &config::AccountConfig,
    request_id: &str,
) -> Option<String> {
    match secrets.resolve_account_credential_result(account) {
        Ok(credential) => credential,
        Err(error) => {
            tracing::warn!(
                account_id = %account.id,
                source_id,
                error_code = error.code(),
                "credential resolution failed"
            );
            events
                .record(
                    events::SystemEvent::new(
                        "security",
                        "credential.resolution_failed",
                        "error",
                        "account",
                        "Account credential resolution failed",
                    )
                    .subject_id(account.id.clone())
                    .correlation_id(request_id.to_owned())
                    .details(serde_json::json!({
                        "source_id": source_id,
                        "error_code": error.code(),
                    })),
                )
                .await;
            None
        }
    }
}
