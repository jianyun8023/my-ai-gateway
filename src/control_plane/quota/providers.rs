use super::types::{FetchFailure, ProviderQuota, QuotaResource};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::Value;

pub(super) fn parse_provider_quota(
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

pub(super) fn quota_status(resources: &[QuotaResource]) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
