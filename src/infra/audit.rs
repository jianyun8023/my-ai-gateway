//! Common, metadata-only audit support for Admin write operations.
//!
//! The control plane runs its successful audit insert in the same PostgreSQL
//! transaction as the mutation. The HTTP middleware records failures after a
//! rolled-back transaction, so operators can still explain conflicts and
//! validation failures without weakening mutation atomicity.

use axum::http::{HeaderMap, Method};
use serde_json::{json, Map, Value};
use sqlx::{PgPool, Postgres, Transaction};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use uuid::Uuid;

const MAX_ACTOR_LEN: usize = 128;
const MAX_REQUEST_ID_LEN: usize = 128;
const MAX_RESOURCE_ID_LEN: usize = 256;
const MAX_DIFF_FIELDS: usize = 128;

const SENSITIVE_MARKERS: [&str; 14] = [
    "authorization",
    "api_key",
    "apikey",
    "credential",
    "ciphertext",
    "secret",
    "password",
    "token",
    "key_hash",
    "prompt",
    "response",
    "signature",
    "thinking",
    "body",
];

/// Request-scoped context shared with database mutation helpers through a
/// Tokio task-local. The recorded flag lets the HTTP middleware avoid emitting
/// a duplicate success event when a control-plane transaction already wrote it.
#[derive(Clone, Debug)]
pub struct AuditContext {
    pub request_id: String,
    pub actor: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub resource: Option<String>,
    pub diff: Value,
    recorded: Arc<AtomicBool>,
}

impl AuditContext {
    pub fn new(
        request_id: impl Into<String>,
        actor: impl Into<String>,
        action: impl Into<String>,
        resource_type: impl Into<String>,
        resource_id: Option<String>,
        diff: Value,
    ) -> Self {
        let request_id = sanitize_request_id(&request_id.into());
        let actor = sanitize_actor(&actor.into());
        let action = sanitize_label(&action.into(), "admin.write");
        let resource_type = sanitize_label(&resource_type.into(), "admin");
        let resource_id = resource_id.map(|value| sanitize_resource_id(&value));
        let resource = resource_id
            .as_ref()
            .map(|id| format!("{resource_type}/{id}"))
            .or_else(|| Some(resource_type.clone()));
        Self {
            request_id,
            actor,
            action,
            resource_type,
            resource_id,
            resource,
            diff: sanitize_diff(diff),
            recorded: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn was_recorded(&self) -> bool {
        self.recorded.load(Ordering::Acquire)
    }

    fn mark_recorded(&self) {
        self.recorded.store(true, Ordering::Release);
    }
}

tokio::task_local! {
    static CURRENT_CONTEXT: AuditContext;
}

/// Run a request inside its audit context. Calls made outside an HTTP request
/// (startup imports, tests, background jobs) intentionally have no implicit
/// actor and therefore do not create Admin-write events.
pub async fn scope<F>(context: AuditContext, future: F) -> F::Output
where
    F: Future,
{
    CURRENT_CONTEXT.scope(context, future).await
}

pub fn current_context() -> Option<AuditContext> {
    CURRENT_CONTEXT.try_with(Clone::clone).ok()
}

/// Insert a successful event in the caller's open control-plane transaction.
/// The boolean indicates whether a request context existed.
#[allow(dead_code)] // consumed by control-plane mutation helpers once wired
pub async fn append_current_success_tx(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<bool, sqlx::Error> {
    let Some(context) = current_context() else {
        return Ok(false);
    };
    append_tx(tx, &context, "success", None, None).await?;
    context.mark_recorded();
    Ok(true)
}

/// Insert a successful event in an independent statement when the operation
/// did not expose a transaction hook (for example a legacy provider helper).
pub async fn append_current_success_pool(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let Some(context) = current_context() else {
        return Ok(false);
    };
    append_pool(pool, &context, "success", None, None).await?;
    context.mark_recorded();
    Ok(true)
}

/// Insert a failure after the mutation transaction has rolled back. This is
/// deliberately independent: a failed audit insert must never turn a failed
/// request into a partially committed control-plane write.
pub async fn append_current_failure_pool(
    pool: &PgPool,
    status: &str,
    error_code: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let Some(context) = current_context() else {
        return Ok(false);
    };
    append_pool(pool, &context, result_for_status(status), error_code, None).await?;
    context.mark_recorded();
    Ok(true)
}

#[allow(dead_code)] // consumed by control-plane mutation helpers once wired
pub async fn append_tx(
    tx: &mut Transaction<'_, Postgres>,
    context: &AuditContext,
    result: &str,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> Result<(), sqlx::Error> {
    let (status, result) = normalize_result(result);
    let details = details_for(context, result);
    sqlx::query(
        "INSERT INTO audit_logs (operation_id,action,status,actor,details,error_code,error_message,request_id,resource_type,resource_id,resource,result,diff,completed_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$1,$8,$9,$10,$11,$12,CASE WHEN $3 IN ('succeeded','failed','cancelled') THEN clock_timestamp() ELSE NULL END)",
    )
    .bind(&context.request_id)
    .bind(&context.action)
    .bind(status)
    .bind(&context.actor)
    .bind(details)
    .bind(sanitize_error_code(error_code))
    .bind(sanitize_error_message(error_message))
    .bind(&context.resource_type)
    .bind(&context.resource_id)
    .bind(&context.resource)
    .bind(result)
    .bind(&context.diff)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn append_pool(
    pool: &PgPool,
    context: &AuditContext,
    result: &str,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> Result<(), sqlx::Error> {
    let (status, result) = normalize_result(result);
    let details = details_for(context, result);
    sqlx::query(
        "INSERT INTO audit_logs (operation_id,action,status,actor,details,error_code,error_message,request_id,resource_type,resource_id,resource,result,diff,completed_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$1,$8,$9,$10,$11,$12,CASE WHEN $3 IN ('succeeded','failed','cancelled') THEN clock_timestamp() ELSE NULL END)",
    )
    .bind(&context.request_id)
    .bind(&context.action)
    .bind(status)
    .bind(&context.actor)
    .bind(details)
    .bind(sanitize_error_code(error_code))
    .bind(sanitize_error_message(error_message))
    .bind(&context.resource_type)
    .bind(&context.resource_id)
    .bind(&context.resource)
    .bind(result)
    .bind(&context.diff)
    .execute(pool)
    .await?;
    Ok(())
}

fn details_for(context: &AuditContext, result: &str) -> Value {
    json!({
        "resource": context.resource,
        "result": result,
        "diff": context.diff,
    })
}

fn normalize_result(value: &str) -> (&'static str, &'static str) {
    match value {
        "success" | "succeeded" => ("succeeded", "success"),
        "conflict" => ("failed", "conflict"),
        "rollback" | "rolled_back" => ("failed", "rollback"),
        _ => ("failed", "failure"),
    }
}

fn result_for_status(status: &str) -> &'static str {
    match status {
        "409" | "conflict" => "conflict",
        "422" | "rollback" => "rollback",
        _ => "failure",
    }
}

fn sanitize_error_code(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Some("admin_operation_failed".into())
    } else {
        Some(value.to_owned())
    }
}

fn sanitize_error_message(value: Option<&str>) -> Option<String> {
    value.map(|_| "admin operation failed".to_owned())
}

pub fn sanitize_actor(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "admin_api".into();
    }
    let sanitized = value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_ACTOR_LEN)
        .collect::<String>()
        .trim()
        .to_owned();
    if sanitized.is_empty() {
        "admin_api".into()
    } else {
        sanitized
    }
}

fn sanitize_request_id(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return Uuid::new_v4().to_string();
    }
    let sanitized = value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_REQUEST_ID_LEN)
        .collect::<String>();
    if sanitized.trim().is_empty() {
        Uuid::new_v4().to_string()
    } else {
        sanitized
    }
}

fn sanitize_resource_id(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_RESOURCE_ID_LEN)
        .collect::<String>()
}

fn sanitize_label(value: &str, fallback: &str) -> String {
    let value = value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_RESOURCE_ID_LEN)
        .collect::<String>();
    if value.trim().is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}

/// Build a request context from an Admin HTTP request. Only field names and
/// JSON types are retained in diff; scalar values are intentionally omitted.
pub fn context_from_request(
    method: &Method,
    path: &str,
    headers: &HeaderMap,
    payload: Option<&Value>,
) -> AuditContext {
    let (action, resource_type, path_resource_id) = classify_path(method, path, payload);
    let actor = payload
        .and_then(|value| value.get("requested_by").and_then(Value::as_str))
        .or_else(|| payload.and_then(|value| value.get("actor").and_then(Value::as_str)))
        .or_else(|| header_text(headers, "x-admin-actor"))
        .or_else(|| header_text(headers, "x-actor"))
        .or_else(|| header_text(headers, "x-requested-by"))
        .unwrap_or("admin_api");
    let request_id = header_text(headers, "x-request-id")
        .or_else(|| payload.and_then(|value| value.get("request_id").and_then(Value::as_str)))
        .or_else(|| payload.and_then(|value| value.get("operation_id").and_then(Value::as_str)))
        .unwrap_or("");
    let resource_id =
        path_resource_id.or_else(|| resource_id_from_payload(&resource_type, payload));
    AuditContext::new(
        request_id,
        actor,
        action,
        resource_type,
        resource_id,
        payload.map_or_else(|| json!({}), summarize_diff),
    )
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn resource_id_from_payload(resource_type: &str, payload: Option<&Value>) -> Option<String> {
    let payload = payload?;
    let key = match resource_type {
        "source_model" => "upstream_model_id",
        "virtual_key" => "id",
        _ => "id",
    };
    payload.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn classify_path(
    method: &Method,
    path: &str,
    payload: Option<&Value>,
) -> (String, String, Option<String>) {
    let segments = path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let admin_index = segments.iter().position(|segment| *segment == "admin");
    let parts = admin_index
        .map(|index| &segments[index + 1..])
        .unwrap_or(&segments);
    let first = parts.first().copied().unwrap_or("admin");
    let (resource_type, resource_id, suffix) = match first {
        "sources" => nested_resource(parts, "source"),
        "accounts" => nested_resource(parts, "account"),
        "logical-models" => nested_resource(parts, "logical_model"),
        "model-bindings" => nested_resource(parts, "model_binding"),
        "routes" => nested_resource(parts, "route"),
        "keys" => nested_resource(parts, "virtual_key"),
        "control-plane" => (
            "control_plane",
            parts.get(1).copied(),
            parts.get(1).copied(),
        ),
        "backup" => ("backup", parts.get(1).copied(), parts.get(1).copied()),
        "retention" => ("retention", parts.get(2).copied(), parts.get(1).copied()),
        "config" => ("config", None, parts.get(1).copied()),
        "health" => ("health", parts.get(1).copied(), parts.get(2).copied()),
        _ => (first, parts.get(1).copied(), parts.get(2).copied()),
    };
    let resource_id = resource_id.map(str::to_owned);
    let action = match (resource_type, suffix, method) {
        ("source", Some("enabled"), _) => enabled_action("source", payload),
        ("account", Some("enabled"), _) => enabled_action("account", payload),
        ("logical_model", Some("enabled"), _) => enabled_action("logical_model", payload),
        ("model_binding", Some("enabled"), _) => enabled_action("model_binding", payload),
        ("route", Some("enabled"), _) => enabled_action("route", payload),
        ("virtual_key", Some("revoke"), _) => "virtual_key.revoke".to_owned(),
        ("virtual_key", Some("rotate"), _) => "virtual_key.rotate".to_owned(),
        ("source", Some("discoveries"), _) => "source.discovery".to_owned(),
        ("source", Some("connection-tests"), _) => "source.connection_test".to_owned(),
        ("source", Some("models"), &Method::PATCH) => "source_model.update".to_owned(),
        ("source", Some("confirm"), _) => "source_model.confirm".to_owned(),
        ("account", Some("probe"), _) | ("health", Some("probe"), _) => "account.probe".to_owned(),
        ("control_plane", Some("import"), _) => "control_plane.import".to_owned(),
        ("backup", Some("import"), _) => "control_plane.import".to_owned(),
        ("config", Some("reload"), _) => "config.reload".to_owned(),
        ("retention", Some("policies"), _) if method == Method::PUT => {
            "retention.policy.update".to_owned()
        }
        ("retention", Some("cleanup"), &Method::POST)
        | ("retention", Some("runs"), &Method::POST) => "retention.cleanup".to_owned(),
        (resource, _, &Method::POST) => format!("{resource}.create"),
        (resource, _, &Method::PUT) | (resource, _, &Method::PATCH) => {
            format!("{resource}.update")
        }
        (resource, _, &Method::DELETE) => format!("{resource}.delete"),
        (resource, _, _) => format!("{resource}.write"),
    };
    (action, resource_type.to_owned(), resource_id)
}

fn nested_resource<'a>(
    parts: &'a [&'a str],
    resource: &'a str,
) -> (&'a str, Option<&'a str>, Option<&'a str>) {
    (resource, parts.get(1).copied(), parts.get(2).copied())
}

fn enabled_action(resource: &str, payload: Option<&Value>) -> String {
    let enabled = payload
        .and_then(|value| value.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    format!("{resource}.{}", if enabled { "enable" } else { "disable" })
}

/// Summarize a JSON request without retaining scalar values. Sensitive field
/// names are listed separately, which is useful for audit review while keeping
/// credentials, tokens and prompt/response content out of the row.
pub fn summarize_diff(value: &Value) -> Value {
    let mut fields = Map::new();
    let mut sensitive = Vec::<String>::new();
    collect_fields(value, "", &mut fields, &mut sensitive);
    if fields.len() > MAX_DIFF_FIELDS {
        fields.clear();
        fields.insert("_truncated".into(), Value::String("true".into()));
    }
    sensitive.sort();
    sensitive.dedup();
    sensitive.truncate(MAX_DIFF_FIELDS);
    json!({
        "changed_fields": fields.keys().cloned().collect::<Vec<_>>(),
        "field_types": fields,
        "sensitive_fields": sensitive,
    })
}

fn collect_fields(
    value: &Value,
    prefix: &str,
    fields: &mut Map<String, Value>,
    sensitive: &mut Vec<String>,
) {
    let Value::Object(object) = value else {
        if !prefix.is_empty() {
            fields.insert(prefix.to_owned(), Value::String(json_type(value).into()));
        }
        return;
    };
    for (key, value) in object {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if is_sensitive_key(key) {
            sensitive.push(path);
            continue;
        }
        if value.is_object() {
            collect_fields(value, &path, fields, sensitive);
        } else {
            fields.insert(path, Value::String(json_type(value).into()));
        }
        if fields.len() >= MAX_DIFF_FIELDS {
            break;
        }
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SENSITIVE_MARKERS.iter().any(|marker| key.contains(marker))
}

/// Recursively redact arbitrary JSON used by legacy operational audit calls.
pub fn sanitize_diff(value: Value) -> Value {
    sanitize_value(value, None)
}

fn sanitize_value(value: Value, key: Option<&str>) -> Value {
    if key.is_some_and(is_sensitive_key) {
        return Value::String("[REDACTED]".into());
    }
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    let sanitized = sanitize_value(value, Some(&key));
                    (key, sanitized)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| sanitize_value(value, None))
                .collect(),
        ),
        Value::String(value) if value.len() > 1024 => {
            Value::String(value.chars().take(1024).collect())
        }
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_summary_never_contains_sensitive_values() {
        let payload = json!({
            "name": "visible-name",
            "credential_env": "SECRET_ENV",
            "api_key": "super-secret",
            "prompt": "private prompt",
            "nested": {"display_name": "safe"}
        });
        let summary = summarize_diff(&payload).to_string();
        assert!(summary.contains("display_name"));
        assert!(!summary.contains("SECRET_ENV"));
        assert!(!summary.contains("super-secret"));
        assert!(!summary.contains("private prompt"));
    }

    #[test]
    fn context_extracts_actor_request_id_and_resource() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", "req-123".parse().unwrap());
        headers.insert("x-admin-actor", "operator".parse().unwrap());
        let context = context_from_request(
            &Method::PUT,
            "/admin/sources/source-a/enabled",
            &headers,
            Some(&json!({"enabled": false})),
        );
        assert_eq!(context.request_id, "req-123");
        assert_eq!(context.actor, "operator");
        assert_eq!(context.action, "source.disable");
        assert_eq!(context.resource.as_deref(), Some("source/source-a"));
    }
}
