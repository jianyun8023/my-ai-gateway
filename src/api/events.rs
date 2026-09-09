use axum::{
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::collections::HashMap;

use crate::{
    http::response::error_response,
    infra::events::{EventCursor, EventFilter},
    state::AppState,
};

const CATEGORIES: [&str; 9] = [
    "lifecycle",
    "configuration",
    "database",
    "security",
    "request",
    "health",
    "operation",
    "admin",
    "discovery",
];
const LEVELS: [&str; 3] = ["info", "warning", "error"];
const SOURCES: [&str; 5] = [
    "system_events",
    "usage_events",
    "account_health_events",
    "audit_logs",
    "source_discovery_runs",
];

#[derive(Debug)]
pub(crate) struct EventQuery {
    pub(crate) filter: EventFilter,
    pub(crate) limit: i64,
    pub(crate) cursor: Option<EventCursor>,
}

pub(crate) fn parse_event_query(query: &HashMap<String, String>) -> Result<EventQuery, String> {
    let from = parse_time(query, "from")?;
    let since = parse_time(query, "since")?;
    let to = parse_time(query, "to")?;
    if from.is_some() && since.is_some() {
        return Err("from and since cannot be combined".into());
    }
    let lower = from.or(since);
    if lower.zip(to).is_some_and(|(lower, to)| lower >= to) {
        return Err("from/since must be earlier than to".into());
    }

    let category = enum_filter(query, "category", &CATEGORIES)?;
    let level = enum_filter(query, "level", &LEVELS)?;
    let source = enum_filter(query, "source", &SOURCES)?;
    let event_type = text_filter(query, "event_type", 128)?;
    let subject_type = text_filter(query, "subject_type", 64)?;
    let subject_id = text_filter(query, "subject_id", 256)?;
    let correlation_id = match (
        text_filter(query, "correlation_id", 256)?,
        text_filter(query, "operation_id", 256)?,
    ) {
        (Some(correlation), Some(operation)) if correlation != operation => {
            return Err("correlation_id and operation_id must match when both are provided".into())
        }
        (Some(value), _) | (_, Some(value)) => Some(value),
        (None, None) => None,
    };
    let limit = query
        .get("limit")
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|value| (1..=500).contains(value))
                .ok_or_else(|| "limit must be between 1 and 500".to_owned())
        })
        .transpose()?
        .unwrap_or(100);
    let cursor = query
        .get("cursor")
        .map(|value| EventCursor::decode(value).ok_or_else(|| "cursor is invalid".to_owned()))
        .transpose()?;

    Ok(EventQuery {
        filter: EventFilter {
            from,
            since,
            to,
            category,
            level,
            event_type,
            subject_type,
            subject_id,
            correlation_id,
            source,
        },
        limit,
        cursor,
    })
}

pub(crate) async fn list_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    if !state.events.is_enabled() {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        );
    }
    let query = match parse_event_query(&query) {
        Ok(query) => query,
        Err(message) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid_event_query", &message)
        }
    };
    match state
        .events
        .list(&query.filter, query.limit, query.cursor.as_ref())
        .await
    {
        Ok(page) => {
            state.events.database_recovered("events.query").await;
            (
                StatusCode::OK,
                Json(json!({
                    "version": "v1",
                    "timezone": "UTC",
                    "fact_source": "postgresql_unified_read_model",
                    "range": {
                        "from": query.filter.from.map(|value| value.to_rfc3339()),
                        "since": query.filter.since.map(|value| value.to_rfc3339()),
                        "to": query.filter.to.map(|value| value.to_rfc3339()),
                        "boundary": if query.filter.since.is_some() { "(since,to)" } else { "[from,to)" },
                    },
                    "data": page.data,
                    "page": {
                        "limit": query.limit,
                        "has_more": page.has_more,
                        "next_cursor": page.next_cursor,
                    },
                })),
            )
                .into_response()
        }
        Err(error) => {
            state.events.database_failed("events.query", &error).await;
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "events_query_failed",
                "event query failed",
            )
        }
    }
}

fn parse_time(
    query: &HashMap<String, String>,
    name: &str,
) -> Result<Option<DateTime<Utc>>, String> {
    query
        .get(name)
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|_| format!("{name} must be an RFC3339 timestamp"))
        })
        .transpose()
}

fn enum_filter(
    query: &HashMap<String, String>,
    name: &str,
    allowed: &[&str],
) -> Result<Option<String>, String> {
    let value = text_filter(query, name, 64)?;
    if value
        .as_deref()
        .is_some_and(|value| !allowed.contains(&value))
    {
        return Err(format!("unsupported {name}"));
    }
    Ok(value)
}

fn text_filter(
    query: &HashMap<String, String>,
    name: &str,
    max_len: usize,
) -> Result<Option<String>, String> {
    let Some(value) = query.get(name) else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_len || value.chars().any(char::is_control) {
        return Err(format!("{name} must be 1..{max_len} printable characters"));
    }
    Ok(Some(value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_query_supports_operation_polling_and_strict_bounds() {
        let query = HashMap::from([
            ("since".into(), "2026-09-09T00:00:00Z".into()),
            ("operation_id".into(), "cleanup-1".into()),
            ("category".into(), "operation".into()),
            ("limit".into(), "50".into()),
        ]);
        let parsed = parse_event_query(&query).unwrap();
        assert_eq!(parsed.filter.correlation_id.as_deref(), Some("cleanup-1"));
        assert!(parsed.filter.since.is_some());
        assert_eq!(parsed.limit, 50);

        let invalid = HashMap::from([
            ("from".into(), "2026-09-09T00:00:00Z".into()),
            ("since".into(), "2026-09-09T00:00:01Z".into()),
        ]);
        assert_eq!(
            parse_event_query(&invalid).unwrap_err(),
            "from and since cannot be combined"
        );
    }

    #[test]
    fn event_query_rejects_unknown_dimensions_and_bad_cursors() {
        assert!(
            parse_event_query(&HashMap::from([("category".into(), "everything".into())])).is_err()
        );
        assert!(parse_event_query(&HashMap::from([("cursor".into(), "invalid".into())])).is_err());
    }
}
