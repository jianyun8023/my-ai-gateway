//! Narrow system-event storage and the unified, read-only event projection.
//!
//! Existing domain facts remain authoritative in their own tables. This
//! module writes only runtime facts that otherwise have no durable home and
//! projects notable rows from the existing tables for the Admin event feed.

use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

const MAX_DETAILS_DEPTH: usize = 6;
const MAX_DETAILS_FIELDS: usize = 64;
const MAX_DETAILS_STRING: usize = 512;

const UNIFIED_EVENTS_SQL: &str = r#"
WITH unified_events AS (
    SELECT
        'system:' || id::text AS event_id,
        occurred_at,
        category,
        event_type,
        level,
        subject_type,
        subject_id,
        correlation_id,
        message,
        details,
        'system_events'::text AS source
    FROM system_events

    UNION ALL

    SELECT
        'audit:' || id::text AS event_id,
        created_at AS occurred_at,
        CASE
            WHEN action LIKE 'retention.%' OR action LIKE 'backup.%' THEN 'operation'
            ELSE 'admin'
        END AS category,
        action AS event_type,
        CASE
            WHEN status = 'cancel_requested' OR status = 'cancelled' THEN 'warning'
            WHEN result IN ('failure', 'conflict', 'rollback') OR status = 'failed' THEN 'error'
            ELSE 'info'
        END AS level,
        COALESCE(resource_type, 'operation') AS subject_type,
        COALESCE(resource_id, operation_id) AS subject_id,
        COALESCE(NULLIF(request_id, ''), operation_id) AS correlation_id,
        action AS message,
        jsonb_strip_nulls(jsonb_build_object(
            'status', status,
            'result', result,
            'actor', actor,
            'resource', resource,
            'error_code', error_code,
            'completed_at', completed_at,
            'metadata', details
        )) AS details,
        'audit_logs'::text AS source
    FROM audit_logs

    UNION ALL

    SELECT
        'health:' || id::text AS event_id,
        observed_at AS occurred_at,
        'health'::text AS category,
        'account.health.' || status AS event_type,
        CASE
            WHEN status IN ('cooling_down', 'unhealthy', 'stale') THEN 'warning'
            ELSE 'info'
        END AS level,
        'account'::text AS subject_type,
        account_id AS subject_id,
        CASE WHEN connection_test_id IS NULL THEN NULL ELSE 'connection-test:' || connection_test_id::text END AS correlation_id,
        'Account health changed to ' || status AS message,
        jsonb_strip_nulls(jsonb_build_object(
            'status', status,
            'health_source', source,
            'cooldown_until', cooldown_until,
            'consecutive_failures', consecutive_failures,
            'error_code', error_code,
            'connection_test_id', connection_test_id,
            'latency_ms', latency_ms
        )) AS details,
        'account_health_events'::text AS source
    FROM account_health_events

    UNION ALL

    SELECT
        'discovery:' || id::text AS event_id,
        completed_at AS occurred_at,
        'discovery'::text AS category,
        'source.discovery.' || status AS event_type,
        CASE WHEN status = 'succeeded' THEN 'info' ELSE 'error' END AS level,
        'source'::text AS subject_type,
        source_id AS subject_id,
        'discovery:' || id::text AS correlation_id,
        'Model discovery ' || status AS message,
        jsonb_strip_nulls(jsonb_build_object(
            'account_id', account_id,
            'provider_preset_id', provider_preset_id,
            'provider_preset_version', provider_preset_version,
            'discovered_model_count', discovered_model_count,
            'http_status', http_status,
            'latency_ms', latency_ms,
            'error_code', error_code,
            'requested_by', requested_by
        )) AS details,
        'source_discovery_runs'::text AS source
    FROM source_discovery_runs

    UNION ALL

    SELECT
        'request:' || request_id AS event_id,
        created_at AS occurred_at,
        'request'::text AS category,
        CASE
            WHEN NOT success THEN 'request.failed'
            WHEN fallback_reason IS NOT NULL THEN 'request.fallback'
            ELSE 'request.degraded'
        END AS event_type,
        CASE WHEN NOT success THEN 'error' ELSE 'warning' END AS level,
        'request'::text AS subject_type,
        request_id AS subject_id,
        request_id AS correlation_id,
        CASE
            WHEN NOT success THEN 'Gateway request failed'
            WHEN fallback_reason IS NOT NULL THEN 'Gateway request used fallback'
            ELSE 'Gateway request used degraded routing'
        END AS message,
        jsonb_strip_nulls(jsonb_build_object(
            'logical_model', logical_model,
            'upstream_model_id', upstream_model_id,
            'provider_id', provider_id,
            'source_id', source_id,
            'account_id', account_id,
            'protocol_in', protocol_in,
            'protocol_upstream', protocol_upstream,
            'status_code', status_code,
            'retry_count', retry_count,
            'route_id', route_id,
            'streamed', streamed,
            'fallback_reason', fallback_reason,
            'error_summary', error_summary
        )) AS details,
        'usage_events'::text AS source
    FROM usage_events
    WHERE NOT success OR fallback_reason IS NOT NULL OR degraded
)
SELECT event_id,occurred_at,category,event_type,level,subject_type,subject_id,
       correlation_id,message,details,source
FROM unified_events
WHERE ($1::timestamptz IS NULL OR occurred_at >= $1)
  AND ($2::timestamptz IS NULL OR occurred_at > $2)
  AND ($3::timestamptz IS NULL OR occurred_at < $3)
  AND ($4::text IS NULL OR category = $4)
  AND ($5::text IS NULL OR level = $5)
  AND ($6::text IS NULL OR event_type = $6)
  AND ($7::text IS NULL OR subject_type = $7)
  AND ($8::text IS NULL OR subject_id = $8)
  AND ($9::text IS NULL OR correlation_id = $9)
  AND ($10::text IS NULL OR source = $10)
  AND ($11::timestamptz IS NULL OR (occurred_at, event_id) < ($11, $12::text))
ORDER BY occurred_at DESC, event_id DESC
LIMIT $13
"#;

#[derive(Clone, Debug)]
pub(crate) struct SystemEvent {
    pub(crate) occurred_at: DateTime<Utc>,
    pub(crate) category: String,
    pub(crate) event_type: String,
    pub(crate) level: String,
    pub(crate) subject_type: String,
    pub(crate) subject_id: Option<String>,
    pub(crate) correlation_id: Option<String>,
    pub(crate) message: String,
    pub(crate) details: Value,
}

impl SystemEvent {
    pub(crate) fn new(
        category: &str,
        event_type: &str,
        level: &str,
        subject_type: &str,
        message: &str,
    ) -> Self {
        Self {
            occurred_at: Utc::now(),
            category: category.to_owned(),
            event_type: event_type.to_owned(),
            level: level.to_owned(),
            subject_type: subject_type.to_owned(),
            subject_id: None,
            correlation_id: None,
            message: message.to_owned(),
            details: json!({}),
        }
    }

    pub(crate) fn subject_id(mut self, subject_id: impl Into<String>) -> Self {
        self.subject_id = non_empty(subject_id.into());
        self
    }

    pub(crate) fn correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = non_empty(correlation_id.into());
        self
    }

    pub(crate) fn details(mut self, details: Value) -> Self {
        self.details = sanitize_details(details, 0);
        if !self.details.is_object() {
            self.details = json!({});
        }
        self
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EventFilter {
    pub(crate) from: Option<DateTime<Utc>>,
    pub(crate) since: Option<DateTime<Utc>>,
    pub(crate) to: Option<DateTime<Utc>>,
    pub(crate) category: Option<String>,
    pub(crate) level: Option<String>,
    pub(crate) event_type: Option<String>,
    pub(crate) subject_type: Option<String>,
    pub(crate) subject_id: Option<String>,
    pub(crate) correlation_id: Option<String>,
    pub(crate) source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EventCursor {
    pub(crate) occurred_at: DateTime<Utc>,
    pub(crate) event_id: String,
}

impl EventCursor {
    pub(crate) fn encode(&self) -> String {
        format!("{}:{}", self.occurred_at.timestamp_micros(), self.event_id)
    }

    pub(crate) fn decode(value: &str) -> Option<Self> {
        let (micros, event_id) = value.split_once(':')?;
        if event_id.is_empty() {
            return None;
        }
        Some(Self {
            occurred_at: Utc.timestamp_micros(micros.parse().ok()?).single()?,
            event_id: event_id.to_owned(),
        })
    }
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub(crate) struct EventRecord {
    pub(crate) event_id: String,
    pub(crate) occurred_at: DateTime<Utc>,
    pub(crate) category: String,
    pub(crate) event_type: String,
    pub(crate) level: String,
    pub(crate) subject_type: String,
    pub(crate) subject_id: Option<String>,
    pub(crate) correlation_id: Option<String>,
    pub(crate) message: String,
    pub(crate) details: Value,
    pub(crate) source: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct EventPage {
    pub(crate) data: Vec<EventRecord>,
    pub(crate) next_cursor: Option<String>,
    pub(crate) has_more: bool,
}

#[derive(Clone, Debug)]
struct DatabaseIncident {
    id: String,
    occurred_at: DateTime<Utc>,
    component: String,
    persisted: bool,
}

#[derive(Clone)]
pub(crate) struct EventRepository {
    pool: Option<PgPool>,
    database_incident: Arc<Mutex<Option<DatabaseIncident>>>,
}

impl EventRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self {
            pool: Some(pool),
            database_incident: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn disabled() -> Self {
        Self {
            pool: None,
            database_incident: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.pool.is_some()
    }

    pub(crate) async fn insert(&self, event: &SystemEvent) -> Result<(), sqlx::Error> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let event = normalized_event(event);
        sqlx::query(
            "INSERT INTO system_events (occurred_at,category,event_type,level,subject_type,subject_id,correlation_id,message,details) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(event.occurred_at)
        .bind(event.category)
        .bind(event.event_type)
        .bind(event.level)
        .bind(event.subject_type)
        .bind(event.subject_id)
        .bind(event.correlation_id)
        .bind(event.message)
        .bind(event.details)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Runtime event persistence must never turn an otherwise successful
    /// gateway operation into a failure.
    pub(crate) async fn record(&self, event: SystemEvent) {
        match self.insert(&event).await {
            Ok(()) => self.database_recovered("system_events.write").await,
            Err(_) => {
                tracing::warn!(event_type = %event.event_type, "failed to persist system event");
                self.remember_database_failure("system_events.write").await;
            }
        }
    }

    /// Persist diagnostics emitted because a database-backed operation
    /// failed without treating the diagnostic insert itself as proof that the
    /// original operation recovered. A later successful domain operation
    /// closes the incident through `database_recovered`.
    pub(crate) async fn record_during_database_incident(&self, event: SystemEvent) {
        if self.insert(&event).await.is_err() {
            tracing::warn!(event_type = %event.event_type, "failed to persist system event");
            self.remember_database_failure("system_events.write").await;
        }
    }

    /// Remember one database outage in memory. If PostgreSQL is unavailable,
    /// the failure row is backfilled with its original timestamp after the
    /// first later success, followed by a recovery row with the same incident
    /// correlation id.
    pub(crate) async fn database_failed(&self, component: &str) {
        if self.pool.is_none() {
            return;
        }
        let mut current = self.database_incident.lock().await;
        if current.is_some() {
            return;
        }
        let incident = DatabaseIncident {
            id: Uuid::new_v4().to_string(),
            occurred_at: Utc::now(),
            component: safe_text(component, 64, "database"),
            persisted: false,
        };
        *current = Some(incident.clone());
        let mut event = SystemEvent::new(
            "database",
            "database.connection_failed",
            "error",
            "database",
            "Database operation became unavailable",
        )
        .subject_id("postgresql")
        .correlation_id(incident.id.clone())
        .details(json!({
            "component": incident.component,
            "error_code": "database_unavailable",
        }));
        event.occurred_at = incident.occurred_at;
        if self.insert(&event).await.is_ok() {
            if let Some(current) = current.as_mut().filter(|value| value.id == incident.id) {
                current.persisted = true;
            }
        }
    }

    pub(crate) async fn database_recovered(&self, component: &str) {
        let mut current = self.database_incident.lock().await;
        let Some(mut incident) = current.clone() else {
            return;
        };
        if !incident.persisted {
            let mut failure = SystemEvent::new(
                "database",
                "database.connection_failed",
                "error",
                "database",
                "Database operation became unavailable",
            )
            .subject_id("postgresql")
            .correlation_id(incident.id.clone())
            .details(json!({
                "component": incident.component,
                "error_code": "database_unavailable",
                "recorded_after_recovery": true,
            }));
            failure.occurred_at = incident.occurred_at;
            if self.insert(&failure).await.is_err() {
                return;
            }
            incident.persisted = true;
            if let Some(current) = current.as_mut().filter(|value| value.id == incident.id) {
                current.persisted = true;
            }
        }
        let recovery = SystemEvent::new(
            "database",
            "database.connection_recovered",
            "info",
            "database",
            "Database operation recovered",
        )
        .subject_id("postgresql")
        .correlation_id(incident.id)
        .details(json!({
            "component": safe_text(component, 64, "database"),
        }));
        match self.insert(&recovery).await {
            Ok(()) => *current = None,
            Err(_) => tracing::warn!("failed to persist database recovery event"),
        }
    }

    async fn remember_database_failure(&self, component: &str) {
        if self.pool.is_none() {
            return;
        }
        let mut current = self.database_incident.lock().await;
        if current.is_none() {
            *current = Some(DatabaseIncident {
                id: Uuid::new_v4().to_string(),
                occurred_at: Utc::now(),
                component: safe_text(component, 64, "database"),
                persisted: false,
            });
        }
    }

    pub(crate) async fn list(
        &self,
        filter: &EventFilter,
        limit: i64,
        cursor: Option<&EventCursor>,
    ) -> Result<EventPage, sqlx::Error> {
        let pool = self.pool.as_ref().ok_or(sqlx::Error::PoolClosed)?;
        let limit = limit.clamp(1, 500);
        let cursor_time = cursor.map(|value| value.occurred_at);
        let cursor_id = cursor.map(|value| value.event_id.as_str());
        let mut data = sqlx::query_as::<_, EventRecord>(UNIFIED_EVENTS_SQL)
            .bind(filter.from)
            .bind(filter.since)
            .bind(filter.to)
            .bind(filter.category.as_deref())
            .bind(filter.level.as_deref())
            .bind(filter.event_type.as_deref())
            .bind(filter.subject_type.as_deref())
            .bind(filter.subject_id.as_deref())
            .bind(filter.correlation_id.as_deref())
            .bind(filter.source.as_deref())
            .bind(cursor_time)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(pool)
            .await?;
        let has_more = data.len() as i64 > limit;
        if has_more {
            data.truncate(limit as usize);
        }
        let next_cursor = has_more.then(|| {
            let last = data.last().expect("a page with more rows is non-empty");
            EventCursor {
                occurred_at: last.occurred_at,
                event_id: last.event_id.clone(),
            }
            .encode()
        });
        Ok(EventPage {
            data,
            next_cursor,
            has_more,
        })
    }
}

fn normalized_event(event: &SystemEvent) -> SystemEvent {
    SystemEvent {
        occurred_at: event.occurred_at,
        category: match event.category.as_str() {
            "lifecycle" | "configuration" | "database" | "security" => event.category.clone(),
            _ => "lifecycle".into(),
        },
        event_type: safe_event_type(&event.event_type),
        level: match event.level.as_str() {
            "info" | "warning" | "error" => event.level.clone(),
            _ => "warning".into(),
        },
        subject_type: safe_text(&event.subject_type, 64, "gateway"),
        subject_id: event
            .subject_id
            .as_deref()
            .map(|value| safe_text(value, 256, "unknown")),
        correlation_id: event
            .correlation_id
            .as_deref()
            .map(|value| safe_text(value, 256, "unknown")),
        message: safe_text(&event.message, 256, "System event"),
        details: sanitize_details(event.details.clone(), 0),
    }
}

fn safe_event_type(value: &str) -> String {
    let value = value
        .chars()
        .filter_map(|character| {
            let character = character.to_ascii_lowercase();
            (character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
                .then_some(character)
        })
        .take(128)
        .collect::<String>();
    if value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
    {
        value
    } else {
        "system.event".into()
    }
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn safe_text(value: &str, limit: usize, fallback: &str) -> String {
    let value = value
        .chars()
        .filter(|character| !character.is_control())
        .take(limit)
        .collect::<String>();
    if value.trim().is_empty() {
        fallback.to_owned()
    } else if looks_like_secret(&value) {
        "[REDACTED]".into()
    } else {
        value
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "authorization",
        "api_key",
        "api-key",
        "apikey",
        "credential",
        "ciphertext",
        "secret",
        "password",
        "token",
        "key_hash",
        "prompt",
        "response",
        "body",
        "thinking",
        "signature",
    ]
    .iter()
    .any(|marker| key.contains(marker))
        || key.ends_with("_key")
}

fn looks_like_secret(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    (lower.starts_with("sk-") && value.len() >= 16)
        || (lower.starts_with("sk_") && value.len() >= 16)
        || (lower.starts_with("ghp_") && value.len() >= 20)
        || (lower.starts_with("bearer ") && value.len() >= 20)
}

fn sanitize_detail_string(value: String) -> String {
    let value = safe_text(&value, MAX_DETAILS_STRING, "[EMPTY]");
    if let Ok(mut url) = reqwest::Url::parse(&value) {
        let query_is_sensitive = url.query().is_some_and(|query| {
            let query = query.to_ascii_lowercase();
            [
                "api_key", "api-key", "apikey", "token", "secret", "password",
            ]
            .iter()
            .any(|marker| query.contains(marker))
        });
        if !url.username().is_empty()
            || url.password().is_some()
            || query_is_sensitive
            || url.fragment().is_some()
        {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            if query_is_sensitive {
                url.set_query(None);
            }
            url.set_fragment(None);
            return url.to_string();
        }
    }
    value
}

fn sanitize_details(value: Value, depth: usize) -> Value {
    if depth >= MAX_DETAILS_DEPTH {
        return Value::String("[TRUNCATED]".into());
    }
    match value {
        Value::Object(object) => {
            let mut sanitized = Map::new();
            for (key, value) in object.into_iter().take(MAX_DETAILS_FIELDS) {
                if is_sensitive_key(&key) {
                    sanitized.insert(key, Value::String("[REDACTED]".into()));
                } else {
                    sanitized.insert(key, sanitize_details(value, depth + 1));
                }
            }
            Value::Object(sanitized)
        }
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .take(MAX_DETAILS_FIELDS)
                .map(|value| sanitize_details(value, depth + 1))
                .collect(),
        ),
        Value::String(value) => Value::String(sanitize_detail_string(value)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::db::Database;
    use crate::{
        auth::AdminAuth,
        domain::config::GatewayConfig,
        infra::{health::HealthRegistry, observability, secrets::SecretResolver},
        state::{AppState, LiveConfig},
        test_helpers::TEST_ADMIN_KEY,
    };
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::{str::FromStr, sync::Arc, time::Duration};
    use tower::ServiceExt;

    #[test]
    fn cursor_round_trip_preserves_namespaced_id() {
        let cursor = EventCursor {
            occurred_at: "2026-09-09T01:02:03.456789Z".parse().unwrap(),
            event_id: "request:with:colons".into(),
        };
        assert_eq!(EventCursor::decode(&cursor.encode()), Some(cursor));
        assert!(EventCursor::decode("invalid").is_none());
    }

    #[test]
    fn details_redact_sensitive_values_and_unified_query_does_not_project_all_requests() {
        let value = sanitize_details(
            json!({
                "credential": "plain-value",
                "nested": {
                    "api_key": "sk-1234567890123456",
                    "session_token": "token-value",
                    "response_body": "private output",
                    "revision": 24
                },
                "endpoint": "https://user:password@example.com/path?token=secret#fragment",
            }),
            0,
        );
        assert_eq!(value["credential"], "[REDACTED]");
        assert_eq!(value["nested"]["api_key"], "[REDACTED]");
        assert_eq!(value["nested"]["session_token"], "[REDACTED]");
        assert_eq!(value["nested"]["response_body"], "[REDACTED]");
        assert_eq!(value["nested"]["revision"], 24);
        assert_eq!(value["endpoint"], "https://example.com/path");
        for source in [
            "system_events",
            "audit_logs",
            "account_health_events",
            "source_discovery_runs",
            "usage_events",
        ] {
            assert!(UNIFIED_EVENTS_SQL.contains(source));
        }
        assert!(UNIFIED_EVENTS_SQL
            .contains("WHERE NOT success OR fallback_reason IS NOT NULL OR degraded"));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL; run with the PostgreSQL regression suite"]
    async fn postgres_unified_projection_filters_and_paginates_authoritative_facts() {
        let Some(url) = std::env::var("TEST_DATABASE_URL").ok() else {
            eprintln!("TEST_DATABASE_URL is not set; skipping unified event regression");
            return;
        };
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect unified event test admin database");
        let schema = format!("events_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create unified event test schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse TEST_DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect isolated unified event schema");
        let database = Database::from_test_pool(pool.clone())
            .await
            .expect("migrate unified event schema");
        let suffix = Uuid::new_v4().simple().to_string();
        let source_id = format!("event-source-{suffix}");
        let account_id = format!("event-account-{suffix}");
        let operation_id = format!("event-operation-{suffix}");
        let failed_request_id = format!("failed-request-{suffix}");
        let successful_request_id = format!("successful-request-{suffix}");

        sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url) VALUES ($1,'Event source','custom',1,'{}','https://events.example')")
            .bind(&source_id)
            .execute(&pool)
            .await
            .expect("insert event Source fixture");
        sqlx::query("INSERT INTO accounts (id,provider_id,source_id,display_name) VALUES ($1,NULL,$2,'Event account')")
            .bind(&account_id)
            .bind(&source_id)
            .execute(&pool)
            .await
            .expect("insert event Account fixture");

        let repository = EventRepository::new(pool.clone());
        repository
            .insert(
                &SystemEvent::new(
                    "lifecycle",
                    "gateway.test_started",
                    "info",
                    "gateway",
                    "Unified event fixture",
                )
                .subject_id("process")
                .correlation_id(&operation_id),
            )
            .await
            .expect("insert system event fixture");
        sqlx::query("INSERT INTO audit_logs (operation_id,action,status,actor,details,request_id,resource_type,resource_id,result,diff) VALUES ($1,'retention.test','succeeded','events-test','{}',NULL,'retention',$1,'success','{}')")
            .bind(&operation_id)
            .execute(&pool)
            .await
            .expect("insert audit event fixture");
        sqlx::query("INSERT INTO account_health_events (account_id,status,source,observed_at,consecutive_failures) VALUES ($1,'cooling_down','passive',clock_timestamp(),1)")
            .bind(&account_id)
            .execute(&pool)
            .await
            .expect("insert health event fixture");
        sqlx::query("INSERT INTO source_discovery_runs (source_id,account_id,provider_preset_id,provider_preset_version,status,raw_snapshot,diff,discovered_model_count,latency_ms,error_code,error_message,requested_by,started_at,completed_at) VALUES ($1,$2,'custom',1,'failed',NULL,'{\"added\":[],\"changed\":[],\"missing\":[]}',0,1,'test_failure','Fixture failure','events-test',clock_timestamp()-INTERVAL '1 second',clock_timestamp())")
            .bind(&source_id)
            .bind(&account_id)
            .execute(&pool)
            .await
            .expect("insert discovery event fixture");
        for (request_id, success) in [(&failed_request_id, false), (&successful_request_id, true)] {
            sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success) VALUES ($1,'custom',$2,'model','model',$3,'events-test','openai_responses','openai_responses','native',$4,$5)")
                .bind(request_id)
                .bind(&account_id)
                .bind(&source_id)
                .bind(if success { 200 } else { 503 })
                .bind(success)
                .execute(&pool)
                .await
                .expect("insert usage event fixture");
        }

        let page = repository
            .list(&EventFilter::default(), 100, None)
            .await
            .expect("query unified event projection");
        let sources = page
            .data
            .iter()
            .map(|event| event.source.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            sources,
            std::collections::HashSet::from([
                "system_events",
                "audit_logs",
                "account_health_events",
                "source_discovery_runs",
                "usage_events",
            ])
        );
        assert!(page
            .data
            .iter()
            .any(|event| event.event_id == format!("request:{failed_request_id}")));
        assert!(!page
            .data
            .iter()
            .any(|event| event.event_id == format!("request:{successful_request_id}")));

        let operation_page = repository
            .list(
                &EventFilter {
                    correlation_id: Some(operation_id.clone()),
                    ..EventFilter::default()
                },
                100,
                None,
            )
            .await
            .expect("filter unified events by operation correlation");
        assert_eq!(operation_page.data.len(), 2);
        assert!(operation_page
            .data
            .iter()
            .all(|event| event.correlation_id.as_deref() == Some(operation_id.as_str())));

        let first = repository
            .list(&EventFilter::default(), 2, None)
            .await
            .expect("query first unified event page");
        assert_eq!(first.data.len(), 2);
        assert!(first.has_more);
        let cursor = EventCursor::decode(first.next_cursor.as_deref().unwrap()).unwrap();
        let second = repository
            .list(&EventFilter::default(), 100, Some(&cursor))
            .await
            .expect("query second unified event page");
        let event_ids = first
            .data
            .iter()
            .chain(&second.data)
            .map(|event| event.event_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(event_ids.len(), page.data.len());

        let state = AppState {
            live: Arc::new(std::sync::RwLock::new(LiveConfig::legacy(Arc::new(
                GatewayConfig {
                    listen_addr: "127.0.0.1:0".into(),
                    providers: Vec::new(),
                    accounts: Vec::new(),
                    routes: Vec::new(),
                },
            )))),
            http: crate::http::test_client().expect("event API HTTP client"),
            db: Some(database.clone()),
            control_plane: None,
            events: repository.clone(),
            health: HealthRegistry::new(Duration::from_secs(30)),
            admin_auth: AdminAuth::test(),
            secrets: SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
        };
        let response = crate::app::application(state)
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/admin/events?category=operation&operation_id={operation_id}&since=1970-01-01T00%3A00%3A00Z"
                    ))
                    .header("authorization", format!("Bearer {TEST_ADMIN_KEY}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("query unified event HTTP API");
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("read unified event HTTP response"),
        )
        .expect("parse unified event HTTP response");
        assert_eq!(body["version"], "v1");
        assert_eq!(body["range"]["boundary"], "(since,to)");
        assert_eq!(body["data"].as_array().unwrap().len(), 1);
        assert_eq!(body["data"][0]["source"], "audit_logs");

        repository.database_failed("events.test").await;
        repository
            .record_during_database_incident(SystemEvent::new(
                "configuration",
                "runtime.snapshot_build_failed",
                "error",
                "runtime_snapshot",
                "Runtime snapshot build failed",
            ))
            .await;
        repository.database_failed("events.duplicate").await;
        let recoveries_before_success: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM system_events WHERE event_type='database.connection_recovered'",
        )
        .fetch_one(&pool)
        .await
        .expect("count premature database recoveries");
        assert_eq!(recoveries_before_success, 0);
        repository.database_recovered("events.test").await;
        repository
            .database_recovered("events.already-recovered")
            .await;
        let incident_rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT event_type,correlation_id FROM system_events WHERE event_type IN ('database.connection_failed','database.connection_recovered') ORDER BY occurred_at,id",
        )
        .fetch_all(&pool)
        .await
        .expect("load database incident pair");
        assert_eq!(incident_rows.len(), 2);
        assert_eq!(incident_rows[0].0, "database.connection_failed");
        assert_eq!(incident_rows[1].0, "database.connection_recovered");
        assert_eq!(incident_rows[0].1, incident_rows[1].1);

        drop(database);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop unified event test schema");
        admin.close().await;
    }
}
