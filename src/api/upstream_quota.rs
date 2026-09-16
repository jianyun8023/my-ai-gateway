use axum::{
    body::Body,
    extract::{Path, State},
    http::{Response, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures_util::{future::join_all, StreamExt};
use reqwest::{header::HeaderName, Method, Url};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use std::time::{Duration, Instant};

use crate::{
    domain::provider_preset::SourceAuthConfig, http::response::error_response,
    infra::secrets::SecretResolverError, source_url::reqwest_error_is_policy_violation,
    state::AppState,
};

const QUOTA_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_QUOTA_RESPONSE_BYTES: usize = 1024 * 1024;
const MINIMAX_GLOBAL_QUOTA_URL: &str = "https://www.minimax.io/v1/token_plan/remains";
const MINIMAX_CN_QUOTA_URL: &str = "https://www.minimaxi.com/v1/token_plan/remains";

#[derive(Clone, Debug, FromRow)]
struct QuotaTarget {
    account_id: String,
    source_id: String,
    account_display_name: String,
    credential_env: Option<String>,
    credential_ciphertext: Option<String>,
    account_enabled: bool,
    source_display_name: String,
    provider_preset_id: String,
    base_url: String,
    auth_config: Value,
    source_enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct QuotaAccountView {
    account_id: String,
    account_display_name: String,
    source_id: String,
    source_display_name: String,
    provider_id: String,
    enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct QuotaResource {
    #[serde(rename = "type")]
    resource_type: &'static str,
    key: String,
    label: String,
    unit: String,
    used: Option<f64>,
    remaining: Option<f64>,
    limit: Option<f64>,
    reset_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct QuotaRefreshError {
    code: String,
    message: String,
    http_status: Option<u16>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UpstreamQuotaSnapshot {
    account: QuotaAccountView,
    status: &'static str,
    resources: Vec<QuotaResource>,
    fetched_at: Option<DateTime<Utc>>,
    attempted_at: DateTime<Utc>,
    latency_ms: i64,
    stale: bool,
    refresh_error: Option<QuotaRefreshError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw: Option<Value>,
}

#[derive(Debug)]
struct FetchFailure {
    status: &'static str,
    code: &'static str,
    message: &'static str,
    http_status: Option<u16>,
}

#[derive(Debug)]
struct ProviderQuota {
    resources: Vec<QuotaResource>,
    status_override: Option<&'static str>,
    raw: Value,
}

pub(crate) async fn list_upstream_quotas(State(state): State<AppState>) -> Response<Body> {
    let targets = match list_targets(&state).await {
        Ok(targets) => targets,
        Err(response) => return response,
    };
    let snapshots = join_all(
        targets
            .into_iter()
            .map(|target| fetch_snapshot(state.clone(), target, false)),
    )
    .await;
    (StatusCode::OK, Json(json!({ "data": snapshots }))).into_response()
}

pub(crate) async fn refresh_upstream_quotas(State(state): State<AppState>) -> Response<Body> {
    list_upstream_quotas(State(state)).await
}

pub(crate) async fn get_upstream_quota(
    Path(account_id): Path<String>,
    State(state): State<AppState>,
) -> Response<Body> {
    let target = match get_target(&state, &account_id).await {
        Ok(Some(target)) => target,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "quota_account_not_found",
                "upstream account not found",
            )
        }
        Err(response) => return response,
    };
    let snapshot = fetch_snapshot(state, target, true).await;
    (StatusCode::OK, Json(json!({ "data": snapshot }))).into_response()
}

pub(crate) async fn refresh_upstream_quota(
    Path(account_id): Path<String>,
    State(state): State<AppState>,
) -> Response<Body> {
    get_upstream_quota(Path(account_id), State(state)).await
}

async fn list_targets(state: &AppState) -> Result<Vec<QuotaTarget>, Response<Body>> {
    let database = state.db.as_ref().ok_or_else(database_unavailable)?;
    sqlx::query_as::<_, QuotaTarget>(
        "SELECT a.id AS account_id,a.source_id,a.display_name AS account_display_name,a.credential_env,a.credential_ciphertext,a.enabled AS account_enabled,s.display_name AS source_display_name,s.provider_preset_id,s.base_url,s.auth_config,s.enabled AS source_enabled FROM accounts a JOIN sources s ON s.id=a.source_id ORDER BY s.display_name,a.display_name,a.id",
    )
    .fetch_all(database.pool())
    .await
    .map_err(database_error)
}

async fn get_target(
    state: &AppState,
    account_id: &str,
) -> Result<Option<QuotaTarget>, Response<Body>> {
    let database = state.db.as_ref().ok_or_else(database_unavailable)?;
    sqlx::query_as::<_, QuotaTarget>(
        "SELECT a.id AS account_id,a.source_id,a.display_name AS account_display_name,a.credential_env,a.credential_ciphertext,a.enabled AS account_enabled,s.display_name AS source_display_name,s.provider_preset_id,s.base_url,s.auth_config,s.enabled AS source_enabled FROM accounts a JOIN sources s ON s.id=a.source_id WHERE a.id=$1",
    )
    .bind(account_id)
    .fetch_optional(database.pool())
    .await
    .map_err(database_error)
}

fn database_unavailable() -> Response<Body> {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
        "DATABASE_URL is not configured",
    )
}

fn database_error(error: sqlx::Error) -> Response<Body> {
    tracing::error!(%error, "upstream quota database query failed");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "quota_database_error",
        "upstream quota database query failed",
    )
}

async fn fetch_snapshot(
    state: AppState,
    target: QuotaTarget,
    include_raw: bool,
) -> UpstreamQuotaSnapshot {
    let attempted_at = Utc::now();
    let started = Instant::now();
    let account = account_view(&target);
    if !target.account_enabled || !target.source_enabled {
        return UpstreamQuotaSnapshot {
            account,
            status: "disabled",
            resources: Vec::new(),
            fetched_at: None,
            attempted_at,
            latency_ms: elapsed_ms(started),
            stale: false,
            refresh_error: None,
            raw: None,
        };
    }

    if !matches!(
        target.provider_preset_id.as_str(),
        "deepseek" | "minimax" | "kimi_code"
    ) {
        return UpstreamQuotaSnapshot {
            account,
            status: "unsupported",
            resources: Vec::new(),
            fetched_at: Some(Utc::now()),
            attempted_at,
            latency_ms: elapsed_ms(started),
            stale: false,
            refresh_error: None,
            raw: None,
        };
    }

    let credential = match state.secrets.resolve_account(
        &target.source_id,
        &target.account_id,
        target.credential_env.as_deref(),
        target.credential_ciphertext.as_deref(),
        None,
    ) {
        Ok(credential) => credential,
        Err(error) => {
            return failed_snapshot(account, attempted_at, started, secret_failure(error))
        }
    };

    let result = fetch_provider_quota(&state, &target, credential.as_str()).await;
    match result {
        Ok(provider) => {
            let status = provider
                .status_override
                .unwrap_or_else(|| quota_status(&provider.resources));
            UpstreamQuotaSnapshot {
                account,
                status,
                resources: provider.resources,
                fetched_at: Some(Utc::now()),
                attempted_at,
                latency_ms: elapsed_ms(started),
                stale: false,
                refresh_error: None,
                raw: include_raw.then_some(provider.raw),
            }
        }
        Err(failure) => failed_snapshot(account, attempted_at, started, failure),
    }
}

fn account_view(target: &QuotaTarget) -> QuotaAccountView {
    QuotaAccountView {
        account_id: target.account_id.clone(),
        account_display_name: target.account_display_name.clone(),
        source_id: target.source_id.clone(),
        source_display_name: target.source_display_name.clone(),
        provider_id: target.provider_preset_id.clone(),
        enabled: target.account_enabled && target.source_enabled,
    }
}

fn failed_snapshot(
    account: QuotaAccountView,
    attempted_at: DateTime<Utc>,
    started: Instant,
    failure: FetchFailure,
) -> UpstreamQuotaSnapshot {
    UpstreamQuotaSnapshot {
        account,
        status: failure.status,
        resources: Vec::new(),
        fetched_at: None,
        attempted_at,
        latency_ms: elapsed_ms(started),
        stale: false,
        refresh_error: Some(QuotaRefreshError {
            code: failure.code.into(),
            message: failure.message.into(),
            http_status: failure.http_status,
        }),
        raw: None,
    }
}

fn secret_failure(error: SecretResolverError) -> FetchFailure {
    FetchFailure {
        status: "auth_error",
        code: error.code(),
        message: error.public_message(),
        http_status: None,
    }
}

async fn fetch_provider_quota(
    state: &AppState,
    target: &QuotaTarget,
    credential: &str,
) -> Result<ProviderQuota, FetchFailure> {
    let url = quota_url(target)?;
    let auth =
        serde_json::from_value::<SourceAuthConfig>(target.auth_config.clone()).map_err(|_| {
            FetchFailure {
                status: "refresh_failed",
                code: "invalid_auth_config",
                message: "source authentication configuration is invalid",
                http_status: None,
            }
        })?;
    let mut request = state
        .http
        .request(Method::GET, url)
        .map_err(|_| FetchFailure {
            status: "refresh_failed",
            code: "source_url_blocked",
            message: "quota URL is blocked by server policy",
            http_status: None,
        })?
        .timeout(QUOTA_TIMEOUT);
    for (name, value) in &auth.default_headers {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| FetchFailure {
            status: "refresh_failed",
            code: "invalid_header_template",
            message: "source header template is invalid",
            http_status: None,
        })?;
        request = request.header(name, value);
    }
    let credential_name = HeaderName::from_bytes(auth.credential_header.header.as_bytes())
        .map_err(|_| FetchFailure {
            status: "refresh_failed",
            code: "invalid_header_template",
            message: "source credential header is invalid",
            http_status: None,
        })?;
    request = request.header(
        credential_name,
        format!("{}{}", auth.credential_header.prefix, credential),
    );

    let response = request.send().await.map_err(transport_failure)?;
    let status = response.status();
    if !status.is_success() {
        return Err(FetchFailure {
            status: if matches!(status.as_u16(), 401 | 403) {
                "auth_error"
            } else {
                "refresh_failed"
            },
            code: if matches!(status.as_u16(), 401 | 403) {
                "upstream_auth_failed"
            } else {
                "upstream_http_error"
            },
            message: if matches!(status.as_u16(), 401 | 403) {
                "upstream rejected the account credential"
            } else {
                "upstream quota endpoint returned a non-success status"
            },
            http_status: Some(status.as_u16()),
        });
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_QUOTA_RESPONSE_BYTES as u64)
    {
        return Err(quota_response_too_large(status));
    }
    let bytes = read_quota_body(response, status).await?;
    let raw: Value = serde_json::from_slice(&bytes).map_err(|_| FetchFailure {
        status: "refresh_failed",
        code: "invalid_quota_response",
        message: "upstream quota response is not valid JSON",
        http_status: Some(status.as_u16()),
    })?;
    parse_provider_quota(&target.provider_preset_id, raw, Utc::now())
}

async fn read_quota_body(
    response: reqwest::Response,
    status: StatusCode,
) -> Result<Vec<u8>, FetchFailure> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(transport_failure)?;
        append_quota_chunk(&mut body, &chunk, status)?;
    }
    Ok(body)
}

fn append_quota_chunk(
    body: &mut Vec<u8>,
    chunk: &[u8],
    status: StatusCode,
) -> Result<(), FetchFailure> {
    if body.len().saturating_add(chunk.len()) > MAX_QUOTA_RESPONSE_BYTES {
        return Err(quota_response_too_large(status));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

fn quota_response_too_large(status: StatusCode) -> FetchFailure {
    FetchFailure {
        status: "refresh_failed",
        code: "quota_response_too_large",
        message: "upstream quota response is too large",
        http_status: Some(status.as_u16()),
    }
}

fn quota_url(target: &QuotaTarget) -> Result<Url, FetchFailure> {
    match target.provider_preset_id.as_str() {
        "deepseek" => join_source_url(&target.base_url, "/user/balance"),
        "kimi_code" => join_source_url(&target.base_url, "/v1/usages"),
        "minimax" => {
            let base = Url::parse(&target.base_url).map_err(|_| invalid_quota_url())?;
            let host = base.host_str().unwrap_or_default().to_ascii_lowercase();
            Url::parse(if host.ends_with("minimaxi.com") {
                MINIMAX_CN_QUOTA_URL
            } else {
                MINIMAX_GLOBAL_QUOTA_URL
            })
            .map_err(|_| invalid_quota_url())
        }
        _ => Err(invalid_quota_url()),
    }
}

fn join_source_url(base_url: &str, endpoint: &str) -> Result<Url, FetchFailure> {
    let base = Url::parse(base_url).map_err(|_| invalid_quota_url())?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(invalid_quota_url());
    }
    Url::parse(&format!(
        "{}{}",
        base.as_str().trim_end_matches('/'),
        endpoint
    ))
    .map_err(|_| invalid_quota_url())
}

fn invalid_quota_url() -> FetchFailure {
    FetchFailure {
        status: "refresh_failed",
        code: "invalid_quota_url",
        message: "upstream quota URL is invalid",
        http_status: None,
    }
}

fn transport_failure(error: reqwest::Error) -> FetchFailure {
    if reqwest_error_is_policy_violation(&error) {
        FetchFailure {
            status: "refresh_failed",
            code: "source_url_blocked",
            message: "quota URL is blocked by server policy",
            http_status: None,
        }
    } else if error.is_timeout() {
        FetchFailure {
            status: "refresh_failed",
            code: "upstream_timeout",
            message: "upstream quota request timed out",
            http_status: None,
        }
    } else if error.is_connect() {
        FetchFailure {
            status: "refresh_failed",
            code: "upstream_connect_failed",
            message: "upstream quota connection failed",
            http_status: None,
        }
    } else {
        FetchFailure {
            status: "refresh_failed",
            code: "upstream_request_failed",
            message: "upstream quota request failed",
            http_status: None,
        }
    }
}

fn parse_provider_quota(
    provider_id: &str,
    raw: Value,
    now: DateTime<Utc>,
) -> Result<ProviderQuota, FetchFailure> {
    match provider_id {
        "deepseek" => parse_deepseek(raw),
        "kimi_code" => parse_kimi(raw),
        "minimax" => parse_minimax(raw, now),
        _ => Ok(ProviderQuota {
            resources: Vec::new(),
            status_override: Some("unsupported"),
            raw,
        }),
    }
}

fn parse_deepseek(raw: Value) -> Result<ProviderQuota, FetchFailure> {
    let available = raw
        .get("is_available")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let balances = raw
        .get("balance_infos")
        .and_then(Value::as_array)
        .ok_or_else(parse_failure)?;
    let resources = balances
        .iter()
        .filter_map(|entry| {
            let currency = entry.get("currency")?.as_str()?.to_owned();
            let total = number(entry.get("total_balance")?)?;
            Some(QuotaResource {
                resource_type: "balance",
                key: format!("balance_{}", currency.to_ascii_lowercase()),
                label: format!("{currency} 余额"),
                unit: currency,
                used: None,
                remaining: Some(total),
                limit: None,
                reset_at: None,
            })
        })
        .collect::<Vec<_>>();
    if resources.is_empty() && available {
        return Err(parse_failure());
    }
    Ok(ProviderQuota {
        resources,
        status_override: (!available).then_some("exhausted"),
        raw,
    })
}

fn parse_kimi(raw: Value) -> Result<ProviderQuota, FetchFailure> {
    let mut resources = Vec::new();

    if let Some(limits) = raw.get("limits").and_then(Value::as_array) {
        for limit in limits {
            if limit.get("window").and_then(kimi_window_minutes) != Some(300) {
                continue;
            }
            if let Some(resource) = limit
                .get("detail")
                .and_then(|detail| kimi_resource(detail, "5h", "5 小时"))
            {
                resources.push(resource);
                break;
            }
        }
    }

    if let Some(resource) = raw
        .get("usage")
        .and_then(|detail| kimi_resource(detail, "7d", "7 天"))
    {
        resources.push(resource);
    }

    if resources.is_empty() {
        return Ok(ProviderQuota {
            resources,
            status_override: Some("unsupported"),
            raw,
        });
    }
    Ok(ProviderQuota {
        resources,
        status_override: None,
        raw,
    })
}

fn kimi_window_minutes(window: &Value) -> Option<i64> {
    let duration = window.get("duration").and_then(integer)?;
    if duration <= 0 {
        return None;
    }
    match window.get("timeUnit").and_then(Value::as_str)? {
        "TIME_UNIT_MINUTE" => Some(duration),
        "TIME_UNIT_HOUR" => duration.checked_mul(60),
        "TIME_UNIT_DAY" => duration.checked_mul(24 * 60),
        "TIME_UNIT_SECOND" if duration % 60 == 0 => Some(duration / 60),
        _ => None,
    }
}

fn kimi_resource(detail: &Value, key: &str, label: &str) -> Option<QuotaResource> {
    let limit = detail.get("limit").and_then(number)?;
    if limit <= 0.0 {
        return None;
    }
    let raw_used = detail.get("used").and_then(number);
    let raw_remaining = detail.get("remaining").and_then(number);
    let used = raw_used.or_else(|| raw_remaining.map(|remaining| limit - remaining))?;
    let remaining = raw_remaining.or_else(|| raw_used.map(|used| limit - used))?;
    let used_percent = (used / limit * 100.0).clamp(0.0, 100.0);
    let remaining_percent = (remaining / limit * 100.0).clamp(0.0, 100.0);
    Some(QuotaResource {
        resource_type: "window",
        key: key.into(),
        label: label.into(),
        unit: "percent".into(),
        used: Some(used_percent),
        remaining: Some(remaining_percent),
        limit: Some(100.0),
        reset_at: detail
            .get("resetTime")
            .or_else(|| detail.get("reset_time"))
            .and_then(parse_reset_time),
    })
}

fn parse_minimax(raw: Value, now: DateTime<Utc>) -> Result<ProviderQuota, FetchFailure> {
    let status_code = raw
        .get("status_code")
        .and_then(integer)
        .or_else(|| raw.get("base_resp")?.get("status_code").and_then(integer))
        .unwrap_or(0);
    if status_code != 0 {
        if status_code == 2062 {
            return Ok(ProviderQuota {
                resources: Vec::new(),
                status_override: Some("unsupported"),
                raw,
            });
        }
        return Err(FetchFailure {
            status: "refresh_failed",
            code: "minimax_quota_error",
            message: "MiniMax quota API returned an application error",
            http_status: None,
        });
    }
    let items = raw
        .get("model_remains")
        .and_then(Value::as_array)
        .or_else(|| raw.get("data")?.get("model_remains")?.as_array())
        .ok_or_else(parse_failure)?;
    let selected = items
        .iter()
        .find(|item| item.get("model_name").and_then(Value::as_str) == Some("general"))
        .or_else(|| items.first())
        .ok_or_else(parse_failure)?;

    let mut resources = Vec::new();
    if let Some(remaining) = minimax_remaining(selected, false) {
        resources.push(QuotaResource {
            resource_type: "window",
            key: "5h".into(),
            label: "5 小时".into(),
            unit: "percent".into(),
            used: Some((100.0 - remaining).clamp(0.0, 100.0)),
            remaining: Some(remaining),
            limit: Some(100.0),
            reset_at: minimax_reset(selected, false, now),
        });
    }
    if let Some(remaining) = minimax_remaining(selected, true) {
        resources.push(QuotaResource {
            resource_type: "window",
            key: "7d".into(),
            label: "7 天".into(),
            unit: "percent".into(),
            used: Some((100.0 - remaining).clamp(0.0, 100.0)),
            remaining: Some(remaining),
            limit: Some(100.0),
            reset_at: minimax_reset(selected, true, now),
        });
    }
    if resources.is_empty() {
        return Err(parse_failure());
    }
    Ok(ProviderQuota {
        resources,
        status_override: None,
        raw,
    })
}

fn minimax_remaining(value: &Value, weekly: bool) -> Option<f64> {
    let prefix = if weekly {
        "current_weekly"
    } else {
        "current_interval"
    };
    if let Some(percent) = value
        .get(format!("{prefix}_remaining_percent"))
        .and_then(number)
    {
        return Some(percent.clamp(0.0, 100.0));
    }
    let total = value
        .get(format!("{prefix}_total_count"))
        .and_then(number)?;
    if total <= 0.0 {
        return None;
    }
    let used = value
        .get(format!("{prefix}_usage_count"))
        .or_else(|| value.get(format!("{prefix}_used_count")))
        .and_then(number)
        .unwrap_or(0.0);
    Some((100.0 - used / total * 100.0).clamp(0.0, 100.0))
}

fn minimax_reset(value: &Value, weekly: bool, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let end_key = if weekly {
        "weekly_end_time"
    } else {
        "end_time"
    };
    if let Some(end) = value.get(end_key).and_then(parse_absolute_time) {
        return Some(end);
    }

    let remains_key = if weekly {
        "weekly_remains_time"
    } else {
        "remains_time"
    };
    let millis = value.get(remains_key).and_then(integer)?.max(0);
    now.checked_add_signed(ChronoDuration::milliseconds(millis))
}

fn parse_failure() -> FetchFailure {
    FetchFailure {
        status: "refresh_failed",
        code: "invalid_quota_response",
        message: "upstream quota response does not match the expected schema",
        http_status: None,
    }
}

fn quota_status(resources: &[QuotaResource]) -> &'static str {
    let remaining = resources
        .iter()
        .filter(|resource| resource.resource_type == "window" && resource.unit == "percent")
        .filter_map(|resource| resource.remaining)
        .reduce(f64::min);
    match remaining {
        Some(value) if value <= 0.0 => "exhausted",
        Some(value) if value < 20.0 => "low",
        _ => "ok",
    }
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn integer(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.parse::<i64>().ok())
}

fn parse_reset_time(value: &Value) -> Option<DateTime<Utc>> {
    let value = value.as_str()?;
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn parse_absolute_time(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(text) = value.as_str() {
        if let Ok(timestamp) = DateTime::parse_from_rfc3339(text) {
            return Some(timestamp.with_timezone(&Utc));
        }
    }
    let raw = integer(value)?;
    if raw > 10_000_000_000 {
        DateTime::<Utc>::from_timestamp_millis(raw)
    } else if raw > 1_000_000_000 {
        DateTime::<Utc>::from_timestamp(raw, 0)
    } else {
        None
    }
}

fn elapsed_ms(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_deepseek_balance_resources() {
        let parsed = parse_deepseek(json!({
            "is_available": true,
            "balance_infos": [{
                "currency": "CNY",
                "total_balance": "82.31",
                "granted_balance": "22.31",
                "topped_up_balance": "60.00"
            }]
        }))
        .unwrap();
        assert_eq!(parsed.resources.len(), 1);
        assert_eq!(parsed.resources[0].key, "balance_cny");
        assert_eq!(parsed.resources[0].remaining, Some(82.31));
        assert_eq!(parsed.status_override, None);
    }

    #[test]
    fn parses_current_kimi_usage_and_limits_shape() {
        let parsed = parse_kimi(json!({
            "usage": {
                "limit": "100",
                "used": "59",
                "remaining": "41",
                "resetTime": "2026-09-19T12:24:00Z"
            },
            "limits": [{
                "window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
                "detail": {
                    "limit": "100",
                    "used": "27",
                    "remaining": "73",
                    "resetTime": "2026-09-16T12:24:00Z"
                }
            }]
        }))
        .unwrap();
        assert_eq!(parsed.resources.len(), 2);
        assert_eq!(parsed.resources[0].key, "5h");
        assert_eq!(parsed.resources[0].remaining, Some(73.0));
        assert_eq!(parsed.resources[1].key, "7d");
        assert_eq!(parsed.resources[1].remaining, Some(41.0));
        assert_eq!(
            parsed.resources[0].reset_at,
            DateTime::parse_from_rfc3339("2026-09-16T12:24:00Z")
                .ok()
                .map(|value| value.with_timezone(&Utc))
        );
    }

    #[test]
    fn parses_minimax_end_time_as_millisecond_epoch() {
        let now = DateTime::parse_from_rfc3339("2026-09-16T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let parsed = parse_minimax(
            json!({
                "status_code": 0,
                "model_remains": [{
                    "model_name": "general",
                    "current_interval_remaining_percent": 62,
                    "current_weekly_remaining_percent": 78,
                    "end_time": 1789567800000_i64,
                    "weekly_end_time": 1789898400000_i64,
                    "remains_time": 14998196,
                    "weekly_remains_time": 345600000
                }]
            }),
            now,
        )
        .unwrap();
        assert_eq!(parsed.resources[0].remaining, Some(62.0));
        assert_eq!(parsed.resources[1].remaining, Some(78.0));
        assert_eq!(
            parsed.resources[0].reset_at,
            DateTime::<Utc>::from_timestamp_millis(1789567800000)
        );
        assert_eq!(
            parsed.resources[1].reset_at,
            DateTime::<Utc>::from_timestamp_millis(1789898400000)
        );
    }

    #[test]
    fn parses_minimax_remains_time_as_milliseconds() {
        let now = DateTime::parse_from_rfc3339("2026-09-16T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let parsed = parse_minimax(
            json!({
                "status_code": 0,
                "model_remains": [{
                    "model_name": "general",
                    "current_interval_remaining_percent": 62,
                    "current_weekly_remaining_percent": 78,
                    "remains_time": 14998196,
                    "weekly_remains_time": 345600000
                }]
            }),
            now,
        )
        .unwrap();
        assert_eq!(
            parsed.resources[0].reset_at,
            now.checked_add_signed(ChronoDuration::milliseconds(14_998_196))
        );
        assert_eq!(
            parsed.resources[1].reset_at,
            now.checked_add_signed(ChronoDuration::milliseconds(345_600_000))
        );
    }

    #[test]
    fn list_projection_omits_raw_provider_payload() {
        let snapshot = UpstreamQuotaSnapshot {
            account: QuotaAccountView {
                account_id: "a".into(),
                account_display_name: "A".into(),
                source_id: "s".into(),
                source_display_name: "S".into(),
                provider_id: "kimi_code".into(),
                enabled: true,
            },
            status: "ok",
            resources: Vec::new(),
            fetched_at: Some(Utc::now()),
            attempted_at: Utc::now(),
            latency_ms: 1,
            stale: false,
            refresh_error: None,
            raw: None,
        };
        let serialized = serde_json::to_value(snapshot).unwrap();
        assert!(serialized.get("raw").is_none());
    }

    #[test]
    fn response_limit_rejects_chunk_before_appending_past_one_mib() {
        let mut body = vec![0_u8; MAX_QUOTA_RESPONSE_BYTES - 1];
        let error = append_quota_chunk(&mut body, &[1, 2], StatusCode::OK).unwrap_err();
        assert_eq!(error.code, "quota_response_too_large");
        assert_eq!(body.len(), MAX_QUOTA_RESPONSE_BYTES - 1);
    }

    #[test]
    fn low_status_uses_lowest_window() {
        let resources = vec![QuotaResource {
            resource_type: "window",
            key: "5h".into(),
            label: "5 小时".into(),
            unit: "percent".into(),
            used: Some(92.0),
            remaining: Some(8.0),
            limit: Some(100.0),
            reset_at: None,
        }];
        assert_eq!(quota_status(&resources), "low");
    }
}
