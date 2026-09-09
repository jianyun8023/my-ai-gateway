//! Narrow system-event storage and the unified, read-only event projection.
//!
//! Existing domain facts remain authoritative in their own tables. This
//! module writes only runtime facts that otherwise have no durable home and
//! projects notable rows from the existing tables for the Admin event feed.

use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sqlx::{PgConnection, PgPool};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::Mutex;
use uuid::Uuid;

const MAX_DETAILS_DEPTH: usize = 6;
const MAX_DETAILS_FIELDS: usize = 64;
const MAX_DETAILS_STRING: usize = 512;

/// SQL/programming, constraint, permission and decoding failures do not imply
/// lost connectivity. Pool exhaustion is included because callers cannot
/// acquire a usable connection, even when the server itself is healthy.
pub(crate) fn is_connection_error(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => true,
        sqlx::Error::Database(error) => error.code().is_some_and(|code| {
            code.starts_with("08") || matches!(code.as_ref(), "57P01" | "57P02" | "57P03")
        }),
        _ => false,
    }
}

const UNIFIED_EVENTS_CTE: &str = r#"
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
        'system_events'::text AS source,
        FALSE AS redact_subject_id,
        category = 'configuration' AND correlation_id IS NOT NULL AS redact_correlation_id
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
            'actor', CASE WHEN actor IS NULL THEN NULL ELSE '[REDACTED]' END,
            'resource', resource,
            'error_code', error_code,
            'completed_at', completed_at,
            'metadata', details
        )) AS details,
        'audit_logs'::text AS source,
        NULLIF(request_id, '') IS NOT NULL AND resource_id IS NULL AS redact_subject_id,
        NULLIF(request_id, '') IS NOT NULL AS redact_correlation_id
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
        'account_health_events'::text AS source,
        FALSE AS redact_subject_id,
        FALSE AS redact_correlation_id
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
            'requested_by', CASE WHEN requested_by IS NULL THEN NULL ELSE '[REDACTED]' END
        )) AS details,
        'source_discovery_runs'::text AS source,
        FALSE AS redact_subject_id,
        FALSE AS redact_correlation_id
    FROM source_discovery_runs

    UNION ALL

    SELECT
        'request:' || md5(request_id) AS event_id,
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
        'usage_events'::text AS source,
        FALSE AS redact_subject_id,
        FALSE AS redact_correlation_id
    FROM usage_events
    WHERE NOT success OR fallback_reason IS NOT NULL OR degraded
)
"#;

const EVENT_PAGE_SQL: &str = r#"
SELECT event_id,occurred_at,category,event_type,level,subject_type,
       CASE
           WHEN redact_subject_id
             OR (redact_correlation_id AND subject_id IS NOT NULL AND subject_id = correlation_id)
           THEN '[REDACTED]'
           ELSE subject_id
       END AS subject_id,
       CASE WHEN redact_correlation_id THEN '[REDACTED]' ELSE correlation_id END AS correlation_id,
       message,details,source
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
    recovering: bool,
    failure_during_recovery_at: Option<DateTime<Utc>>,
}

fn new_database_incident(component: String, occurred_at: DateTime<Utc>) -> DatabaseIncident {
    DatabaseIncident {
        id: Uuid::new_v4().to_string(),
        occurred_at,
        component,
        persisted: false,
        recovering: false,
        failure_during_recovery_at: None,
    }
}

#[derive(Clone)]
pub(crate) struct EventRepository {
    pool: Option<PgPool>,
    database_incidents: Arc<Mutex<BTreeMap<String, DatabaseIncident>>>,
    has_database_incidents: Arc<AtomicBool>,
}

impl EventRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self {
            pool: Some(pool),
            database_incidents: Arc::new(Mutex::new(BTreeMap::new())),
            has_database_incidents: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn disabled() -> Self {
        Self {
            pool: None,
            database_incidents: Arc::new(Mutex::new(BTreeMap::new())),
            has_database_incidents: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.pool.is_some()
    }

    pub(crate) async fn insert(&self, event: &SystemEvent) -> Result<(), sqlx::Error> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let mut connection = pool.acquire().await?;
        Self::insert_with_connection(&mut connection, event).await
    }

    async fn insert_with_connection(
        connection: &mut PgConnection,
        event: &SystemEvent,
    ) -> Result<(), sqlx::Error> {
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
        .execute(connection)
        .await?;
        Ok(())
    }

    /// Runtime event persistence must never turn an otherwise successful
    /// gateway operation into a failure.
    pub(crate) async fn record(&self, event: SystemEvent) {
        match self.insert(&event).await {
            Ok(()) => self.database_recovered("system_events.write").await,
            Err(error) => {
                tracing::warn!(event_type = %event.event_type, "failed to persist system event");
                if is_connection_error(&error) {
                    self.remember_database_failure("system_events.write").await;
                }
            }
        }
    }

    /// Persist diagnostics emitted because a database-backed operation failed
    /// only when a pooled connection is immediately available. The diagnostic
    /// must not add another acquisition timeout to an already failed response,
    /// and a successful insert is not proof that the original operation
    /// recovered. A later successful domain operation closes the incident.
    pub(crate) async fn record_during_database_incident(&self, event: SystemEvent) {
        let Some(pool) = &self.pool else {
            return;
        };
        let Some(mut connection) = pool.try_acquire() else {
            return;
        };
        if let Err(error) = Self::insert_with_connection(&mut connection, &event).await {
            tracing::warn!(event_type = %event.event_type, "failed to persist system event");
            if is_connection_error(&error) {
                self.remember_database_failure("system_events.write").await;
            }
        }
    }

    /// Remember the first unresolved outage for each database-backed component
    /// in memory. If PostgreSQL is unavailable, the failure row is backfilled
    /// with its original timestamp after the first later success of that same
    /// component, followed by a recovery row with the same incident correlation
    /// id. Failures and successes from other components are tracked independently.
    pub(crate) async fn database_failed(&self, component: &str, error: &sqlx::Error) {
        if self.pool.is_none() || !is_connection_error(error) {
            return;
        }
        self.remember_database_failure(component).await;
    }

    pub(crate) async fn database_recovered(&self, component: &str) {
        if !self.has_database_incidents.load(Ordering::Acquire) {
            return;
        }
        let component = safe_text(component, 64, "database");
        let incident = {
            let mut incidents = self.database_incidents.lock().await;
            let Some(incident) = incidents.get_mut(&component) else {
                return;
            };
            if incident.recovering {
                return;
            }
            incident.recovering = true;
            incident.failure_during_recovery_at = None;
            incident.clone()
        };
        let mut persisted = incident.persisted;
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
                self.release_database_recovery(&component, &incident.id, false)
                    .await;
                return;
            }
            persisted = true;
        }
        let recovery = SystemEvent::new(
            "database",
            "database.connection_recovered",
            "info",
            "database",
            "Database operation recovered",
        )
        .subject_id("postgresql")
        .correlation_id(incident.id.clone())
        .details(json!({
            "component": component,
        }));
        match self.insert(&recovery).await {
            Ok(()) => {
                let mut incidents = self.database_incidents.lock().await;
                let reopened_at = incidents
                    .get(&component)
                    .filter(|current| current.id == incident.id)
                    .and_then(|current| current.failure_during_recovery_at);
                if let Some(occurred_at) = reopened_at {
                    incidents.insert(
                        component.clone(),
                        new_database_incident(component, occurred_at),
                    );
                } else if incidents
                    .get(&component)
                    .is_some_and(|current| current.id == incident.id)
                {
                    incidents.remove(&component);
                    if incidents.is_empty() {
                        self.has_database_incidents.store(false, Ordering::Release);
                    }
                }
            }
            Err(_) => {
                self.release_database_recovery(&component, &incident.id, persisted)
                    .await;
                tracing::warn!("failed to persist database recovery event");
            }
        }
    }

    async fn remember_database_failure(&self, component: &str) {
        if self.pool.is_none() {
            return;
        }
        let component = safe_text(component, 64, "database");
        let mut incidents = self.database_incidents.lock().await;
        if let Some(incident) = incidents.get_mut(&component) {
            if incident.recovering && incident.failure_during_recovery_at.is_none() {
                incident.failure_during_recovery_at = Some(Utc::now());
            }
            return;
        }
        incidents.insert(
            component.clone(),
            new_database_incident(component, Utc::now()),
        );
        self.has_database_incidents.store(true, Ordering::Release);
    }

    async fn release_database_recovery(&self, component: &str, id: &str, persisted: bool) {
        let mut incidents = self.database_incidents.lock().await;
        if let Some(incident) = incidents
            .get_mut(component)
            .filter(|incident| incident.id == id)
        {
            incident.persisted |= persisted;
            incident.recovering = false;
            incident.failure_during_recovery_at = None;
        }
    }

    pub(crate) async fn observe<T>(
        &self,
        component: &str,
        result: Result<T, sqlx::Error>,
    ) -> Result<T, sqlx::Error> {
        match &result {
            Ok(_) => self.database_recovered(component).await,
            Err(error) => self.database_failed(component, error).await,
        }
        result
    }

    pub(crate) async fn event_type_options(
        &self,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        search: &str,
        limit: i64,
    ) -> Result<Vec<String>, sqlx::Error> {
        let pool = self.pool.as_ref().ok_or(sqlx::Error::PoolClosed)?;
        let sql = format!(
            "{UNIFIED_EVENTS_CTE}
             SELECT DISTINCT event_type COLLATE \"C\" AS value FROM unified_events
             WHERE ($1::timestamptz IS NULL OR occurred_at >= $1)
               AND ($2::timestamptz IS NULL OR occurred_at < $2)
               AND event_type IS NOT NULL AND btrim(event_type) <> ''
               AND strpos(lower(event_type), lower($3)) > 0
             ORDER BY value LIMIT $4"
        );
        let result = sqlx::query_scalar(&sql)
            .bind(from)
            .bind(to)
            .bind(search)
            .bind(limit.clamp(1, 100) + 1)
            .fetch_all(pool)
            .await;
        self.observe("events.filter_options", result).await
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
        let sql = format!("{UNIFIED_EVENTS_CTE}{EVENT_PAGE_SQL}");
        let mut data = sqlx::query_as::<_, EventRecord>(&sql)
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
        for event in &mut data {
            event.message = sanitize_detail_string(std::mem::take(&mut event.message));
            event.event_type = safe_text(&event.event_type, 128, "event");
            event.subject_type = safe_text(&event.subject_type, 64, "subject");
            event.subject_id = event.subject_id.take().map(sanitize_detail_string);
            event.correlation_id = event.correlation_id.take().map(sanitize_detail_string);
            event.details = sanitize_details(std::mem::take(&mut event.details), 0);
        }
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
    [
        "sk-",
        "sk_",
        "ghp_",
        "github_pat_",
        "gw_",
        "gwenc:",
        "bearer ",
    ]
    .iter()
    .any(|prefix| lower.contains(prefix) && value.len() >= 16)
        || [
            "authorization:",
            "api_key=",
            "api-key=",
            "password=",
            "credential=",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
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
                    sanitized.insert(
                        sanitize_detail_string(key),
                        Value::String("[REDACTED]".into()),
                    );
                } else {
                    sanitized.insert(
                        sanitize_detail_string(key),
                        sanitize_details(value, depth + 1),
                    );
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
    fn connection_classification_excludes_query_and_decode_errors() {
        assert!(is_connection_error(&sqlx::Error::PoolTimedOut));
        assert!(is_connection_error(&sqlx::Error::PoolClosed));
        assert!(is_connection_error(&sqlx::Error::Io(std::io::Error::from(
            std::io::ErrorKind::ConnectionReset,
        ))));
        assert!(!is_connection_error(&sqlx::Error::RowNotFound));
        assert!(!is_connection_error(&sqlx::Error::Protocol(
            "bad query result".into()
        )));
        assert!(!is_connection_error(&sqlx::Error::ColumnNotFound(
            "actor".into()
        )));
    }

    fn test_app(database: &Database) -> axum::Router {
        let control =
            crate::control_plane::ControlPlane::new(database.pool().clone(), "127.0.0.1:0")
                .with_events(database.event_repository());
        crate::app::application(AppState {
            live: Arc::new(std::sync::RwLock::new(LiveConfig::legacy(Arc::new(
                GatewayConfig {
                    listen_addr: "127.0.0.1:0".into(),
                    providers: vec![],
                    accounts: vec![],
                    routes: vec![],
                },
            )))),
            http: crate::http::test_client().unwrap(),
            db: Some(database.clone()),
            control_plane: Some(control),
            events: database.event_repository(),
            health: HealthRegistry::with_database_config(database.clone(), Default::default()),
            admin_auth: AdminAuth::test(),
            secrets: SecretResolver::empty(),
            prometheus_handle: observability::prometheus_handle(),
        })
    }

    async fn admin_request(
        app: &axum::Router,
        method: &str,
        path: &str,
        actor: &str,
    ) -> (StatusCode, Value) {
        admin_request_with_request_id(app, method, path, actor, None).await
    }

    async fn admin_request_with_request_id(
        app: &axum::Router,
        method: &str,
        path: &str,
        actor: &str,
        request_id: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {TEST_ADMIN_KEY}"))
            .header("x-admin-actor", actor);
        if let Some(request_id) = request_id {
            request = request.header("x-request-id", request_id);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    async fn verify_read_boundary_and_query_failure(database: &Database) {
        let app = test_app(database);
        let actor_secret = "AIzaSyOpaqueCallerControlledActor123456789";
        let external_correlation = "AIzaSyOpaqueCallerCorrelation987654321";
        let (status, _) = admin_request_with_request_id(
            &app,
            "POST",
            "/admin/config/reload",
            actor_secret,
            Some(external_correlation),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (raw_actor, raw_request_id, raw_operation_id): (String, Option<String>, String) =
            sqlx::query_as(
                "SELECT actor,request_id,operation_id FROM audit_logs WHERE action='config.reload' ORDER BY id DESC LIMIT 1",
            )
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(
            raw_actor, actor_secret,
            "exercise the existing caller-controlled audit writer"
        );
        assert_eq!(raw_request_id.as_deref(), Some(external_correlation));
        assert_eq!(raw_operation_id, external_correlation);
        let correlated_system_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM system_events WHERE category='configuration' AND correlation_id=$1",
        )
        .bind(external_correlation)
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert!(correlated_system_events > 0);

        let (status, body) = admin_request(&app, "GET", "/admin/events", "test").await;
        assert_eq!(status, StatusCode::OK);
        let serialized = body.to_string();
        assert!(!serialized.contains(actor_secret));
        assert!(!serialized.contains(external_correlation));
        let rows = body["data"].as_array().unwrap();
        let audit_row = rows
            .iter()
            .find(|row| row["source"] == "audit_logs" && row["event_type"] == "config.reload")
            .unwrap();
        assert_eq!(audit_row["details"]["actor"], "[REDACTED]");
        assert_eq!(audit_row["subject_id"], "[REDACTED]");
        assert_eq!(audit_row["correlation_id"], "[REDACTED]");
        assert!(rows
            .iter()
            .filter(|row| {
                row["source"] == "system_events" && row["category"] == "configuration"
            })
            .any(|row| row["correlation_id"] == "[REDACTED]"));

        let (status, correlated) = admin_request(
            &app,
            "GET",
            &format!("/admin/events?correlation_id={external_correlation}"),
            "test",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!correlated.to_string().contains(external_correlation));
        let correlated_rows = correlated["data"].as_array().unwrap();
        assert!(!correlated_rows.is_empty());
        assert!(correlated_rows
            .iter()
            .all(|row| row["correlation_id"] == "[REDACTED]"));

        let (status, subject) = admin_request(
            &app,
            "GET",
            &format!("/admin/events?source=audit_logs&subject_id={external_correlation}"),
            "test",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(subject["data"].as_array().unwrap().len(), 1);
        assert_eq!(subject["data"][0]["subject_id"], "[REDACTED]");
        assert!(!subject.to_string().contains(external_correlation));

        sqlx::query("ALTER TABLE source_discovery_runs RENAME TO unavailable_discovery_runs")
            .execute(database.pool())
            .await
            .unwrap();
        let (status, _) = admin_request(&app, "GET", "/admin/events", "test").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        let error = database
            .event_repository()
            .list(&EventFilter::default(), 10, None)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42P01")
        );
        assert!(!is_connection_error(&error));
        assert_eq!(
            admin_request(&app, "POST", "/admin/config/reload", "test")
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            admin_request(&app, "GET", "/admin/events", "test").await.0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        let incidents: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM system_events WHERE category='database'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(
            incidents, 0,
            "SQL errors must not generate connection failure/recovery pairs"
        );
        sqlx::query("ALTER TABLE unavailable_discovery_runs RENAME TO source_discovery_runs")
            .execute(database.pool())
            .await
            .unwrap();
    }

    #[test]
    fn details_redact_sensitive_values_and_unified_query_does_not_project_all_requests() {
        let value = sanitize_details(
            json!({
                "credential": "plain-value",
                "sk-secret-as-key-0123456789": "metadata",
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
        assert!(!value.to_string().contains("sk-secret-as-key-0123456789"));
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
            assert!(UNIFIED_EVENTS_CTE.contains(source));
        }
        assert!(UNIFIED_EVENTS_CTE
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
        verify_read_boundary_and_query_failure(&database).await;
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
            .any(|event| event.subject_id.as_deref() == Some(failed_request_id.as_str())));
        assert!(!page
            .data
            .iter()
            .any(|event| event.subject_id.as_deref() == Some(successful_request_id.as_str())));

        let (status, options) = admin_request(
            &test_app(&database),
            "GET",
            "/admin/events/filter-options?field=event_type",
            "test",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let expected_types = page
            .data
            .iter()
            .map(|event| &event.event_type)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            options,
            json!({ "data": expected_types, "has_more": false })
        );
        assert!(options["data"]
            .as_array()
            .unwrap()
            .contains(&json!("request.failed")));
        assert!(!options["data"]
            .as_array()
            .unwrap()
            .contains(&json!("request.success")));

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

        let requested_by_secret = "opaque-high-entropy-requester-value-7f4a91c2d8e6";
        let nested_secret = "gw_test_nested_metadata_0123456789";
        sqlx::query("UPDATE source_discovery_runs SET requested_by=$1 WHERE source_id=$2")
            .bind(requested_by_secret)
            .bind(&source_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE audit_logs SET details=$1 WHERE operation_id=$2")
            .bind(json!({"nested": {"label": nested_secret, "credential": "arbitrary-secret"}}))
            .bind(&operation_id)
            .execute(&pool)
            .await
            .unwrap();
        let (status, sanitized) =
            admin_request(&test_app(&database), "GET", "/admin/events", "test").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!sanitized.to_string().contains(requested_by_secret));
        assert!(!sanitized.to_string().contains(nested_secret));
        assert!(!sanitized.to_string().contains("arbitrary-secret"));
        assert!(sanitized["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["details"]["requested_by"] == "[REDACTED]"));

        repository
            .database_failed("events.test", &sqlx::Error::PoolTimedOut)
            .await;
        repository
            .record_during_database_incident(SystemEvent::new(
                "configuration",
                "runtime.snapshot_build_failed",
                "error",
                "runtime_snapshot",
                "Runtime snapshot build failed",
            ))
            .await;
        repository
            .database_failed("events.test", &sqlx::Error::PoolTimedOut)
            .await;
        repository.database_recovered("runtime.snapshot").await;
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

        verify_database_hooks(&url, &schema, &account_id).await;
        verify_filter_options(&database).await;

        drop(database);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop unified event test schema");
        admin.close().await;
    }

    async fn verify_filter_options(database: &Database) {
        let app = test_app(database);
        let pool = database.pool();
        let key_id: i64 = sqlx::query_scalar("INSERT INTO virtual_keys (name,key_prefix,key_hash,enabled) VALUES ('historical-key','secret-prefix','secret-hash',FALSE) RETURNING id")
            .fetch_one(pool).await.unwrap();
        // Resources deliberately have no current Source/Account/LogicalModel row.
        for (index, (model, timestamp, upstream)) in [
            ("before", "2020-01-01T00:00:00Z", None),
            ("Alpha", "2020-01-02T00:00:00Z", Some("upstream-old")),
            ("Alpha", "2020-01-02T00:00:01Z", Some("upstream-old")),
            ("Beta%_'", "2020-01-02T00:00:02Z", Some("")),
            (" ", "2020-01-02T00:00:03Z", None),
            ("after", "2020-01-03T00:00:00Z", None),
        ]
        .iter()
        .enumerate()
        {
            sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,upstream_model_id,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success,created_at,virtual_key_id) VALUES ($1,'old-provider','old-account','model',$2,$3,'old-source','old-client','openai_responses','openai_responses','native',200,TRUE,$4,$5)")
                .bind(format!("filter-option-{index}"))
                .bind(model).bind(upstream)
                .bind(DateTime::parse_from_rfc3339(timestamp).unwrap().with_timezone(&Utc))
                .bind(key_id).execute(pool).await.unwrap();
        }
        let range = "from=2020-01-02T00%3A00%3A00Z&to=2020-01-03T00%3A00%3A00Z";
        for (field, expected) in [
            ("logical_model", json!(["Alpha", "Beta%_'"])),
            ("upstream_model", json!(["upstream-old"])),
            ("provider", json!(["old-provider"])),
            ("source_id", json!(["old-source"])),
            ("account", json!(["old-account"])),
            ("client_source", json!(["old-client"])),
            ("virtual_key", json!([key_id.to_string()])),
        ] {
            let path = format!("/admin/usage/filter-options?field={field}&{range}");
            let (status, body) = admin_request(&app, "GET", &path, "test").await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body, json!({ "data": expected, "has_more": false }));
        }
        for (extra, expected, more) in [
            ("limit=1", json!(["Alpha"]), true),
            ("q=ALP", json!(["Alpha"]), false),
            ("q=%25_%27", json!(["Beta%_'"]), false),
            ("q=unmatched", json!([]), false),
        ] {
            let path = format!("/admin/usage/filter-options?field=logical_model&{range}&{extra}");
            let (status, body) = admin_request(&app, "GET", &path, "test").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, json!({"data": expected, "has_more": more}));
        }
        // Event type candidates use the same half-open bounds and never include
        // normal successful requests. Duplicate types across facts collapse.
        for (event_type, timestamp) in [
            ("test.options", "2020-01-02T00:00:00Z"),
            ("test.options", "2020-01-02T00:00:01Z"),
            ("test.outside", "2020-01-03T00:00:00Z"),
        ] {
            sqlx::query("INSERT INTO system_events (event_type,occurred_at,category,level,subject_type,message,details) VALUES ($1,$2,'lifecycle','info','gateway','test','{}')")
                .bind(event_type).bind(DateTime::parse_from_rfc3339(timestamp).unwrap().with_timezone(&Utc)).execute(pool).await.unwrap();
        }
        let (_, body) = admin_request(
            &app,
            "GET",
            &format!("/admin/events/filter-options?field=event_type&{range}&q=OPTIONS"),
            "test",
        )
        .await;
        assert_eq!(body, json!({"data": ["test.options"], "has_more": false}));
        let (_, body) = admin_request(
            &app,
            "GET",
            "/admin/events/filter-options?field=event_type&limit=1",
            "test",
        )
        .await;
        assert_eq!(body["data"].as_array().unwrap().len(), 1);
        assert_eq!(body["has_more"], true);
        for path in [
            "/admin/usage/filter-options?field=credential_env",
            "/admin/usage/filter-options?field=provider&limit=101",
            "/admin/events/filter-options?field=subject_id",
            "/admin/events/filter-options?field=event_type&from=invalid",
        ] {
            assert_eq!(
                admin_request(&app, "GET", path, "test").await.0,
                StatusCode::BAD_REQUEST
            );
        }
        for path in [
            "/admin/usage/filter-options?field=provider",
            "/admin/events/filter-options?field=event_type",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    async fn verify_database_hooks(url: &str, schema: &str, account_id: &str) {
        let options = PgConnectOptions::from_str(url)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_millis(200))
            .connect_with(options)
            .await
            .unwrap();
        let database = Database::from_pool(pool.clone());
        let events = database.event_repository();
        let control = crate::control_plane::ControlPlane::new(pool.clone(), "127.0.0.1:0")
            .with_events(events.clone());
        let app = test_app(&database);
        let mut usage = crate::infra::db::UsageEvent {
            request_id: Uuid::new_v4().to_string(),
            virtual_key_id: None,
            provider_id: "custom".into(),
            account_id: account_id.into(),
            model: "test".into(),
            logical_model: "test".into(),
            upstream_model_id: None,
            source_id: "test".into(),
            client_source: "test".into(),
            protocol_in: "openai_responses".into(),
            protocol_upstream: "openai_responses".into(),
            mode: "native".into(),
            status_code: 503,
            success: false,
            retry_count: 0,
            latency_ms: 0,
            ttft_ms: None,
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            cached_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_tokens: 0,
            usage_source: "missing".into(),
            degraded: false,
            route_id: None,
            streamed: true,
            error_summary: None,
            fallback_reason: None,
        };
        for component in [
            "auth.virtual_key",
            "usage.write",
            "health.write",
            "health.read",
            "usage.read",
            "control_plane.read",
            "runtime.snapshot",
        ] {
            let held = pool.acquire().await.unwrap();
            match component {
                "auth.virtual_key" => {
                    let headers = axum::http::HeaderMap::from_iter([(
                        axum::http::header::AUTHORIZATION,
                        "Bearer gw_invalid_fixture".parse().unwrap(),
                    )]);
                    assert!(
                        crate::auth::authorized_with_db(Some(&database), &headers, None)
                            .await
                            .is_none()
                    );
                }
                "usage.write" => {
                    assert!(database
                        .insert_usage_with_attempts(&usage, &[])
                        .await
                        .is_err());
                }
                "health.write" => {
                    assert!(database
                        .record_account_health_success(
                            account_id,
                            Utc::now(),
                            "passive",
                            None,
                            None
                        )
                        .await
                        .is_err());
                }
                "health.read" => {
                    assert!(database.account_health(account_id).await.is_err());
                }
                "usage.read" => {
                    assert!(database
                        .usage_aggregate(&crate::infra::db::UsageFilter::default())
                        .await
                        .is_err());
                }
                "control_plane.read" => {
                    assert_eq!(
                        admin_request(&app, "GET", "/admin/sources", "test").await.0,
                        StatusCode::INTERNAL_SERVER_ERROR
                    );
                }
                _ => {
                    assert!(control.load_snapshot().await.is_err());
                }
            }
            assert_eq!(
                events
                    .database_incidents
                    .lock()
                    .await
                    .get(component)
                    .unwrap()
                    .component,
                component
            );
            drop(held);
            events.database_recovered("unrelated.component").await;
            assert!(events
                .database_incidents
                .lock()
                .await
                .contains_key(component));
            match component {
                "auth.virtual_key" => {
                    assert!(database
                        .authenticate_virtual_key_with_identity("invalid", None)
                        .await
                        .unwrap()
                        .is_none());
                }
                "usage.write" => {
                    database
                        .insert_usage_with_attempts(&usage, &[])
                        .await
                        .unwrap();
                }
                "health.write" => {
                    database
                        .record_account_health_success(
                            account_id,
                            Utc::now(),
                            "passive",
                            None,
                            None,
                        )
                        .await
                        .unwrap();
                }
                "health.read" => {
                    database.account_health(account_id).await.unwrap();
                }
                "usage.read" => {
                    database
                        .usage_aggregate(&crate::infra::db::UsageFilter::default())
                        .await
                        .unwrap();
                }
                "control_plane.read" => {
                    assert_eq!(
                        admin_request(&app, "GET", "/admin/sources", "test").await.0,
                        StatusCode::OK
                    );
                }
                _ => {
                    control.load_snapshot().await.unwrap();
                }
            }
            assert!(
                !events
                    .database_incidents
                    .lock()
                    .await
                    .contains_key(component),
                "{component} success closes its own incident"
            );
            let pair: Vec<(String, String)> = sqlx::query_as("SELECT event_type,correlation_id FROM system_events WHERE category='database' AND details->>'component'=$1 ORDER BY occurred_at,id")
                .bind(component).fetch_all(&pool).await.unwrap();
            assert_eq!(
                pair.len(),
                2,
                "{component} must emit exactly one failure/recovery pair without a periodic probe"
            );
            assert_eq!(pair[0].0, "database.connection_failed");
            assert_eq!(pair[1].0, "database.connection_recovered");
            assert_eq!(pair[0].1, pair[1].1);
        }

        usage.request_id = Uuid::new_v4().to_string();
        let held = pool.acquire().await.unwrap();
        let headers = axum::http::HeaderMap::from_iter([(
            axum::http::header::AUTHORIZATION,
            "Bearer gw_invalid_fixture".parse().unwrap(),
        )]);
        tokio::time::timeout(
            Duration::from_millis(100),
            events.record_during_database_incident(SystemEvent::new(
                "configuration",
                "runtime.snapshot_build_failed",
                "error",
                "runtime_snapshot",
                "Runtime snapshot build failed",
            )),
        )
        .await
        .expect("database failure diagnostics must not wait for the exhausted pool");
        tokio::time::timeout(Duration::from_millis(350), async {
            let (authorized, usage_result) = tokio::join!(
                crate::auth::authorized_with_db(Some(&database), &headers, None),
                database.insert_usage_with_attempts(&usage, &[]),
            );
            assert!(authorized.is_none());
            assert!(usage_result.is_err());
        })
        .await
        .expect("concurrent pool timeouts must not retry the unavailable pool or serialize on the incident mutex");
        {
            let incidents = events.database_incidents.lock().await;
            assert!(incidents.contains_key("auth.virtual_key"));
            assert!(incidents.contains_key("usage.write"));
        }
        drop(held);

        database
            .insert_usage_with_attempts(&usage, &[])
            .await
            .unwrap();
        {
            let incidents = events.database_incidents.lock().await;
            assert!(incidents.contains_key("auth.virtual_key"));
            assert!(!incidents.contains_key("usage.write"));
        }
        let usage_pair: Vec<(String, String)> = sqlx::query_as("SELECT event_type,correlation_id FROM system_events WHERE category='database' AND details->>'component'='usage.write' ORDER BY occurred_at,id")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(usage_pair.len(), 4);
        assert_eq!(usage_pair[2].0, "database.connection_failed");
        assert_eq!(usage_pair[3].0, "database.connection_recovered");
        assert_eq!(usage_pair[2].1, usage_pair[3].1);

        assert!(database
            .authenticate_virtual_key_with_identity("invalid", None)
            .await
            .unwrap()
            .is_none());
        assert!(events.database_incidents.lock().await.is_empty());
        let auth_pair: Vec<(String, String)> = sqlx::query_as("SELECT event_type,correlation_id FROM system_events WHERE category='database' AND details->>'component'='auth.virtual_key' ORDER BY occurred_at,id")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(auth_pair.len(), 4);
        assert_eq!(auth_pair[2].0, "database.connection_failed");
        assert_eq!(auth_pair[3].0, "database.connection_recovered");
        assert_eq!(auth_pair[2].1, auth_pair[3].1);
        pool.close().await;
    }
}
