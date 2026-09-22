use crate::domain::config;
use crate::domain::config::GatewayConfig;
use crate::domain::protocol::Protocol;
use crate::domain::routing::ResolvedRoute;
use crate::infra::health;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct FallbackCandidate<'a> {
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

pub(super) async fn available_fallback_candidates<'a>(
    config: &'a GatewayConfig,
    health: &health::HealthRegistry,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
) -> Vec<FallbackCandidate<'a>> {
    let mut available = Vec::new();
    if !route.fallback_bindings.is_empty() {
        for binding in &route.fallback_bindings {
            if binding.account_id == route.primary_account_id
                && (route.strategy != "ordered_fallback"
                    || binding.upstream_model_id == route.upstream_model_id)
            {
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
        if route.strategy != "ordered_fallback"
            && available.iter().any(|candidate| candidate.mode == "native")
        {
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
    available
}

pub(crate) async fn select_fallback_candidate<'a>(
    config: &'a GatewayConfig,
    health: &health::HealthRegistry,
    route: &ResolvedRoute,
    model: &str,
    protocol: Protocol,
) -> Option<FallbackCandidate<'a>> {
    let mut available = available_fallback_candidates(config, health, route, model, protocol).await;
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

impl<'a> FallbackCandidate<'a> {
    pub(super) fn primary(
        route: &ResolvedRoute,
        account: &'a config::AccountConfig,
        provider: &'a config::ProviderConfig,
        model: &str,
    ) -> Self {
        Self {
            account,
            provider,
            provider_id: route.provider_id.clone(),
            source_id: route.source_id.clone(),
            upstream_model: if route.binding_id.is_none() {
                account
                    .model_map
                    .get(model)
                    .cloned()
                    .unwrap_or_else(|| route.upstream_model_id.clone())
            } else {
                route.upstream_model_id.clone()
            },
            protocol_upstream: route.protocol_upstream,
            mode: route.mode.clone(),
            upstream_endpoint: Some(route.upstream_endpoint.clone()),
            degraded_features: route.degraded_features.clone(),
        }
    }
}
