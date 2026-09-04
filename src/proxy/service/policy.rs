fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
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

fn transport_error_status(error: &transport::TransportError) -> StatusCode {
    if matches!(error, transport::TransportError::Timeout(_)) {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    }
}

async fn record_response_health(
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
