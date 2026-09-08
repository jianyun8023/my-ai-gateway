use super::transport;
use crate::domain::routing::ResolvedRoute;
use crate::infra::{health, observability};
use axum::http::{HeaderMap, StatusCode};
use std::time::Duration;

pub(super) const MAX_SAME_ACCOUNT_RETRY_AFTER: Duration = Duration::from_secs(2);

/// Map an unusable primary account to the persisted `fallback_reason` code.
/// Only called when `health.available == false` (cooldown / unhealthy / stale
/// with residual failures / unknown with no row); the `disabled` status covers
/// account or source being turned off in the control plane.
pub(super) fn primary_unavailable_reason(health: &health::AccountHealth) -> String {
    if health.status == "disabled" {
        "account_disabled".into()
    } else {
        match health.status.as_str() {
            "cooling_down" => "account_cooling_down".into(),
            "unhealthy" => "account_unhealthy".into(),
            _ => "account_unavailable".into(),
        }
    }
}

pub(super) fn warn_degraded_route(request_id: &str, route: &ResolvedRoute) {
    if !route.is_degraded() {
        return;
    }
    warn_degraded_features(request_id, &route.route_id, &route.degraded_features);
}

pub(super) fn warn_degraded_features(
    request_id: &str,
    route_id: &str,
    degraded_features: &[String],
) {
    tracing::warn!(
        request_id = %request_id,
        route_id = %route_id,
        degraded_features = ?degraded_features,
        "route has degraded features due to adapter conversion"
    );
}

pub(super) fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

pub(super) fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get("retry-after")?.to_str().ok()?.trim();
    let delay = if let Ok(seconds) = value.parse::<u64>() {
        Duration::from_secs(seconds)
    } else {
        let retry_at = chrono::DateTime::parse_from_rfc2822(value)
            .ok()?
            .with_timezone(&chrono::Utc);
        retry_at
            .signed_duration_since(chrono::Utc::now())
            .to_std()
            .ok()?
    };
    (delay <= MAX_SAME_ACCOUNT_RETRY_AFTER).then_some(delay)
}

pub(super) fn transport_error_status(error: &transport::TransportError) -> StatusCode {
    if matches!(error, transport::TransportError::Timeout(_)) {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    }
}

pub(super) async fn record_response_health(
    health: &health::HealthRegistry,
    source_id: &str,
    account_id: &str,
    status: StatusCode,
) {
    if is_retryable(status) {
        let code = format!("upstream_http_{}", status.as_u16());
        let cooldown_started = health
            .mark_failure_with_details(
                account_id,
                "passive",
                Some(code.as_str()),
                Some("retryable upstream response"),
            )
            .await;
        if cooldown_started {
            observability::record_cooldown(source_id, account_id);
        }
    } else if status.is_success() {
        health.mark_success(account_id).await;
    }
}
