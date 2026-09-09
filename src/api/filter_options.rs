use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::json;

use crate::{http::response::error_response, infra::db::UsageOptionField, state::AppState};

#[derive(Debug)]
struct OptionsQuery {
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    search: String,
    limit: i64,
}

fn parse_query(query: &HashMap<String, String>) -> Result<OptionsQuery, String> {
    for key in query.keys() {
        if !matches!(key.as_str(), "field" | "from" | "to" | "q" | "limit") {
            return Err(format!("unsupported parameter: {key}"));
        }
    }
    let time = |key: &str| {
        query
            .get(key)
            .map(|value| {
                DateTime::parse_from_rfc3339(value)
                    .map(|time| time.with_timezone(&Utc))
                    .map_err(|_| format!("{key} must be an RFC3339 timestamp"))
            })
            .transpose()
    };
    let from = time("from")?;
    let to = time("to")?;
    if from.zip(to).is_some_and(|(from, to)| from >= to) {
        return Err("from must be earlier than to".into());
    }
    let search = query.get("q").map(String::as_str).unwrap_or("").trim();
    if search.chars().count() > 256 || search.chars().any(char::is_control) {
        return Err("q must contain at most 256 printable characters".into());
    }
    let limit = query
        .get("limit")
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|value| (1..=100).contains(value))
                .ok_or_else(|| "limit must be between 1 and 100".to_owned())
        })
        .transpose()?
        .unwrap_or(50);
    Ok(OptionsQuery {
        from,
        to,
        search: search.to_owned(),
        limit,
    })
}

fn options_response(mut data: Vec<String>, query: &OptionsQuery) -> Response<Body> {
    let has_more = data.len() as i64 > query.limit;
    data.truncate(query.limit as usize);
    Json(json!({
        "data": data,
        "has_more": has_more,
    }))
    .into_response()
}

fn invalid_query(message: &str) -> Response<Body> {
    error_response(
        StatusCode::BAD_REQUEST,
        "invalid_filter_options_query",
        message,
    )
}

pub(crate) async fn usage_options(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response<Body> {
    if !state.admin_auth.authorized(&headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    let Some(database) = &state.db else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        );
    };
    let Some(field) = params
        .get("field")
        .and_then(|value| UsageOptionField::parse(value))
    else {
        return invalid_query("unsupported or missing usage field");
    };
    let query = match parse_query(&params) {
        Ok(query) => query,
        Err(message) => return invalid_query(&message),
    };
    match database
        .usage_filter_options(field, query.from, query.to, &query.search, query.limit)
        .await
    {
        Ok(data) => options_response(data, &query),
        Err(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "filter_options_failed",
            "filter options query failed",
        ),
    }
}

pub(crate) async fn event_options(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
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
    if params.get("field").map(String::as_str) != Some("event_type") {
        return invalid_query("field must be event_type");
    }
    let query = match parse_query(&params) {
        Ok(query) => query,
        Err(message) => return invalid_query(&message),
    };
    match state
        .events
        .event_type_options(query.from, query.to, &query.search, query.limit)
        .await
    {
        Ok(data) => options_response(data, &query),
        Err(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "filter_options_failed",
            "filter options query failed",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_bounds_limits_and_literal_search() {
        let parsed = parse_query(&HashMap::from([("q".into(), "  %_'模型  ".into())])).unwrap();
        assert_eq!(parsed.search, "%_'模型");
        assert_eq!(parsed.limit, 50);
        for (key, value) in [
            ("limit", "0"),
            ("limit", "101"),
            ("limit", "abc"),
            ("from", "bad"),
            ("q", "a\nb"),
            ("cursor", "abc"),
        ] {
            assert!(parse_query(&HashMap::from([(key.into(), value.into())])).is_err());
        }
        assert!(parse_query(&HashMap::from([("q".into(), "a".repeat(257))])).is_err());
        assert!(parse_query(&HashMap::from([
            ("from".into(), "2026-09-09T00:00:00Z".into()),
            ("to".into(), "2026-09-09T00:00:00Z".into()),
        ]))
        .is_err());
        assert!(UsageOptionField::parse("credential_env").is_none());
        assert!(UsageOptionField::parse("provider_id; DROP TABLE usage_events").is_none());
    }
}
