use std::fmt::Write as _;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        HeaderMap, Response, StatusCode,
    },
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};

use crate::infra::db;

use crate::{http::response::error_response, state::AppState};

#[derive(Debug)]
pub(crate) struct UsageQuery {
    pub(crate) filter: db::UsageFilter,
    pub(crate) granularity: String,
    pub(crate) breakdown: String,
    pub(crate) limit: i64,
    pub(crate) cursor: Option<db::UsageCursor>,
    pub(crate) format: String,
}

pub(crate) fn parse_usage_query(
    query: &std::collections::HashMap<String, String>,
) -> Result<UsageQuery, String> {
    if query.contains_key("source") {
        return Err("source is no longer supported; use source_id or client_source".into());
    }
    let parse_time = |name: &str| -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
        query
            .get(name)
            .map(|value| {
                chrono::DateTime::parse_from_rfc3339(value)
                    .map(|time| time.with_timezone(&chrono::Utc))
                    .map_err(|_| format!("{name} must be an RFC3339 timestamp"))
            })
            .transpose()
    };
    let from = parse_time("from")?;
    let to = parse_time("to")?;
    if from.zip(to).is_some_and(|(from, to)| from >= to) {
        return Err("from must be earlier than to".into());
    }
    let virtual_key_id = query
        .get("virtual_key")
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| "virtual_key must be a positive integer".to_string())
        })
        .transpose()?;
    let status_code = query
        .get("status_code")
        .map(|value| {
            value
                .parse::<i32>()
                .ok()
                .filter(|code| (100..=599).contains(code))
                .ok_or_else(|| "status_code must be between 100 and 599".to_string())
        })
        .transpose()?;
    let success = match query.get("status").map(String::as_str) {
        None => None,
        Some("success") => Some(true),
        Some("failure") => Some(false),
        Some(_) => return Err("status must be success or failure".into()),
    };
    if let Some(value) = query.get("usage_source") {
        if !matches!(
            value.as_str(),
            "upstream" | "parsed" | "estimated" | "missing"
        ) {
            return Err("usage_source must be upstream, parsed, estimated, or missing".into());
        }
    }
    let granularity = query
        .get("granularity")
        .cloned()
        .unwrap_or_else(|| "hour".into());
    if !matches!(granularity.as_str(), "hour" | "day") {
        return Err("granularity must be hour or day".into());
    }
    let breakdown = query
        .get("breakdown")
        .cloned()
        .unwrap_or_else(|| "logical_model".into());
    if !matches!(
        breakdown.as_str(),
        "logical_model"
            | "upstream_model"
            | "provider"
            | "source_id"
            | "client_source"
            | "account"
            | "protocol_in"
            | "protocol_upstream"
            | "virtual_key"
            | "status"
            | "usage_source"
    ) {
        return Err("unsupported breakdown dimension".into());
    }
    let limit = query
        .get("limit")
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|limit| (1..=500).contains(limit))
                .ok_or_else(|| "limit must be between 1 and 500".to_string())
        })
        .transpose()?
        .unwrap_or(100);
    let cursor = query
        .get("cursor")
        .map(|value| db::UsageCursor::decode(value).ok_or_else(|| "cursor is invalid".to_string()))
        .transpose()?;
    let format = query
        .get("format")
        .cloned()
        .unwrap_or_else(|| "json".into());
    if !matches!(format.as_str(), "json" | "csv") {
        return Err("format must be json or csv".into());
    }
    Ok(UsageQuery {
        filter: db::UsageFilter {
            from,
            to,
            logical_model: query.get("logical_model").cloned(),
            upstream_model_id: query.get("upstream_model").cloned(),
            provider_id: query.get("provider").cloned(),
            source_id: query.get("source_id").cloned(),
            client_source: query.get("client_source").cloned(),
            account_id: query.get("account").cloned(),
            protocol_in: query.get("protocol_in").cloned(),
            protocol_upstream: query.get("protocol_upstream").cloned(),
            virtual_key_id,
            success,
            status_code,
            usage_source: query.get("usage_source").cloned(),
        },
        granularity,
        breakdown,
        limit,
        cursor,
        format,
    })
}

fn usage_range(filter: &db::UsageFilter) -> Value {
    json!({
        "from": filter.from.as_ref().map(chrono::DateTime::to_rfc3339),
        "to": filter.to.as_ref().map(chrono::DateTime::to_rfc3339),
        "boundary": "[from,to)"
    })
}

fn invalid_usage_query(message: &str) -> Response<Body> {
    error_response(StatusCode::BAD_REQUEST, "invalid_usage_query", message)
}

pub(crate) async fn usage_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    match database.usage_aggregate(&query.filter).await {
        Ok(data) => (StatusCode::OK, Json(json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"data":data}))).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "usage_summary_failed", &error.to_string()),
    }
}

pub(crate) async fn usage_timeseries(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    match database.usage_timeseries(&query.filter, &query.granularity).await {
        Ok(data) => (StatusCode::OK, Json(json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"granularity":query.granularity,"data":data}))).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "usage_timeseries_failed", &error.to_string()),
    }
}

pub(crate) async fn usage_breakdown(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    match database.usage_breakdown(&query.filter, &query.breakdown).await {
        Ok(data) => (StatusCode::OK, Json(json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"dimension":query.breakdown,"data":data}))).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "usage_breakdown_failed", &error.to_string()),
    }
}

pub(crate) async fn usage_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    match database
        .list_usage_events_page(&query.filter, query.limit, query.cursor.as_ref())
        .await
    {
        Ok(page) => (StatusCode::OK, Json(json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"data":page.data,"page":{"limit":query.limit,"has_more":page.has_more,"next_cursor":page.next_cursor}}))).into_response(),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "usage_events_failed",
            &error.to_string(),
        ),
    }
}

pub(crate) async fn usage_aggregate(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    let aggregate = match database.usage_aggregate(&query.filter).await {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_aggregate_failed",
                &error.to_string(),
            )
        }
    };
    let timeseries = match database
        .usage_timeseries(&query.filter, &query.granularity)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_timeseries_failed",
                &error.to_string(),
            )
        }
    };
    let breakdown = match database
        .usage_breakdown(&query.filter, &query.breakdown)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_breakdown_failed",
                &error.to_string(),
            )
        }
    };
    (StatusCode::OK, Json(json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"granularity":query.granularity,"breakdown_dimension":query.breakdown,"aggregate":aggregate,"timeseries":timeseries,"breakdown":breakdown}))).into_response()
}

pub(crate) async fn usage_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
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
    let query = match parse_usage_query(&query) {
        Ok(query) => query,
        Err(message) => return invalid_usage_query(&message),
    };
    let export_limit: i64 = 10_000;
    let events = match database
        .export_usage_events(&query.filter, export_limit + 1)
        .await
    {
        Ok(events) => events,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "usage_export_failed",
                &error.to_string(),
            )
        }
    };
    if events.len() as i64 > export_limit {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "export_too_large",
            &format!("export exceeds {export_limit} rows; narrow the time range or add filters"),
        );
    }
    if query.format == "csv" {
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/csv; charset=utf-8")
            .header(CONTENT_DISPOSITION, "attachment; filename=usage-events.csv")
            .body(Body::from(usage_events_csv(&events)))
            .expect("valid CSV export response")
    } else {
        let payload = json!({"version":"v1","timezone":"UTC","range":usage_range(&query.filter),"data":events});
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/json")
            .header(
                CONTENT_DISPOSITION,
                "attachment; filename=usage-events.json",
            )
            .body(Body::from(
                serde_json::to_vec(&payload).expect("serializable usage export"),
            ))
            .expect("valid JSON export response")
    }
}

pub(crate) fn usage_events_csv(events: &[db::UsageEventRecord]) -> String {
    let mut output = String::from("request_id,created_at,virtual_key_id,logical_model,upstream_model_id,provider_id,source_id,client_source,account_id,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,ttft_ms,input_tokens,output_tokens,reasoning_tokens,cached_tokens,cache_read_tokens,cache_creation_tokens,total_tokens,usage_source,degraded,route_id,streamed,error_summary,fallback_reason\n");
    for event in events {
        let values = [
            event.request_id.clone(),
            event.created_at.to_rfc3339(),
            event
                .virtual_key_id
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event.logical_model.clone(),
            event.upstream_model_id.clone().unwrap_or_default(),
            event.provider_id.clone(),
            event.source_id.clone().unwrap_or_default(),
            event.client_source.clone(),
            event.account_id.clone(),
            event.protocol_in.clone(),
            event.protocol_upstream.clone(),
            event.mode.clone(),
            event.status_code.to_string(),
            event.success.to_string(),
            event.retry_count.to_string(),
            event.latency_ms.to_string(),
            event
                .ttft_ms
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event.input_tokens.to_string(),
            event.output_tokens.to_string(),
            event.reasoning_tokens.to_string(),
            event.cached_tokens.to_string(),
            event.cache_read_tokens.to_string(),
            event.cache_creation_tokens.to_string(),
            event.total_tokens.to_string(),
            event.usage_source.clone(),
            event.degraded.to_string(),
            event.route_id.clone().unwrap_or_default(),
            event.streamed.to_string(),
            event.error_summary.clone().unwrap_or_default(),
            event.fallback_reason.clone().unwrap_or_default(),
        ];
        let line = values
            .iter()
            .map(|value| csv_field(value))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(output, "{line}").expect("writing to String cannot fail");
    }
    output
}

pub(crate) fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub(crate) async fn usage_event_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
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
    let event = match database.get_usage_event_detail(&request_id).await {
        Ok(Some(event)) => event,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "event_not_found",
                "usage event not found",
            )
        }
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "event_detail_failed",
                &error.to_string(),
            )
        }
    };
    let attempts = match database.list_attempts_for_event(&request_id).await {
        Ok(attempts) => attempts,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "event_detail_failed",
                &error.to_string(),
            )
        }
    };
    (
        StatusCode::OK,
        Json(json!({"version":"v1","data":event,"attempts":attempts})),
    )
        .into_response()
}
