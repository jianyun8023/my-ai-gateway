use super::forward::forward_fallback;
use super::policy::{record_response_health, transport_error_status};
use super::{stream, transport};
use crate::domain::config;
use crate::domain::config::GatewayConfig;
use crate::domain::protocol::Protocol;
use crate::domain::routing::ResolvedRoute;
use crate::http::response::data_plane_error_response;
use crate::http::SourceHttpClient;
use crate::infra::{db, events, health, observability, secrets};
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Response};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(super) struct FallbackCandidate<'a> {
    pub(super) account: &'a config::AccountConfig,
    pub(super) provider: &'a config::ProviderConfig,
    pub(super) provider_id: String,
    pub(super) source_id: String,
    pub(super) upstream_model: String,
    pub(super) protocol_upstream: Protocol,
    pub(super) mode: String,
    pub(super) upstream_endpoint: Option<String>,
    pub(super) degraded_features: Vec<String>,
}

pub(super) async fn select_fallback_candidate<'a>(
    config: &'a GatewayConfig,
    health: &health::HealthRegistry,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
) -> Option<FallbackCandidate<'a>> {
    let mut available = Vec::new();
    if !route.fallback_bindings.is_empty() {
        for binding in &route.fallback_bindings {
            if binding.account_id == route.primary_account_id {
                continue;
            }
            let Some(account) = config.account(&binding.account_id) else {
                continue;
            };
            if !account.enabled || !health.is_available(&account.id).await {
                continue;
            }
            let Some(provider) = config.provider(&binding.source_id) else {
                continue;
            };
            available.push(FallbackCandidate {
                account,
                provider,
                provider_id: binding.provider_id.clone(),
                source_id: binding.source_id.clone(),
                upstream_model: binding.upstream_model_id.clone(),
                protocol_upstream: binding.protocol_upstream,
                mode: binding.mode.clone(),
                upstream_endpoint: Some(binding.upstream_endpoint.clone()),
                degraded_features: binding.degraded_features.clone(),
            });
        }
        if available.iter().any(|candidate| candidate.mode == "native") {
            available.retain(|candidate| candidate.mode == "native");
        }
    } else {
        for id in &route.fallback_accounts {
            if id == &route.primary_account_id {
                continue;
            }
            let Some(account) = config.account(id) else {
                continue;
            };
            if !account.enabled {
                continue;
            }
            if !health.is_available(&account.id).await {
                continue;
            }
            let Some(provider) = config.provider(&account.provider_id) else {
                continue;
            };
            if account.provider_id != route.source_id {
                let cap =
                    config.protocol_capability(&provider.id, Some(&account.id), model, protocol);
                if cap.mode != config::ProtocolMode::Native {
                    continue;
                }
            }
            let upstream_model = account
                .model_map
                .get(model)
                .cloned()
                .unwrap_or_else(|| model.to_string());
            available.push(FallbackCandidate {
                account,
                provider,
                provider_id: provider.id.clone(),
                source_id: provider.id.clone(),
                upstream_model,
                protocol_upstream: route.protocol_upstream,
                mode: route.mode.clone(),
                upstream_endpoint: None,
                degraded_features: route.degraded_features.clone(),
            });
        }
    }
    if available.is_empty() {
        return None;
    }
    let total: u32 = available.iter().map(|c| c.account.weight.max(1)).sum();
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
        % total.max(1);
    let mut cursor = 0;
    let mut selected_index = 0;
    for (i, candidate) in available.iter().enumerate() {
        cursor += candidate.account.weight.max(1);
        if tick < cursor {
            selected_index = i;
            break;
        }
    }
    Some(available.swap_remove(selected_index))
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(name = "gateway.fallback", skip_all, fields(model = %model, protocol = %protocol))]
pub(super) async fn try_fallback(
    config: &GatewayConfig,
    secrets: &secrets::SecretResolver,
    events: &events::EventRepository,
    health: &health::HealthRegistry,
    http: &SourceHttpClient,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    first: Response<Body>,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
    request_id: &str,
) -> (Response<Body>, Vec<db::UsageAttempt>) {
    let mut attempts = Vec::new();
    let Some(candidate) = select_fallback_candidate(config, health, route, model, protocol).await
    else {
        return (first, attempts);
    };
    let prepared = transport::prepare_model_request(&body, model, &candidate.upstream_model);
    let started = Instant::now();
    match forward_fallback(
        secrets,
        events,
        http,
        candidate.provider,
        candidate.account,
        &candidate.source_id,
        request_id,
        candidate.protocol_upstream,
        &candidate.mode,
        candidate.upstream_endpoint.as_deref(),
        headers,
        prepared.body,
        stream_config,
        request_started,
    )
    .await
    {
        Ok(response) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider_id.clone(),
                source_id: candidate.source_id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(prepared.upstream_model_id),
                status_code: response.status().as_u16() as i32,
                success: response.status().is_success(),
                latency_ms: started.elapsed().as_millis() as i64,
            });
            record_response_health(
                health,
                &candidate.source_id,
                &candidate.account.id,
                response.status(),
            )
            .await;
            observability::record_attempt(
                &protocol.to_string(),
                &candidate.source_id,
                &candidate.account.id,
                response.status().as_u16(),
                true,
            );
            (response, attempts)
        }
        Err(error) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider_id.clone(),
                source_id: candidate.source_id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(prepared.upstream_model_id),
                status_code: error.status_code(),
                success: false,
                latency_ms: started.elapsed().as_millis() as i64,
            });
            observability::record_attempt(
                &protocol.to_string(),
                &candidate.source_id,
                &candidate.account.id,
                error.status_code() as u16,
                true,
            );
            health
                .mark_failure_with_details(
                    &candidate.account.id,
                    "passive",
                    Some("upstream_transport_error"),
                    Some("upstream request failed"),
                )
                .await;
            (first, attempts)
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(name = "gateway.fallback_error", skip_all, fields(model = %model, protocol = %protocol))]
pub(crate) async fn try_fallback_error(
    config: &GatewayConfig,
    secrets: &secrets::SecretResolver,
    events: &events::EventRepository,
    health: &health::HealthRegistry,
    http: &SourceHttpClient,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    body: Bytes,
    first_error: transport::TransportError,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
    request_id: &str,
) -> (Response<Body>, Vec<db::UsageAttempt>) {
    let mut attempts = Vec::new();
    let Some(candidate) = select_fallback_candidate(config, health, route, model, protocol).await
    else {
        return (
            data_plane_error_response(
                protocol,
                transport_error_status(&first_error),
                "upstream_request_failed",
                first_error.message(),
                request_id,
            ),
            attempts,
        );
    };
    let prepared = transport::prepare_model_request(&body, model, &candidate.upstream_model);
    let started = Instant::now();
    match forward_fallback(
        secrets,
        events,
        http,
        candidate.provider,
        candidate.account,
        &candidate.source_id,
        request_id,
        candidate.protocol_upstream,
        &candidate.mode,
        candidate.upstream_endpoint.as_deref(),
        headers,
        prepared.body,
        stream_config,
        request_started,
    )
    .await
    {
        Ok(response) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider_id.clone(),
                source_id: candidate.source_id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(prepared.upstream_model_id),
                status_code: response.status().as_u16() as i32,
                success: response.status().is_success(),
                latency_ms: started.elapsed().as_millis() as i64,
            });
            record_response_health(
                health,
                &candidate.source_id,
                &candidate.account.id,
                response.status(),
            )
            .await;
            observability::record_attempt(
                &protocol.to_string(),
                &candidate.source_id,
                &candidate.account.id,
                response.status().as_u16(),
                true,
            );
            (response, attempts)
        }
        Err(error) => {
            attempts.push(db::UsageAttempt {
                attempt_no: 1,
                provider_id: candidate.provider_id.clone(),
                source_id: candidate.source_id.clone(),
                account_id: candidate.account.id.clone(),
                upstream_model_id: Some(prepared.upstream_model_id),
                status_code: error.status_code(),
                success: false,
                latency_ms: started.elapsed().as_millis() as i64,
            });
            observability::record_attempt(
                &protocol.to_string(),
                &candidate.source_id,
                &candidate.account.id,
                error.status_code() as u16,
                true,
            );
            health
                .mark_failure_with_details(
                    &candidate.account.id,
                    "passive",
                    Some("upstream_transport_error"),
                    Some("upstream request failed"),
                )
                .await;
            (
                data_plane_error_response(
                    protocol,
                    transport_error_status(&error),
                    "upstream_request_failed",
                    error.message(),
                    request_id,
                ),
                attempts,
            )
        }
    }
}
