#[cfg(test)]
use crate::domain::config::{AccountConfig, GatewayConfig, ProviderConfig, RouteConfig};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{fmt, time::Duration};

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

/// Persisted account health row. `health_updated_at` is independent of the
/// account configuration `updated_at` and is the clock used for stale checks.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AccountHealthRow {
    pub account_id: String,
    pub enabled: bool,
    pub source_enabled: bool,
    pub health_status: String,
    pub health_source: String,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub consecutive_failures: i32,
    pub failure_window_started_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub health_updated_at: Option<DateTime<Utc>>,
    pub last_probe_at: Option<DateTime<Utc>>,
    pub last_probe_status: Option<String>,
    pub last_probe_error: Option<String>,
    pub account_updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UsageEvent {
    pub request_id: String,
    pub virtual_key_id: Option<i64>,
    pub provider_id: String,
    pub account_id: String,
    pub model: String,
    pub logical_model: String,
    pub upstream_model_id: Option<String>,
    pub source_id: String,
    pub client_source: String,
    pub protocol_in: String,
    pub protocol_upstream: String,
    pub mode: String,
    pub status_code: i32,
    pub success: bool,
    pub retry_count: i32,
    pub latency_ms: i64,
    pub ttft_ms: Option<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
    pub usage_source: String,
    pub degraded: bool,
    pub route_id: Option<String>,
    pub streamed: bool,
    pub error_summary: Option<String>,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct VirtualKeyRecord {
    pub id: i64,
    pub name: String,
    pub key_prefix: String,
    pub key_recoverable: bool,
    pub allowed_models: Value,
    pub scopes: Value,
    pub key_group: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub replaced_by_id: Option<i64>,
    pub overlap_until: Option<DateTime<Utc>>,
    pub origin: String,
}

/// The only scope currently consumed by the data plane. Keeping this as a
/// named contract makes adding another operation explicit instead of treating
/// an arbitrary user string as permission.
pub const VIRTUAL_KEY_INVOKE_SCOPE: &str = "gateway:invoke";
pub const VIRTUAL_KEY_MODELS_SCOPE: &str = "gateway:models:read";

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct VirtualKeyUpdate {
    pub name: Option<String>,
    pub allowed_models: Option<Vec<String>>,
    pub scopes: Option<Vec<String>>,
    /// `Some(None)` explicitly clears the expiry; `None` leaves it unchanged.
    pub expires_at: Option<Option<DateTime<Utc>>>,
    /// `Some(None)` explicitly clears the group; `None` leaves it unchanged.
    pub key_group: Option<Option<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct VirtualKeyRotationOptions {
    pub overlap: Duration,
    pub expires_at: Option<DateTime<Utc>>,
    pub name: Option<String>,
    pub allowed_models: Option<Vec<String>>,
    pub scopes: Option<Vec<String>>,
    pub key_group: Option<Option<String>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VirtualKeyRotation {
    pub old_id: i64,
    pub new_id: i64,
    pub key_prefix: String,
    pub key: String,
    pub overlap_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct VirtualKeyMaterial {
    pub raw: String,
    pub prefix: String,
    hash: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[allow(dead_code)]
pub struct StaticVirtualKeyMigration {
    pub id: i64,
    pub key_prefix: String,
    pub created: bool,
    pub active: bool,
}

#[derive(Debug)]
pub enum VirtualKeyError {
    Database(sqlx::Error),
    NotFound,
    Conflict(String),
    Validation(String),
}

impl fmt::Display for VirtualKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::NotFound => f.write_str("virtual key not found"),
            Self::Conflict(message) | Self::Validation(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for VirtualKeyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for VirtualKeyError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct UsageEventRecord {
    pub request_id: String,
    pub virtual_key_id: Option<i64>,
    pub provider_id: String,
    pub account_id: String,
    pub logical_model: String,
    pub upstream_model_id: Option<String>,
    pub source_id: Option<String>,
    pub client_source: String,
    pub protocol_in: String,
    pub protocol_upstream: String,
    pub mode: String,
    pub status_code: i32,
    pub success: bool,
    pub retry_count: i32,
    pub latency_ms: i64,
    pub ttft_ms: Option<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
    pub usage_source: String,
    pub degraded: bool,
    pub route_id: Option<String>,
    pub streamed: bool,
    pub error_summary: Option<String>,
    pub fallback_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UsageAttempt {
    pub attempt_no: i32,
    pub provider_id: String,
    pub source_id: String,
    pub account_id: String,
    pub upstream_model_id: Option<String>,
    pub status_code: i32,
    pub success: bool,
    pub latency_ms: i64,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct UsageAttemptRecord {
    pub attempt_no: i32,
    pub provider_id: String,
    pub source_id: Option<String>,
    pub account_id: String,
    pub upstream_model_id: Option<String>,
    pub status_code: i32,
    pub success: bool,
    pub latency_ms: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct UsageFilter {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub logical_model: Option<String>,
    pub upstream_model_id: Option<String>,
    pub provider_id: Option<String>,
    pub source_id: Option<String>,
    pub client_source: Option<String>,
    pub account_id: Option<String>,
    pub protocol_in: Option<String>,
    pub protocol_upstream: Option<String>,
    pub virtual_key_id: Option<i64>,
    pub success: Option<bool>,
    pub status_code: Option<i32>,
    pub usage_source: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct UsageAggregate {
    pub logical_requests: i64,
    pub upstream_attempts: i64,
    pub retries: i64,
    pub successes: i64,
    pub failures: i64,
    pub success_rate: f64,
    pub average_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct UsageTimeBucket {
    pub bucket: DateTime<Utc>,
    pub logical_requests: i64,
    pub upstream_attempts: i64,
    pub retries: i64,
    pub successes: i64,
    pub failures: i64,
    pub success_rate: f64,
    pub average_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct UsageBreakdown {
    pub key: Option<String>,
    pub logical_requests: i64,
    pub upstream_attempts: i64,
    pub retries: i64,
    pub successes: i64,
    pub failures: i64,
    pub success_rate: f64,
    pub logical_request_share: f64,
    pub total_token_share: f64,
    pub average_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageCursor {
    pub created_at: DateTime<Utc>,
    pub request_id: String,
}

impl UsageCursor {
    pub fn encode(&self) -> String {
        format!("{}:{}", self.created_at.timestamp_micros(), self.request_id)
    }

    pub fn decode(value: &str) -> Option<Self> {
        let (micros, request_id) = value.split_once(':')?;
        let micros = micros.parse::<i64>().ok()?;
        if request_id.is_empty() {
            return None;
        }
        Some(Self {
            created_at: Utc.timestamp_micros(micros).single()?,
            request_id: request_id.to_string(),
        })
    }
}

#[derive(Debug, serde::Serialize)]
pub struct UsageEventPage {
    pub data: Vec<UsageEventRecord>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

impl Database {
    pub async fn connect_from_env() -> Result<Option<Self>, sqlx::Error> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            return Ok(None);
        };
        Self::connect(&url).await.map(Some)
    }

    pub(crate) async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await?;
        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    #[cfg(test)]
    pub(crate) async fn from_test_pool(pool: PgPool) -> Result<Self, sqlx::Error> {
        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    async fn migrate(&self) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        // The embedded scripts are idempotent; serialize concurrent gateway
        // startups so PostgreSQL does not race while creating the same types.
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(0x6d79_6169_6777_6179_i64)
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../../migrations/0001_init.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../../migrations/0002_control_plane.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../../migrations/0003_model_catalog.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!(
            "../../migrations/0004_usage_query_contract.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!("../../migrations/0005_usage_event_fields.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../../migrations/0006_control_plane_crud.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../../migrations/0007_db_first_runtime.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../../migrations/0008_provider_discovery.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!(
            "../../migrations/0009_usage_source_dimensions.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!(
            "../../migrations/0010_usage_provider_attribution.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!("../../migrations/0011_retention_backup.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../../migrations/0012_health_persistence.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!(
            "../../migrations/0015_virtual_key_lifecycle.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!(
            "../../migrations/0016_virtual_key_recovery.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!(
            "../../migrations/0017_restore_builtin_provider_presets.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!(
            "../../migrations/0018_document_token_count_semantics.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!(
            "../../migrations/0019_usage_fallback_reason.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!("../../migrations/0020_split_cache_tokens.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!(
            "../../migrations/0021_repair_builtin_source_auth_snapshots.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!(
            "../../migrations/0022_health_failure_window.sql"
        ))
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[cfg(test)]
mod config;
mod health;
mod usage;
mod virtual_keys;

pub use virtual_keys::validate_virtual_key_scopes;

#[cfg(test)]
include!("db_tests.rs");
