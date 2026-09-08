#[cfg(test)]
use crate::domain::config::{AccountConfig, GatewayConfig, ProviderConfig, RouteConfig};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{fmt, time::Duration};

#[derive(Clone)]
pub(crate) struct Database {
    pool: PgPool,
}

/// Persisted account health row. `health_updated_at` is independent of the
/// account configuration `updated_at` and is the clock used for stale checks.
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct AccountHealthRow {
    pub(crate) account_id: String,
    pub(crate) enabled: bool,
    pub(crate) source_enabled: bool,
    pub(crate) health_status: String,
    pub(crate) health_source: String,
    pub(crate) cooldown_until: Option<DateTime<Utc>>,
    pub(crate) consecutive_failures: i32,
    pub(crate) failure_window_started_at: Option<DateTime<Utc>>,
    pub(crate) last_error: Option<String>,
    pub(crate) last_success_at: Option<DateTime<Utc>>,
    pub(crate) health_updated_at: Option<DateTime<Utc>>,
    pub(crate) last_probe_at: Option<DateTime<Utc>>,
    pub(crate) last_probe_status: Option<String>,
    pub(crate) last_probe_error: Option<String>,
    pub(crate) account_updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub(crate) struct UsageEvent {
    pub(crate) request_id: String,
    pub(crate) virtual_key_id: Option<i64>,
    pub(crate) provider_id: String,
    pub(crate) account_id: String,
    pub(crate) model: String,
    pub(crate) logical_model: String,
    pub(crate) upstream_model_id: Option<String>,
    pub(crate) source_id: String,
    pub(crate) client_source: String,
    pub(crate) protocol_in: String,
    pub(crate) protocol_upstream: String,
    pub(crate) mode: String,
    pub(crate) status_code: i32,
    pub(crate) success: bool,
    pub(crate) retry_count: i32,
    pub(crate) latency_ms: i64,
    pub(crate) ttft_ms: Option<i64>,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) reasoning_tokens: i64,
    pub(crate) cached_tokens: i64,
    pub(crate) cache_read_tokens: i64,
    pub(crate) cache_creation_tokens: i64,
    pub(crate) total_tokens: i64,
    pub(crate) usage_source: String,
    pub(crate) degraded: bool,
    pub(crate) route_id: Option<String>,
    pub(crate) streamed: bool,
    pub(crate) error_summary: Option<String>,
    pub(crate) fallback_reason: Option<String>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub(crate) struct VirtualKeyRecord {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) key_prefix: String,
    pub(crate) key_recoverable: bool,
    pub(crate) allowed_models: Value,
    pub(crate) scopes: Value,
    pub(crate) key_group: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) last_used_at: Option<DateTime<Utc>>,
    pub(crate) expires_at: Option<DateTime<Utc>>,
    pub(crate) revoked_at: Option<DateTime<Utc>>,
    pub(crate) replaced_by_id: Option<i64>,
    pub(crate) overlap_until: Option<DateTime<Utc>>,
    pub(crate) origin: String,
}

/// The only scope currently consumed by the data plane. Keeping this as a
/// named contract makes adding another operation explicit instead of treating
/// an arbitrary user string as permission.
pub(crate) const VIRTUAL_KEY_INVOKE_SCOPE: &str = "gateway:invoke";
pub(crate) const VIRTUAL_KEY_MODELS_SCOPE: &str = "gateway:models:read";

#[derive(Debug, Clone, Default)]
pub(crate) struct VirtualKeyRotationOptions {
    pub(crate) overlap: Duration,
    pub(crate) expires_at: Option<DateTime<Utc>>,
    pub(crate) name: Option<String>,
    pub(crate) allowed_models: Option<Vec<String>>,
    pub(crate) scopes: Option<Vec<String>>,
    pub(crate) key_group: Option<Option<String>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct VirtualKeyRotation {
    pub(crate) old_id: i64,
    pub(crate) new_id: i64,
    pub(crate) key_prefix: String,
    pub(crate) key: String,
    pub(crate) overlap_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub(crate) struct VirtualKeyMaterial {
    pub(crate) raw: String,
    pub(crate) prefix: String,
    hash: String,
}

#[derive(Debug)]
pub(crate) enum VirtualKeyError {
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
pub(crate) struct UsageEventRecord {
    pub(crate) request_id: String,
    pub(crate) virtual_key_id: Option<i64>,
    pub(crate) provider_id: String,
    pub(crate) account_id: String,
    pub(crate) logical_model: String,
    pub(crate) upstream_model_id: Option<String>,
    pub(crate) source_id: Option<String>,
    pub(crate) client_source: String,
    pub(crate) protocol_in: String,
    pub(crate) protocol_upstream: String,
    pub(crate) mode: String,
    pub(crate) status_code: i32,
    pub(crate) success: bool,
    pub(crate) retry_count: i32,
    pub(crate) latency_ms: i64,
    pub(crate) ttft_ms: Option<i64>,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) reasoning_tokens: i64,
    pub(crate) cached_tokens: i64,
    pub(crate) cache_read_tokens: i64,
    pub(crate) cache_creation_tokens: i64,
    pub(crate) total_tokens: i64,
    pub(crate) usage_source: String,
    pub(crate) degraded: bool,
    pub(crate) route_id: Option<String>,
    pub(crate) streamed: bool,
    pub(crate) error_summary: Option<String>,
    pub(crate) fallback_reason: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub(crate) struct UsageAttempt {
    pub(crate) attempt_no: i32,
    pub(crate) provider_id: String,
    pub(crate) source_id: String,
    pub(crate) account_id: String,
    pub(crate) upstream_model_id: Option<String>,
    pub(crate) status_code: i32,
    pub(crate) success: bool,
    pub(crate) latency_ms: i64,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub(crate) struct UsageAttemptRecord {
    pub(crate) attempt_no: i32,
    pub(crate) provider_id: String,
    pub(crate) source_id: Option<String>,
    pub(crate) account_id: String,
    pub(crate) upstream_model_id: Option<String>,
    pub(crate) status_code: i32,
    pub(crate) success: bool,
    pub(crate) latency_ms: i64,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct UsageFilter {
    pub(crate) from: Option<DateTime<Utc>>,
    pub(crate) to: Option<DateTime<Utc>>,
    pub(crate) logical_model: Option<String>,
    pub(crate) upstream_model_id: Option<String>,
    pub(crate) provider_id: Option<String>,
    pub(crate) source_id: Option<String>,
    pub(crate) client_source: Option<String>,
    pub(crate) account_id: Option<String>,
    pub(crate) protocol_in: Option<String>,
    pub(crate) protocol_upstream: Option<String>,
    pub(crate) virtual_key_id: Option<i64>,
    pub(crate) success: Option<bool>,
    pub(crate) status_code: Option<i32>,
    pub(crate) usage_source: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub(crate) struct UsageAggregate {
    pub(crate) logical_requests: i64,
    pub(crate) upstream_attempts: i64,
    pub(crate) retries: i64,
    pub(crate) successes: i64,
    pub(crate) failures: i64,
    pub(crate) success_rate: f64,
    pub(crate) average_latency_ms: f64,
    pub(crate) p95_latency_ms: f64,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) reasoning_tokens: i64,
    pub(crate) cached_tokens: i64,
    pub(crate) cache_read_tokens: i64,
    pub(crate) cache_creation_tokens: i64,
    pub(crate) total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub(crate) struct UsageTimeBucket {
    pub(crate) bucket: DateTime<Utc>,
    pub(crate) logical_requests: i64,
    pub(crate) upstream_attempts: i64,
    pub(crate) retries: i64,
    pub(crate) successes: i64,
    pub(crate) failures: i64,
    pub(crate) success_rate: f64,
    pub(crate) average_latency_ms: f64,
    pub(crate) p95_latency_ms: f64,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) reasoning_tokens: i64,
    pub(crate) cached_tokens: i64,
    pub(crate) cache_read_tokens: i64,
    pub(crate) cache_creation_tokens: i64,
    pub(crate) total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub(crate) struct UsageBreakdown {
    pub(crate) key: Option<String>,
    pub(crate) logical_requests: i64,
    pub(crate) upstream_attempts: i64,
    pub(crate) retries: i64,
    pub(crate) successes: i64,
    pub(crate) failures: i64,
    pub(crate) success_rate: f64,
    pub(crate) logical_request_share: f64,
    pub(crate) total_token_share: f64,
    pub(crate) average_latency_ms: f64,
    pub(crate) p95_latency_ms: f64,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) reasoning_tokens: i64,
    pub(crate) cached_tokens: i64,
    pub(crate) cache_read_tokens: i64,
    pub(crate) cache_creation_tokens: i64,
    pub(crate) total_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsageCursor {
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) request_id: String,
}

impl UsageCursor {
    pub(crate) fn encode(&self) -> String {
        format!("{}:{}", self.created_at.timestamp_micros(), self.request_id)
    }

    pub(crate) fn decode(value: &str) -> Option<Self> {
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
pub(crate) struct UsageEventPage {
    pub(crate) data: Vec<UsageEventRecord>,
    pub(crate) next_cursor: Option<String>,
    pub(crate) has_more: bool,
}

impl Database {
    pub(crate) async fn connect_from_env() -> Result<Option<Self>, sqlx::Error> {
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
        sqlx::raw_sql(include_str!(
            "../../migrations/0023_kimi_native_responses.sql"
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

pub(crate) use virtual_keys::validate_virtual_key_scopes;

#[cfg(test)]
mod tests;
