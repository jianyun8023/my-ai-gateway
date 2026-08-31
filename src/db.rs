#[cfg(test)]
use crate::config::{AccountConfig, GatewayConfig, ProviderConfig, RouteConfig};
use crate::{model_catalog::ModelCatalogRepository, protocol::Protocol};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;

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
    pub total_tokens: i64,
    pub usage_source: String,
    pub degraded: bool,
    pub route_id: Option<String>,
    pub streamed: bool,
    pub error_summary: Option<String>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct VirtualKeyRecord {
    pub id: i64,
    pub name: String,
    pub key_prefix: String,
    pub allowed_models: Value,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
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
    pub total_tokens: i64,
    pub usage_source: String,
    pub degraded: bool,
    pub route_id: Option<String>,
    pub streamed: bool,
    pub error_summary: Option<String>,
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

const HEALTH_SELECT_ONE: &str = "SELECT a.id AS account_id,a.enabled,s.enabled AS source_enabled,a.health_status,a.health_source,a.cooldown_until,a.consecutive_failures,a.last_error,a.last_success_at,a.health_updated_at,a.last_probe_at,a.last_probe_status,a.last_probe_error,a.updated_at AS account_updated_at FROM accounts a JOIN sources s ON s.id=a.source_id WHERE a.id=$1";
const HEALTH_SELECT_ONE_FOR_UPDATE: &str = "SELECT a.id AS account_id,a.enabled,s.enabled AS source_enabled,a.health_status,a.health_source,a.cooldown_until,a.consecutive_failures,a.last_error,a.last_success_at,a.health_updated_at,a.last_probe_at,a.last_probe_status,a.last_probe_error,a.updated_at AS account_updated_at FROM accounts a JOIN sources s ON s.id=a.source_id WHERE a.id=$1 FOR UPDATE OF a";
const HEALTH_SELECT_ALL: &str = "SELECT a.id AS account_id,a.enabled,s.enabled AS source_enabled,a.health_status,a.health_source,a.cooldown_until,a.consecutive_failures,a.last_error,a.last_success_at,a.health_updated_at,a.last_probe_at,a.last_probe_status,a.last_probe_error,a.updated_at AS account_updated_at FROM accounts a JOIN sources s ON s.id=a.source_id ORDER BY a.id";

fn normalize_health_source(source: &str) -> &str {
    match source {
        "passive" | "probe" | "manual" | "startup" => source,
        _ => "unknown",
    }
}

fn sanitize_health_error(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let safe = matches!(
        value,
        "upstream request failed"
            | "retryable upstream response"
            | "upstream stream failed"
            | "upstream connection failed"
            | "upstream request timed out"
            | "account credential unavailable"
    );
    Some(if safe {
        value.to_owned()
    } else {
        "upstream request failed".into()
    })
}

fn sanitize_health_code(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Some(value.to_owned())
    } else {
        Some("upstream_failure".into())
    }
}

fn exponential_backoff(base: Duration, maximum: Duration, failures: u32) -> Duration {
    if failures == 0 {
        return Duration::ZERO;
    }
    let base_ms = base.as_millis();
    let max_ms = maximum.as_millis().max(base_ms);
    let shift = failures.saturating_sub(1).min(63);
    let multiplier = 1u128.checked_shl(shift).unwrap_or(u128::MAX);
    let delay_ms = base_ms.saturating_mul(multiplier).min(max_ms);
    Duration::from_millis(u64::try_from(delay_ms).unwrap_or(u64::MAX))
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
        sqlx::raw_sql(include_str!("../migrations/0001_init.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0002_control_plane.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0003_model_catalog.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0004_usage_query_contract.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0005_usage_event_fields.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0006_control_plane_crud.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0007_db_first_runtime.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0008_provider_discovery.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!(
            "../migrations/0009_usage_source_dimensions.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!(
            "../migrations/0010_usage_provider_attribution.sql"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(include_str!("../migrations/0011_retention_backup.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0012_health_persistence.sql"))
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Read one persisted account health row. Routing uses this on every
    /// selection so a process restart cannot lose the cooldown state.
    pub async fn account_health(
        &self,
        account_id: &str,
    ) -> Result<Option<AccountHealthRow>, sqlx::Error> {
        sqlx::query_as::<_, AccountHealthRow>(HEALTH_SELECT_ONE)
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn account_health_all(&self) -> Result<Vec<AccountHealthRow>, sqlx::Error> {
        sqlx::query_as::<_, AccountHealthRow>(HEALTH_SELECT_ALL)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn account_source_id(&self, account_id: &str) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT source_id FROM accounts WHERE id=$1")
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await
    }

    /// Return one deterministic enabled protocol per account for periodic
    /// probes. The explicit Admin endpoint can choose another protocol.
    pub async fn health_probe_targets(
        &self,
    ) -> Result<Vec<(String, String, Protocol)>, sqlx::Error> {
        let rows: Vec<(String, String, Value, Value)> = sqlx::query_as(
            "SELECT a.id,a.source_id,s.endpoints,s.protocol_capabilities FROM accounts a JOIN sources s ON s.id=a.source_id WHERE a.enabled AND s.enabled AND s.provider_preset_id <> 'custom' ORDER BY a.id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut targets = Vec::with_capacity(rows.len());
        for (account_id, source_id, endpoints, capabilities) in rows {
            let endpoints =
                serde_json::from_value::<std::collections::BTreeMap<Protocol, String>>(endpoints)
                    .unwrap_or_default();
            let capabilities =
                serde_json::from_value::<std::collections::BTreeMap<Protocol, Value>>(capabilities)
                    .unwrap_or_default();
            let protocol = Protocol::ALL.into_iter().find(|protocol| {
                let has_endpoint = endpoints
                    .get(protocol)
                    .is_some_and(|endpoint| !endpoint.trim().is_empty());
                let supported = capabilities
                    .get(protocol)
                    .and_then(|value| value.get("mode").or(Some(value)))
                    .and_then(Value::as_str)
                    .is_none_or(|mode| !matches!(mode, "unknown" | "unsupported"));
                has_endpoint && supported
            });
            if let Some(protocol) = protocol {
                targets.push((account_id, source_id, protocol));
            }
        }
        Ok(targets)
    }

    pub async fn health_probe_protocol(
        &self,
        account_id: &str,
    ) -> Result<Option<Protocol>, sqlx::Error> {
        Ok(self
            .health_probe_targets()
            .await?
            .into_iter()
            .find(|target| target.0 == account_id)
            .map(|target| target.2))
    }

    /// Atomically increment a failure counter and calculate the next
    /// exponential cooldown. `SELECT ... FOR UPDATE` serializes concurrent
    /// request/probe transitions for the same account.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_account_health_failure(
        &self,
        account_id: &str,
        observed_at: DateTime<Utc>,
        base_cooldown: Duration,
        max_cooldown: Duration,
        source: &str,
        error_code: Option<&str>,
        error_message: Option<&str>,
        latency_ms: Option<i64>,
        connection_test_id: Option<i64>,
    ) -> Result<AccountHealthRow, sqlx::Error> {
        let source = normalize_health_source(source);
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query_as::<_, AccountHealthRow>(HEALTH_SELECT_ONE_FOR_UPDATE)
            .bind(account_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        if !current.enabled || !current.source_enabled {
            tx.commit().await?;
            return Ok(current);
        }
        let failures = current.consecutive_failures.max(0) as u32 + 1;
        let delay = exponential_backoff(base_cooldown, max_cooldown, failures);
        let cooldown_until = observed_at
            + chrono::Duration::from_std(delay).unwrap_or_else(|_| chrono::Duration::zero());
        let status = if current.enabled {
            "cooling_down"
        } else {
            "disabled"
        };
        sqlx::query(
            "UPDATE accounts SET health_status=$2,cooldown_until=$3,last_error=$4,last_success_at=NULL,health_source=$5,health_updated_at=$6,consecutive_failures=$7,last_probe_at=CASE WHEN $5='probe' THEN $6 ELSE last_probe_at END,last_probe_status=CASE WHEN $5='probe' THEN 'failed' ELSE last_probe_status END,last_probe_error=CASE WHEN $5='probe' THEN $4 ELSE last_probe_error END WHERE id=$1",
        )
        .bind(account_id)
        .bind(status)
        .bind(cooldown_until)
        .bind(sanitize_health_error(error_message))
        .bind(source)
        .bind(observed_at)
        .bind(i32::try_from(failures).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO account_health_events (account_id,status,source,observed_at,cooldown_until,consecutive_failures,error_code,error_message,connection_test_id,latency_ms) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(account_id)
        .bind(status)
        .bind(source)
        .bind(observed_at)
        .bind(cooldown_until)
        .bind(i32::try_from(failures).unwrap_or(i32::MAX))
        .bind(sanitize_health_code(error_code))
        .bind(sanitize_health_error(error_message))
        .bind(connection_test_id)
        .bind(latency_ms)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.account_health(account_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn record_account_health_success(
        &self,
        account_id: &str,
        observed_at: DateTime<Utc>,
        source: &str,
        connection_test_id: Option<i64>,
        latency_ms: Option<i64>,
    ) -> Result<AccountHealthRow, sqlx::Error> {
        let source = normalize_health_source(source);
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query_as::<_, AccountHealthRow>(HEALTH_SELECT_ONE_FOR_UPDATE)
            .bind(account_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        if !current.enabled || !current.source_enabled {
            tx.commit().await?;
            return Ok(current);
        }
        let status = if current.enabled {
            "healthy"
        } else {
            "disabled"
        };
        sqlx::query(
            "UPDATE accounts SET health_status=$2,cooldown_until=NULL,last_error=NULL,last_success_at=$3,health_source=$4,health_updated_at=$3,consecutive_failures=0,last_probe_at=CASE WHEN $4='probe' THEN $3 ELSE last_probe_at END,last_probe_status=CASE WHEN $4='probe' THEN 'succeeded' ELSE last_probe_status END,last_probe_error=CASE WHEN $4='probe' THEN NULL ELSE last_probe_error END WHERE id=$1",
        )
        .bind(account_id)
        .bind(status)
        .bind(observed_at)
        .bind(source)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO account_health_events (account_id,status,source,observed_at,cooldown_until,consecutive_failures,connection_test_id,latency_ms) VALUES ($1,$2,$3,$4,NULL,0,$5,$6)",
        )
        .bind(account_id)
        .bind(status)
        .bind(source)
        .bind(observed_at)
        .bind(connection_test_id)
        .bind(latency_ms)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.account_health(account_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    #[allow(dead_code)]
    pub fn model_catalog(&self) -> ModelCatalogRepository {
        ModelCatalogRepository::new(self.pool.clone())
    }

    #[allow(dead_code)]
    pub async fn insert_usage(&self, event: &UsageEvent) -> Result<(), sqlx::Error> {
        self.insert_usage_with_attempts(event, &[]).await
    }

    pub async fn insert_usage_with_attempts(
        &self,
        event: &UsageEvent,
        attempts: &[UsageAttempt],
    ) -> Result<(), sqlx::Error> {
        let now: DateTime<Utc> = Utc::now();
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO usage_events (request_id, virtual_key_id, provider_id, account_id, model, logical_model, upstream_model_id, source_id, client_source, protocol_in, protocol_upstream, mode, status_code, success, retry_count, latency_ms, ttft_ms, input_tokens, output_tokens, reasoning_tokens, cached_tokens, total_tokens, usage_source, degraded, route_id, streamed, error_summary, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28) ON CONFLICT (request_id) DO NOTHING")
            .bind(&event.request_id).bind(event.virtual_key_id).bind(&event.provider_id).bind(&event.account_id).bind(&event.model)
            .bind(&event.logical_model).bind(&event.upstream_model_id).bind(&event.source_id).bind(&event.client_source)
            .bind(&event.protocol_in).bind(&event.protocol_upstream).bind(&event.mode).bind(event.status_code)
            .bind(event.success).bind(event.retry_count).bind(event.latency_ms).bind(event.ttft_ms)
            .bind(event.input_tokens).bind(event.output_tokens).bind(event.reasoning_tokens)
            .bind(event.cached_tokens).bind(event.total_tokens).bind(&event.usage_source).bind(event.degraded)
            .bind(&event.route_id).bind(event.streamed).bind(&event.error_summary).bind(now)
            .execute(&mut *tx).await?;
        for attempt in attempts {
            sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,source_id,account_id,upstream_model_id,status_code,success,latency_ms) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (request_id,attempt_no) DO NOTHING")
                .bind(&event.request_id).bind(attempt.attempt_no).bind(&attempt.provider_id)
                .bind(&attempt.source_id).bind(&attempt.account_id).bind(&attempt.upstream_model_id).bind(attempt.status_code)
                .bind(attempt.success).bind(attempt.latency_ms).execute(&mut *tx).await?;
        }
        tx.commit().await
    }

    #[cfg(test)]
    pub async fn sync_control_plane(&self, config: &GatewayConfig) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        for provider in &config.providers {
            self.upsert_provider_tx(&mut tx, provider).await?;
        }
        for account in &config.accounts {
            self.upsert_account_tx(&mut tx, account).await?;
        }
        for route in &config.routes {
            self.upsert_route_tx(&mut tx, route).await?;
        }
        tx.commit().await
    }

    #[cfg(test)]
    async fn upsert_provider_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        provider: &ProviderConfig,
    ) -> Result<(), sqlx::Error> {
        let endpoints =
            serde_json::to_value(&provider.endpoints).unwrap_or(Value::Object(Default::default()));
        let feature_capabilities = serde_json::to_value(&provider.capabilities)
            .unwrap_or(Value::Object(Default::default()));
        let protocol_capabilities = serde_json::to_value(&provider.protocol_capabilities)
            .unwrap_or(Value::Object(Default::default()));
        let models =
            serde_json::to_value(&provider.models).unwrap_or(Value::Array(Default::default()));
        let native_protocols = serde_json::to_value(&provider.native_protocols)
            .unwrap_or(Value::Array(Default::default()));
        let model_overrides = serde_json::to_value(&provider.model_overrides)
            .unwrap_or(Value::Object(Default::default()));
        sqlx::query(
            "INSERT INTO providers (id,name,base_url,capabilities,endpoints,models,native_protocols,protocol_capabilities,model_overrides) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) \
             ON CONFLICT (id) DO UPDATE SET name=EXCLUDED.name,base_url=EXCLUDED.base_url,\
             capabilities=EXCLUDED.capabilities,endpoints=EXCLUDED.endpoints,\
             models=EXCLUDED.models,native_protocols=EXCLUDED.native_protocols,\
             protocol_capabilities=EXCLUDED.protocol_capabilities,\
             model_overrides=EXCLUDED.model_overrides,updated_at=NOW()"
        )
        .bind(&provider.id)
        .bind(&provider.name)
        .bind(&provider.base_url)
        .bind(&feature_capabilities)
        .bind(&endpoints)
        .bind(&models)
        .bind(&native_protocols)
        .bind(&protocol_capabilities)
        .bind(&model_overrides)
        .execute(&mut **tx)
        .await?;
        let snapshot = serde_json::json!({
            "base_url": provider.base_url,
            "endpoints": endpoints,
            "feature_capabilities": feature_capabilities,
            "protocol_capabilities": protocol_capabilities,
            "native_protocols": provider.native_protocols,
        });
        sqlx::query(
            "INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,protocol_capabilities) \
             VALUES ($1,$2,'custom',1,$3,$4,$5,$6) ON CONFLICT (id) DO NOTHING"
        )
        .bind(&provider.id)
        .bind(&provider.name)
        .bind(snapshot)
        .bind(&provider.base_url)
        .bind(&endpoints)
        .bind(&protocol_capabilities)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    async fn upsert_account_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account: &AccountConfig,
    ) -> Result<(), sqlx::Error> {
        let protocol_capabilities = serde_json::to_value(&account.protocol_capabilities)
            .unwrap_or(Value::Object(Default::default()));
        let capabilities_val = serde_json::to_value(&account.capabilities).ok();
        let model_overrides = serde_json::to_value(&account.model_overrides)
            .unwrap_or(Value::Object(Default::default()));
        let model_map =
            serde_json::to_value(&account.model_map).unwrap_or(Value::Object(Default::default()));
        sqlx::query(
            "INSERT INTO accounts (id,provider_id,source_id,display_name,enabled,weight,protocol_capabilities,capabilities,model_overrides,model_map,credential_env) \
             VALUES ($1,$2,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT (id) DO UPDATE SET provider_id=EXCLUDED.provider_id,\
             display_name=EXCLUDED.display_name,enabled=EXCLUDED.enabled,weight=EXCLUDED.weight,\
             protocol_capabilities=EXCLUDED.protocol_capabilities,\
             capabilities=EXCLUDED.capabilities,model_overrides=EXCLUDED.model_overrides,\
             model_map=EXCLUDED.model_map,credential_env=EXCLUDED.credential_env,updated_at=NOW()"
        )
        .bind(&account.id)
        .bind(&account.provider_id)
        .bind(&account.display_name)
        .bind(account.enabled)
        .bind(account.weight as i32)
        .bind(&protocol_capabilities)
        .bind(&capabilities_val)
        .bind(&model_overrides)
        .bind(&model_map)
        .bind(&account.credential_env)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    async fn upsert_route_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        route: &RouteConfig,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO routes (id,model_pattern,provider_id,protocols,primary_account_id,fallback_accounts,strategy,mode,adapter,allow_lossy_conversion) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT (id) DO UPDATE SET model_pattern=EXCLUDED.model_pattern,\
             provider_id=EXCLUDED.provider_id,protocols=EXCLUDED.protocols,\
             primary_account_id=EXCLUDED.primary_account_id,\
             fallback_accounts=EXCLUDED.fallback_accounts,strategy=EXCLUDED.strategy,\
             mode=EXCLUDED.mode,adapter=EXCLUDED.adapter,\
             allow_lossy_conversion=EXCLUDED.allow_lossy_conversion,updated_at=NOW()"
        )
        .bind(&route.id)
        .bind(&route.model)
        .bind(&route.provider_id)
        .bind(serde_json::to_value(&route.protocols).unwrap_or(Value::Array(vec![])))
        .bind(&route.primary_account_id)
        .bind(serde_json::to_value(&route.fallback_accounts).unwrap_or(Value::Array(vec![])))
        .bind(&route.strategy)
        .bind(&route.mode)
        .bind(&route.adapter)
        .bind(route.allow_lossy_conversion)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn upsert_provider(&self, provider: &ProviderConfig) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        self.upsert_provider_tx(&mut tx, provider).await?;
        tx.commit().await
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn upsert_account(&self, account: &AccountConfig) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        self.upsert_account_tx(&mut tx, account).await?;
        tx.commit().await
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn upsert_route(&self, route: &RouteConfig) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        self.upsert_route_tx(&mut tx, route).await?;
        tx.commit().await
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn delete_provider(&self, id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM providers WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn delete_account(&self, id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM accounts WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn delete_route(&self, id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM routes WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    #[cfg(test)]
    #[allow(clippy::type_complexity, dead_code)]
    pub async fn load_gateway_config(
        &self,
        listen_addr: &str,
    ) -> Result<GatewayConfig, sqlx::Error> {
        let provider_rows: Vec<(String, String, String, bool, Value, Value, Value, Value, Value)> =
            sqlx::query_as(
                "SELECT id,name,base_url,enabled,capabilities,endpoints,models,native_protocols,protocol_capabilities \
                 FROM providers ORDER BY id"
            )
            .fetch_all(&self.pool)
            .await?;
        let mut providers = Vec::with_capacity(provider_rows.len());
        for (
            id,
            name,
            base_url,
            _enabled,
            capabilities_val,
            endpoints_val,
            models_val,
            native_val,
            proto_cap_val,
        ) in provider_rows
        {
            let model_overrides_val: Value = sqlx::query_scalar(
                "SELECT COALESCE(model_overrides, '{}'::jsonb) FROM providers WHERE id=$1",
            )
            .bind(&id)
            .fetch_one(&self.pool)
            .await?;
            providers.push(ProviderConfig {
                id,
                name,
                base_url,
                models: serde_json::from_value(models_val).unwrap_or_default(),
                native_protocols: serde_json::from_value(native_val).unwrap_or_default(),
                endpoints: serde_json::from_value(endpoints_val).unwrap_or_default(),
                capabilities: serde_json::from_value(capabilities_val).unwrap_or_default(),
                protocol_capabilities: serde_json::from_value(proto_cap_val).unwrap_or_default(),
                model_overrides: serde_json::from_value(model_overrides_val).unwrap_or_default(),
            });
        }

        let account_rows: Vec<(String, String, String, bool, i32, Option<String>, Option<String>, Value, Value, Value, Value)> =
            sqlx::query_as(
                "SELECT id,provider_id,display_name,enabled,weight,credential_env,credential_ciphertext,\
                 protocol_capabilities,capabilities,model_overrides,model_map \
                 FROM accounts ORDER BY id"
            )
            .fetch_all(&self.pool)
            .await?;
        let mut accounts = Vec::with_capacity(account_rows.len());
        for (
            id,
            provider_id,
            display_name,
            enabled,
            weight,
            credential_env,
            _ciphertext,
            proto_cap,
            cap_val,
            model_ov,
            model_map_val,
        ) in account_rows
        {
            accounts.push(AccountConfig {
                id,
                provider_id,
                display_name,
                credential_env,
                credential: None,
                enabled,
                weight: weight as u32,
                protocol_capabilities: serde_json::from_value(proto_cap).unwrap_or_default(),
                capabilities: serde_json::from_value(cap_val).ok().flatten(),
                model_overrides: serde_json::from_value(model_ov).unwrap_or_default(),
                model_map: serde_json::from_value(model_map_val).unwrap_or_default(),
            });
        }

        let route_rows: Vec<(
            String,
            String,
            String,
            Value,
            String,
            Value,
            String,
            String,
            Option<String>,
            bool,
        )> = sqlx::query_as(
            "SELECT id,model_pattern,provider_id,protocols,primary_account_id,fallback_accounts,\
                 strategy,mode,adapter,allow_lossy_conversion \
                 FROM routes WHERE enabled=TRUE ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut routes = Vec::with_capacity(route_rows.len());
        for (
            id,
            model,
            provider_id,
            protocols_val,
            primary_account_id,
            fallback_val,
            strategy,
            mode,
            adapter,
            allow_lossy,
        ) in route_rows
        {
            routes.push(RouteConfig {
                id,
                model,
                provider_id,
                protocols: serde_json::from_value(protocols_val).unwrap_or_default(),
                primary_account_id,
                fallback_accounts: serde_json::from_value(fallback_val).unwrap_or_default(),
                strategy,
                mode,
                adapter,
                allow_lossy_conversion: allow_lossy,
            });
        }

        Ok(GatewayConfig {
            listen_addr: listen_addr.to_string(),
            providers,
            accounts,
            routes,
        })
    }

    pub async fn create_virtual_key(
        &self,
        name: &str,
        allowed_models: &[String],
    ) -> Result<(i64, String), sqlx::Error> {
        let raw = format!("gw_{}", uuid::Uuid::new_v4().simple());
        let prefix = raw.chars().take(11).collect::<String>();
        let hash = hash_key(&raw);
        let row = sqlx::query_as::<_, (i64,)>("INSERT INTO virtual_keys (name,key_prefix,key_hash,allowed_models) VALUES ($1,$2,$3,$4) RETURNING id")
            .bind(name).bind(&prefix).bind(&hash).bind(serde_json::to_value(allowed_models).unwrap_or(Value::Array(vec![]))).fetch_one(&self.pool).await?;
        Ok((row.0, raw))
    }

    pub async fn list_virtual_keys(&self) -> Result<Vec<VirtualKeyRecord>, sqlx::Error> {
        sqlx::query_as::<_, VirtualKeyRecord>("SELECT id,name,key_prefix,allowed_models,enabled,created_at,last_used_at,revoked_at FROM virtual_keys ORDER BY id DESC")
            .fetch_all(&self.pool).await
    }

    pub async fn revoke_virtual_key(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE virtual_keys SET enabled=FALSE, revoked_at=NOW() WHERE id=$1 AND enabled=TRUE",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn authenticate_virtual_key(
        &self,
        raw: &str,
        model: &str,
    ) -> Result<Option<i64>, sqlx::Error> {
        let hash = hash_key(raw);
        let row = sqlx::query_as::<_, (i64, bool, Value)>("SELECT id,enabled,allowed_models FROM virtual_keys WHERE key_hash=$1 AND revoked_at IS NULL").bind(&hash).fetch_optional(&self.pool).await?;
        let Some((id, enabled, allowed)) = row else {
            return Ok(None);
        };
        if !enabled {
            return Ok(None);
        }
        let allowed_models = allowed.as_array().cloned().unwrap_or_default();
        let permitted = allowed_models.is_empty()
            || allowed_models
                .iter()
                .any(|item| item.as_str() == Some(model) || item.as_str() == Some("*"));
        if permitted {
            let _ = sqlx::query("UPDATE virtual_keys SET last_used_at=NOW() WHERE key_hash=$1")
                .bind(&hash)
                .execute(&self.pool)
                .await;
        }
        Ok(permitted.then_some(id))
    }

    pub async fn list_usage_events_page(
        &self,
        filter: &UsageFilter,
        limit: i64,
        cursor: Option<&UsageCursor>,
    ) -> Result<UsageEventPage, sqlx::Error> {
        let limit = limit.clamp(1, 500);
        let (mut where_sql, binds) = filter_sql(filter);
        if cursor.is_some() {
            let conjunction = if where_sql.is_empty() { "WHERE" } else { "AND" };
            where_sql.push_str(&format!(
                " {conjunction} (created_at, request_id) < (${}, ${})",
                binds.len() + 1,
                binds.len() + 2
            ));
        }
        let query = format!(
            "{} {where_sql} ORDER BY created_at DESC, request_id DESC LIMIT ${}",
            usage_event_select(),
            binds.len() + if cursor.is_some() { 3 } else { 1 }
        );
        let mut q = sqlx::query_as::<_, UsageEventRecord>(&query);
        q = bind_filter(q, binds);
        if let Some(cursor) = cursor {
            q = q.bind(cursor.created_at).bind(&cursor.request_id);
        }
        let mut data = q.bind(limit + 1).fetch_all(&self.pool).await?;
        let has_more = data.len() as i64 > limit;
        if has_more {
            data.truncate(limit as usize);
        }
        let next_cursor = has_more.then(|| {
            let last = data.last().expect("a page with more rows is non-empty");
            UsageCursor {
                created_at: last.created_at,
                request_id: last.request_id.clone(),
            }
            .encode()
        });
        Ok(UsageEventPage {
            data,
            next_cursor,
            has_more,
        })
    }

    pub async fn export_usage_events(
        &self,
        filter: &UsageFilter,
        limit: i64,
    ) -> Result<Vec<UsageEventRecord>, sqlx::Error> {
        let (where_sql, binds) = filter_sql(filter);
        let query = format!(
            "{} {where_sql} ORDER BY created_at DESC, request_id DESC LIMIT ${}",
            usage_event_select(),
            binds.len() + 1
        );
        let mut q = sqlx::query_as::<_, UsageEventRecord>(&query);
        q = bind_filter(q, binds);
        q.bind(limit).fetch_all(&self.pool).await
    }

    pub async fn get_usage_event_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<UsageEventRecord>, sqlx::Error> {
        sqlx::query_as::<_, UsageEventRecord>(&format!(
            "{} WHERE request_id = $1",
            usage_event_select()
        ))
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn list_attempts_for_event(
        &self,
        request_id: &str,
    ) -> Result<Vec<UsageAttemptRecord>, sqlx::Error> {
        sqlx::query_as::<_, UsageAttemptRecord>("SELECT attempt_no,provider_id,source_id,account_id,upstream_model_id,status_code,success,latency_ms,created_at FROM usage_event_attempts WHERE request_id=$1 ORDER BY attempt_no")
            .bind(request_id)
            .fetch_all(&self.pool)
            .await
    }

    #[cfg(test)]
    pub async fn delete_usage_events_for_test(&self, prefix: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM usage_events WHERE request_id LIKE $1")
            .bind(format!("{prefix}%"))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn usage_aggregate(
        &self,
        filter: &UsageFilter,
    ) -> Result<UsageAggregate, sqlx::Error> {
        let (where_sql, binds) = filter_sql(filter);
        let query = format!(
            "WITH filtered AS (SELECT * FROM usage_events {where_sql}), logical AS (SELECT COUNT(*)::BIGINT AS logical_requests, COALESCE(SUM(retry_count),0)::BIGINT AS retries, COUNT(*) FILTER (WHERE success)::BIGINT AS successes, COUNT(*) FILTER (WHERE NOT success)::BIGINT AS failures, CASE WHEN COUNT(*)=0 THEN 0 ELSE COUNT(*) FILTER (WHERE success)::DOUBLE PRECISION / COUNT(*)::DOUBLE PRECISION END AS success_rate, COALESCE(AVG(latency_ms),0)::DOUBLE PRECISION AS average_latency_ms, COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms),0)::DOUBLE PRECISION AS p95_latency_ms, COALESCE(SUM(input_tokens),0)::BIGINT AS input_tokens, COALESCE(SUM(output_tokens),0)::BIGINT AS output_tokens, COALESCE(SUM(reasoning_tokens),0)::BIGINT AS reasoning_tokens, COALESCE(SUM(cached_tokens),0)::BIGINT AS cached_tokens, COALESCE(SUM(total_tokens),0)::BIGINT AS total_tokens FROM filtered), attempts AS (SELECT COUNT(*)::BIGINT AS upstream_attempts FROM usage_event_attempts a JOIN filtered f ON f.request_id=a.request_id) SELECT logical.logical_requests, attempts.upstream_attempts, logical.retries, logical.successes, logical.failures, logical.success_rate, logical.average_latency_ms, logical.p95_latency_ms, logical.input_tokens, logical.output_tokens, logical.reasoning_tokens, logical.cached_tokens, logical.total_tokens FROM logical CROSS JOIN attempts"
        );
        let mut q = sqlx::query_as::<_, UsageAggregate>(&query);
        q = bind_filter(q, binds);
        q.fetch_one(&self.pool).await
    }

    pub async fn usage_timeseries(
        &self,
        filter: &UsageFilter,
        granularity: &str,
    ) -> Result<Vec<UsageTimeBucket>, sqlx::Error> {
        let trunc = match granularity {
            "day" => "day",
            _ => "hour",
        };
        let (where_sql, binds) = filter_sql(filter);
        let bucket =
            format!("date_trunc('{trunc}', created_at AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'");
        let qualified_bucket = format!(
            "date_trunc('{trunc}', filtered.created_at AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'"
        );
        let query = format!(
            "WITH filtered AS (SELECT * FROM usage_events {where_sql}), logical AS (SELECT {bucket} AS bucket, COUNT(*)::BIGINT AS logical_requests, COALESCE(SUM(retry_count),0)::BIGINT AS retries, COUNT(*) FILTER (WHERE success)::BIGINT AS successes, COUNT(*) FILTER (WHERE NOT success)::BIGINT AS failures, COUNT(*) FILTER (WHERE success)::DOUBLE PRECISION / COUNT(*)::DOUBLE PRECISION AS success_rate, COALESCE(AVG(latency_ms),0)::DOUBLE PRECISION AS average_latency_ms, COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms),0)::DOUBLE PRECISION AS p95_latency_ms, COALESCE(SUM(input_tokens),0)::BIGINT AS input_tokens, COALESCE(SUM(output_tokens),0)::BIGINT AS output_tokens, COALESCE(SUM(reasoning_tokens),0)::BIGINT AS reasoning_tokens, COALESCE(SUM(cached_tokens),0)::BIGINT AS cached_tokens, COALESCE(SUM(total_tokens),0)::BIGINT AS total_tokens FROM filtered GROUP BY 1), attempts AS (SELECT {qualified_bucket} AS bucket, COUNT(*)::BIGINT AS upstream_attempts FROM filtered JOIN usage_event_attempts USING (request_id) GROUP BY 1) SELECT logical.bucket, logical.logical_requests, COALESCE(attempts.upstream_attempts,0)::BIGINT AS upstream_attempts, logical.retries, logical.successes, logical.failures, logical.success_rate, logical.average_latency_ms, logical.p95_latency_ms, logical.input_tokens, logical.output_tokens, logical.reasoning_tokens, logical.cached_tokens, logical.total_tokens FROM logical LEFT JOIN attempts USING (bucket) ORDER BY logical.bucket"
        );
        let mut q = sqlx::query_as::<_, UsageTimeBucket>(&query);
        q = bind_filter(q, binds);
        q.fetch_all(&self.pool).await
    }

    pub async fn usage_breakdown(
        &self,
        filter: &UsageFilter,
        dimension: &str,
    ) -> Result<Vec<UsageBreakdown>, sqlx::Error> {
        let (column, qualified_column) = match dimension {
            "logical_model" => ("logical_model", "f.logical_model"),
            "upstream_model" => ("upstream_model_id", "f.upstream_model_id"),
            "provider" => ("provider_id", "f.provider_id"),
            "source_id" => ("source_id", "f.source_id"),
            "client_source" => ("client_source", "f.client_source"),
            "account" => ("account_id", "f.account_id"),
            "protocol_in" => ("protocol_in", "f.protocol_in"),
            "protocol_upstream" => ("protocol_upstream", "f.protocol_upstream"),
            "virtual_key" => ("virtual_key_id::TEXT", "f.virtual_key_id::TEXT"),
            "status" => (
                "CASE WHEN success THEN 'success' ELSE 'failure' END",
                "CASE WHEN f.success THEN 'success' ELSE 'failure' END",
            ),
            "usage_source" => ("usage_source", "f.usage_source"),
            _ => ("logical_model", "f.logical_model"),
        };
        let (where_sql, binds) = filter_sql(filter);
        let query = format!(
            "WITH filtered AS (SELECT * FROM usage_events {where_sql}), logical AS (SELECT {column} AS key, COUNT(*)::BIGINT AS logical_requests, COALESCE(SUM(retry_count),0)::BIGINT AS retries, COUNT(*) FILTER (WHERE success)::BIGINT AS successes, COUNT(*) FILTER (WHERE NOT success)::BIGINT AS failures, COUNT(*) FILTER (WHERE success)::DOUBLE PRECISION / COUNT(*)::DOUBLE PRECISION AS success_rate, COALESCE(AVG(latency_ms),0)::DOUBLE PRECISION AS average_latency_ms, COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms),0)::DOUBLE PRECISION AS p95_latency_ms, COALESCE(SUM(input_tokens),0)::BIGINT AS input_tokens, COALESCE(SUM(output_tokens),0)::BIGINT AS output_tokens, COALESCE(SUM(reasoning_tokens),0)::BIGINT AS reasoning_tokens, COALESCE(SUM(cached_tokens),0)::BIGINT AS cached_tokens, COALESCE(SUM(total_tokens),0)::BIGINT AS total_tokens FROM filtered GROUP BY {column}), attempts AS (SELECT {qualified_column} AS key, COUNT(*)::BIGINT AS upstream_attempts FROM filtered f JOIN usage_event_attempts a ON f.request_id=a.request_id GROUP BY {qualified_column}) SELECT logical.key, logical.logical_requests, COALESCE(attempts.upstream_attempts,0)::BIGINT AS upstream_attempts, logical.retries, logical.successes, logical.failures, logical.success_rate, logical.logical_requests::DOUBLE PRECISION / SUM(logical.logical_requests) OVER ()::DOUBLE PRECISION AS logical_request_share, CASE WHEN SUM(logical.total_tokens) OVER ()=0 THEN 0 ELSE logical.total_tokens::DOUBLE PRECISION / SUM(logical.total_tokens) OVER ()::DOUBLE PRECISION END AS total_token_share, logical.average_latency_ms, logical.p95_latency_ms, logical.input_tokens, logical.output_tokens, logical.reasoning_tokens, logical.cached_tokens, logical.total_tokens FROM logical LEFT JOIN attempts ON logical.key IS NOT DISTINCT FROM attempts.key ORDER BY logical.logical_requests DESC, logical.key ASC NULLS LAST"
        );
        let mut q = sqlx::query_as::<_, UsageBreakdown>(&query);
        q = bind_filter(q, binds);
        q.fetch_all(&self.pool).await
    }
}

fn usage_event_select() -> &'static str {
    "SELECT request_id,virtual_key_id,provider_id,account_id,logical_model,upstream_model_id,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,ttft_ms,input_tokens,output_tokens,reasoning_tokens,cached_tokens,total_tokens,usage_source,degraded,route_id,streamed,error_summary,created_at FROM usage_events"
}

fn filter_sql(filter: &UsageFilter) -> (String, Vec<FilterBind>) {
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    if let Some(value) = filter.from {
        clauses.push(format!("created_at >= ${}", binds.len() + 1));
        binds.push(FilterBind::Time(value));
    }
    if let Some(value) = filter.to {
        clauses.push(format!("created_at < ${}", binds.len() + 1));
        binds.push(FilterBind::Time(value));
    }
    for (column, value) in [
        ("logical_model", &filter.logical_model),
        ("upstream_model_id", &filter.upstream_model_id),
        ("provider_id", &filter.provider_id),
        ("source_id", &filter.source_id),
        ("client_source", &filter.client_source),
        ("account_id", &filter.account_id),
        ("protocol_in", &filter.protocol_in),
        ("protocol_upstream", &filter.protocol_upstream),
        ("usage_source", &filter.usage_source),
    ] {
        if let Some(value) = value {
            clauses.push(format!("{column} = ${}", binds.len() + 1));
            binds.push(FilterBind::Text(value.clone()));
        }
    }
    if let Some(value) = filter.virtual_key_id {
        clauses.push(format!("virtual_key_id = ${}", binds.len() + 1));
        binds.push(FilterBind::I64(value));
    }
    if let Some(value) = filter.success {
        clauses.push(format!("success = ${}", binds.len() + 1));
        binds.push(FilterBind::Bool(value));
    }
    if let Some(value) = filter.status_code {
        clauses.push(format!("status_code = ${}", binds.len() + 1));
        binds.push(FilterBind::I32(value));
    }
    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (sql, binds)
}

enum FilterBind {
    Time(DateTime<Utc>),
    Text(String),
    I64(i64),
    I32(i32),
    Bool(bool),
}
fn bind_filter<'q, O>(
    mut query: sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments>,
    binds: Vec<FilterBind>,
) -> sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments>
where
    O: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    for bind in binds {
        query = match bind {
            FilterBind::Time(value) => query.bind(value),
            FilterBind::Text(value) => query.bind(value),
            FilterBind::I64(value) => query.bind(value),
            FilterBind::I32(value) => query.bind(value),
            FilterBind::Bool(value) => query.bind(value),
        };
    }
    query
}

fn hash_key(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model_catalog::{
            CapabilitySupport, CatalogMetadata, CatalogStatus, LogicalModelInput, MetadataField,
            MetadataSource, MetadataValues, ModelBindingInput, ModelCatalogRepository,
            ModelPresetInput, ModelPresetRef, ProviderPresetInput, SourceInput,
            SourceModelCapabilityInput, SourceModelRefresh, SourceProtocolMode,
        },
        protocol::Protocol,
    };
    use serde_json::json;
    use sqlx::postgres::PgConnectOptions;
    use std::{collections::BTreeMap, str::FromStr};

    #[test]
    fn filters_support_combined_dimensions_and_utc_bounds() {
        let filter = UsageFilter {
            from: Some("2026-01-01T00:00:00Z".parse().unwrap()),
            to: Some("2026-01-02T00:00:00Z".parse().unwrap()),
            logical_model: Some("m".into()),
            upstream_model_id: Some("upstream-m".into()),
            provider_id: Some("p".into()),
            source_id: Some("source-a".into()),
            client_source: Some("cli".into()),
            account_id: Some("a".into()),
            protocol_in: Some("openai_chat_completions".into()),
            protocol_upstream: Some("anthropic_messages".into()),
            virtual_key_id: Some(7),
            success: Some(false),
            status_code: Some(429),
            usage_source: Some("estimated".into()),
        };
        let (sql, binds) = filter_sql(&filter);
        assert!(sql.contains("created_at >= $1"));
        assert!(sql.contains("logical_model = $3"));
        assert!(sql.contains("source_id = $6"));
        assert!(sql.contains("client_source = $7"));
        assert!(sql.contains("protocol_upstream = $10"));
        assert!(sql.contains("virtual_key_id = $12"));
        assert!(sql.contains("status_code = $14"));
        assert_eq!(binds.len(), 14);
    }

    #[test]
    fn cursor_round_trip_preserves_tie_breaker() {
        let cursor = UsageCursor {
            created_at: "2026-01-01T00:00:00.123456Z".parse().unwrap(),
            request_id: "request:with:colons".into(),
        };
        assert_eq!(UsageCursor::decode(&cursor.encode()), Some(cursor));
        assert!(UsageCursor::decode("not-a-cursor").is_none());
    }

    #[test]
    fn initial_schema_keeps_logical_request_and_attempt_idempotency() {
        let schema = include_str!("../migrations/0001_init.sql");
        assert!(schema.contains("logical_model TEXT NOT NULL"));
        assert!(schema.contains("UNIQUE (request_id, attempt_no)"));
        assert!(schema.contains("request_id TEXT NOT NULL UNIQUE"));
        let query_schema = include_str!("../migrations/0004_usage_query_contract.sql");
        assert!(query_schema.contains("virtual_key_id BIGINT"));
        assert!(query_schema.contains("created_at DESC, request_id DESC"));
        let fields_schema = include_str!("../migrations/0005_usage_event_fields.sql");
        assert!(fields_schema.contains("route_id TEXT"));
        assert!(fields_schema.contains("streamed BOOLEAN"));
        assert!(fields_schema.contains("error_summary TEXT"));
        let source_schema = include_str!("../migrations/0009_usage_source_dimensions.sql");
        assert!(source_schema.contains("RENAME COLUMN source TO client_source"));
        assert!(source_schema.contains("ADD COLUMN IF NOT EXISTS source_id TEXT"));
        assert!(!source_schema.contains("REFERENCES sources"));
        let provider_schema = include_str!("../migrations/0010_usage_provider_attribution.sql");
        assert!(provider_schema.contains("source.provider_preset_id"));
        assert!(provider_schema.contains("provider_id = event.source_id"));
        assert!(provider_schema.contains("provider_id = attempt.source_id"));
        assert!(provider_schema.contains("'unknown'"));
        let health_schema = include_str!("../migrations/0012_health_persistence.sql");
        for marker in [
            "health_source TEXT",
            "health_updated_at TIMESTAMPTZ",
            "consecutive_failures INTEGER",
            "CREATE TABLE IF NOT EXISTS account_health_events",
            "VALUES (12, 'health_persistence')",
        ] {
            assert!(
                health_schema.contains(marker),
                "missing health marker: {marker}"
            );
        }
    }

    #[tokio::test]
    async fn postgres_migrates_legacy_client_source_without_inventing_runtime_source() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("skipping PostgreSQL usage migration test: TEST_DATABASE_URL is not set");
            return;
        };
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect PostgreSQL migration test admin database");
        let schema = format!("usage_source_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create isolated usage migration schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse TEST_DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .expect("connect isolated usage migration schema");

        sqlx::raw_sql(include_str!("../migrations/0001_init.sql"))
            .execute(&pool)
            .await
            .expect("apply legacy usage schema");
        sqlx::query("CREATE INDEX idx_usage_events_source_created_at ON usage_events (source, created_at DESC)")
            .execute(&pool)
            .await
            .expect("create legacy source index");
        sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,source,protocol_in,protocol_upstream,mode,status_code,success) VALUES ('legacy-request','legacy-provider','legacy-account','legacy-model','legacy-model','legacy-cli','openai_responses','openai_responses','native',200,TRUE)")
            .execute(&pool)
            .await
            .expect("insert legacy usage event");
        sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,account_id,status_code,success) VALUES ('legacy-request',0,'legacy-provider','legacy-account',200,TRUE)")
            .execute(&pool)
            .await
            .expect("insert legacy usage attempt");

        sqlx::raw_sql(include_str!(
            "../migrations/0009_usage_source_dimensions.sql"
        ))
        .execute(&pool)
        .await
        .expect("apply Source dimension migration");
        // The gateway embeds idempotent scripts and replays them at startup.
        sqlx::raw_sql(include_str!("../migrations/0004_usage_query_contract.sql"))
            .execute(&pool)
            .await
            .expect("replay earlier usage indexes after migration");
        sqlx::raw_sql(include_str!(
            "../migrations/0009_usage_source_dimensions.sql"
        ))
        .execute(&pool)
        .await
        .expect("replay Source dimension migration");

        let event: (Option<String>, String) = sqlx::query_as(
            "SELECT source_id,client_source FROM usage_events WHERE request_id='legacy-request'",
        )
        .fetch_one(&pool)
        .await
        .expect("read migrated usage event");
        assert_eq!(event, (None, "legacy-cli".into()));
        let attempt_source: Option<String> = sqlx::query_scalar(
            "SELECT source_id FROM usage_event_attempts WHERE request_id='legacy-request'",
        )
        .fetch_one(&pool)
        .await
        .expect("read migrated usage attempt");
        assert_eq!(attempt_source, None);
        let legacy_column_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema=current_schema() AND table_name='usage_events' AND column_name='source')",
        )
        .fetch_one(&pool)
        .await
        .expect("inspect migrated usage columns");
        assert!(!legacy_column_exists);

        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop isolated usage migration schema");
        admin.close().await;
    }

    #[tokio::test]
    async fn postgres_repairs_db_first_provider_attribution_without_guessing_deleted_sources() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("skipping PostgreSQL provider attribution migration test: TEST_DATABASE_URL is not set");
            return;
        };
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect PostgreSQL provider attribution migration admin database");
        let schema = format!("usage_provider_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create isolated provider attribution migration schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse TEST_DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .expect("connect isolated provider attribution migration schema");
        let database = Database::from_test_pool(pool.clone())
            .await
            .expect("apply migrations in provider attribution schema");

        sqlx::query("INSERT INTO provider_presets (id,version,display_name,definition) VALUES ('provider-a',1,'Provider A','{}'::jsonb)")
            .execute(&pool)
            .await
            .expect("insert provider preset fixture");
        sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url) VALUES ('source-a','Source A','provider-a',1,'{}'::jsonb,'https://source-a.example'),('source-b','Source B','provider-a',1,'{}'::jsonb,'https://source-b.example')")
            .execute(&pool)
            .await
            .expect("insert Source fixtures");
        sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success) VALUES ('mapped-a','source-a','account-a','model','model','source-a','test','openai_responses','openai_responses','native',200,TRUE),('mapped-b','source-b','account-b','model','model','source-b','test','openai_responses','openai_responses','native',200,TRUE),('deleted','deleted-source','account-deleted','model','model','deleted-source','test','openai_responses','openai_responses','native',200,TRUE),('legacy','legacy-provider','legacy-account','model','model',NULL,'test','openai_responses','openai_responses','native',200,TRUE)")
            .execute(&pool)
            .await
            .expect("insert provider attribution event fixtures");
        sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,source_id,account_id,status_code,success) VALUES ('mapped-a',0,'source-a','source-a','account-a',200,TRUE),('mapped-b',0,'source-b','source-b','account-b',200,TRUE),('deleted',0,'deleted-source','deleted-source','account-deleted',200,TRUE),('legacy',0,'legacy-provider',NULL,'legacy-account',200,TRUE)")
            .execute(&pool)
            .await
            .expect("insert provider attribution attempt fixtures");

        for _ in 0..2 {
            sqlx::raw_sql(include_str!(
                "../migrations/0010_usage_provider_attribution.sql"
            ))
            .execute(&pool)
            .await
            .expect("replay provider attribution migration");
        }

        let events: Vec<(String, String)> =
            sqlx::query_as("SELECT request_id,provider_id FROM usage_events ORDER BY request_id")
                .fetch_all(&pool)
                .await
                .expect("query repaired provider event attribution");
        assert_eq!(
            events,
            vec![
                ("deleted".into(), "unknown".into()),
                ("legacy".into(), "legacy-provider".into()),
                ("mapped-a".into(), "provider-a".into()),
                ("mapped-b".into(), "provider-a".into()),
            ]
        );
        let attempts: Vec<(String, String)> = sqlx::query_as(
            "SELECT request_id,provider_id FROM usage_event_attempts ORDER BY request_id",
        )
        .fetch_all(&pool)
        .await
        .expect("query repaired provider attempt attribution");
        assert_eq!(attempts, events);

        drop(database);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop isolated provider attribution migration schema");
        admin.close().await;
    }

    async fn postgres_test_database() -> Option<Database> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connect TEST_DATABASE_URL");
        let database = Database { pool };
        database.migrate().await.expect("apply test migrations");
        Some(database)
    }

    #[tokio::test]
    async fn postgres_queries_keep_logical_attempt_and_utc_boundary_semantics() {
        let Some(database) = postgres_test_database().await else {
            eprintln!("skipping PostgreSQL usage query test: TEST_DATABASE_URL is not set");
            return;
        };
        let prefix = format!("usage-contract-{}-", uuid::Uuid::new_v4());
        let logical_model = format!("logical-{prefix}");
        let empty_filter = UsageFilter {
            logical_model: Some(format!("missing-{prefix}")),
            ..Default::default()
        };
        let empty = database
            .usage_aggregate(&empty_filter)
            .await
            .expect("empty summary");
        assert_eq!(empty.logical_requests, 0);
        assert_eq!(empty.upstream_attempts, 0);
        assert_eq!(empty.total_tokens, 0);
        assert!(database
            .usage_timeseries(&empty_filter, "hour")
            .await
            .expect("empty timeseries")
            .is_empty());
        assert!(database
            .usage_breakdown(&empty_filter, "logical_model")
            .await
            .expect("empty breakdown")
            .is_empty());
        assert!(database
            .list_usage_events_page(&empty_filter, 100, None)
            .await
            .expect("empty event page")
            .data
            .is_empty());
        let (virtual_key_id, virtual_key) = database
            .create_virtual_key(&format!("key-{prefix}"), &[])
            .await
            .expect("create virtual key fixture");
        assert_eq!(
            database
                .authenticate_virtual_key(&virtual_key, &logical_model)
                .await
                .expect("authenticate virtual key fixture"),
            Some(virtual_key_id)
        );
        let fixtures = [
            ("a", "2026-01-01T00:00:00Z", true, "upstream", 10_i64, 1_i32),
            (
                "b",
                "2026-01-01T23:59:59.999999Z",
                false,
                "missing",
                0_i64,
                0_i32,
            ),
            (
                "c",
                "2026-01-02T00:00:00Z",
                true,
                "estimated",
                30_i64,
                0_i32,
            ),
        ];
        for (suffix, created_at, success, usage_source, tokens, retry_count) in fixtures {
            let request_id = format!("{prefix}{suffix}");
            sqlx::query("INSERT INTO usage_events (request_id,virtual_key_id,provider_id,account_id,model,logical_model,upstream_model_id,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,input_tokens,output_tokens,total_tokens,usage_source,created_at) VALUES ($1,$2,'provider-a','account-a',$3,$3,'upstream-a','source-a','test','openai_responses','anthropic_messages','adapter',$4,$5,$6,25,$7,0,$7,$8,$9)")
                .bind(&request_id)
                .bind(virtual_key_id)
                .bind(&logical_model)
                .bind(if success { 200 } else { 429 })
                .bind(success)
                .bind(retry_count)
                .bind(tokens)
                .bind(usage_source)
                .bind(created_at.parse::<DateTime<Utc>>().unwrap())
                .execute(&database.pool)
                .await
                .expect("insert usage fixture");
            for attempt_no in 0..=retry_count {
                sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,source_id,account_id,status_code,success,latency_ms) VALUES ($1,$2,'provider-a','source-a','account-a',$3,$4,10)")
                    .bind(&request_id)
                    .bind(attempt_no)
                    .bind(if success { 200 } else { 429 })
                    .bind(success)
                    .execute(&database.pool)
                    .await
                    .expect("insert attempt fixture");
            }
        }
        let filter = UsageFilter {
            from: Some("2026-01-01T00:00:00Z".parse().unwrap()),
            to: Some("2026-01-02T00:00:00Z".parse().unwrap()),
            logical_model: Some(logical_model.clone()),
            upstream_model_id: Some("upstream-a".into()),
            provider_id: Some("provider-a".into()),
            source_id: Some("source-a".into()),
            client_source: Some("test".into()),
            account_id: Some("account-a".into()),
            protocol_in: Some("openai_responses".into()),
            protocol_upstream: Some("anthropic_messages".into()),
            virtual_key_id: Some(virtual_key_id),
            ..Default::default()
        };
        let summary = database.usage_aggregate(&filter).await.expect("summary");
        assert_eq!(summary.logical_requests, 2);
        assert_eq!(summary.upstream_attempts, 3);
        assert_eq!(summary.retries, 1);
        assert_eq!(summary.successes, 1);
        assert_eq!(summary.failures, 1);
        assert_eq!(summary.total_tokens, 10);
        let mut failed_filter = filter.clone();
        failed_filter.success = Some(false);
        failed_filter.status_code = Some(429);
        failed_filter.usage_source = Some("missing".into());
        let failed = database
            .usage_aggregate(&failed_filter)
            .await
            .expect("failure and missing-usage filter");
        assert_eq!(failed.logical_requests, 1);
        assert_eq!(failed.failures, 1);
        assert_eq!(failed.total_tokens, 0);
        let timeseries = database
            .usage_timeseries(&filter, "day")
            .await
            .expect("timeseries");
        assert_eq!(timeseries.len(), 1);
        assert_eq!(timeseries[0].upstream_attempts, 3);
        let breakdown = database
            .usage_breakdown(&filter, "usage_source")
            .await
            .expect("breakdown");
        assert_eq!(breakdown.len(), 2);
        let source_breakdown = database
            .usage_breakdown(&filter, "source_id")
            .await
            .expect("Source breakdown");
        assert_eq!(source_breakdown[0].key.as_deref(), Some("source-a"));
        let client_source_breakdown = database
            .usage_breakdown(&filter, "client_source")
            .await
            .expect("Client Source breakdown");
        assert_eq!(client_source_breakdown[0].key.as_deref(), Some("test"));
        let exported = database
            .export_usage_events(&filter, 10_000)
            .await
            .expect("export");
        assert_eq!(exported.len(), 2);
        sqlx::query("DELETE FROM usage_events WHERE request_id LIKE $1")
            .bind(format!("{prefix}%"))
            .execute(&database.pool)
            .await
            .expect("clean usage fixtures");
        sqlx::query("DELETE FROM virtual_keys WHERE id=$1")
            .bind(virtual_key_id)
            .execute(&database.pool)
            .await
            .expect("clean virtual key fixture");
    }

    #[tokio::test]
    async fn postgres_parsed_usage_source_is_filterable() {
        let Some(database) = postgres_test_database().await else {
            eprintln!("skipping PostgreSQL parsed usage test: TEST_DATABASE_URL is not set");
            return;
        };
        let prefix = format!("usage-parsed-{}-", uuid::Uuid::new_v4());
        let logical_model = format!("logical-{prefix}");
        let event = UsageEvent {
            request_id: format!("{prefix}request"),
            virtual_key_id: None,
            provider_id: "provider-parsed".into(),
            account_id: "account-parsed".into(),
            model: logical_model.clone(),
            logical_model: logical_model.clone(),
            upstream_model_id: Some("upstream-parsed".into()),
            source_id: "source-parsed".into(),
            client_source: "test".into(),
            protocol_in: "openai_responses".into(),
            protocol_upstream: "anthropic_messages".into(),
            mode: "adapter".into(),
            status_code: 200,
            success: true,
            retry_count: 0,
            latency_ms: 12,
            ttft_ms: None,
            input_tokens: 2,
            output_tokens: 3,
            reasoning_tokens: 0,
            cached_tokens: 0,
            total_tokens: 5,
            usage_source: "parsed".into(),
            degraded: false,
            route_id: Some("route-parsed".into()),
            streamed: true,
            error_summary: None,
        };
        database
            .insert_usage(&event)
            .await
            .expect("insert parsed usage fixture");

        let filter = UsageFilter {
            logical_model: Some(logical_model),
            usage_source: Some("parsed".into()),
            ..Default::default()
        };
        let page = database
            .list_usage_events_page(&filter, 10, None)
            .await
            .expect("query parsed usage events");
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].usage_source, "parsed");
        let aggregate = database
            .usage_aggregate(&filter)
            .await
            .expect("aggregate parsed usage events");
        assert_eq!(aggregate.logical_requests, 1);
        assert_eq!(aggregate.total_tokens, 5);
        let breakdown = database
            .usage_breakdown(&filter, "usage_source")
            .await
            .expect("break down parsed usage events");
        assert_eq!(breakdown.len(), 1);
        assert_eq!(breakdown[0].key.as_deref(), Some("parsed"));

        database
            .delete_usage_events_for_test(&prefix)
            .await
            .expect("clean parsed usage fixture");
    }

    #[tokio::test]
    async fn postgres_source_deletion_preserves_usage_history() {
        let Some(database) = postgres_test_database().await else {
            eprintln!("skipping PostgreSQL Source history test: TEST_DATABASE_URL is not set");
            return;
        };
        let suffix = uuid::Uuid::new_v4().to_string();
        let source_id = format!("deleted-source-{suffix}");
        let request_id = format!("deleted-source-request-{suffix}");
        sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url) VALUES ($1,$2,'custom',1,'{}'::jsonb,'https://deleted-source.example')")
            .bind(&source_id)
            .bind(format!("Deleted Source {suffix}"))
            .execute(&database.pool)
            .await
            .expect("insert Source history fixture");
        let event = UsageEvent {
            request_id: request_id.clone(),
            virtual_key_id: None,
            provider_id: source_id.clone(),
            account_id: format!("deleted-account-{suffix}"),
            model: "history-model".into(),
            logical_model: "history-model".into(),
            upstream_model_id: Some("history-upstream".into()),
            source_id: source_id.clone(),
            client_source: "history-client".into(),
            protocol_in: "openai_chat_completions".into(),
            protocol_upstream: "openai_chat_completions".into(),
            mode: "native".into(),
            status_code: 200,
            success: true,
            retry_count: 0,
            latency_ms: 5,
            ttft_ms: None,
            input_tokens: 1,
            output_tokens: 1,
            reasoning_tokens: 0,
            cached_tokens: 0,
            total_tokens: 2,
            usage_source: "upstream".into(),
            degraded: false,
            route_id: Some("history-route".into()),
            streamed: false,
            error_summary: None,
        };
        database
            .insert_usage_with_attempts(
                &event,
                &[UsageAttempt {
                    attempt_no: 0,
                    provider_id: source_id.clone(),
                    source_id: source_id.clone(),
                    account_id: format!("deleted-account-{suffix}"),
                    upstream_model_id: Some("history-upstream".into()),
                    status_code: 200,
                    success: true,
                    latency_ms: 5,
                }],
            )
            .await
            .expect("insert Source history usage fixture");

        sqlx::query("DELETE FROM sources WHERE id=$1")
            .bind(&source_id)
            .execute(&database.pool)
            .await
            .expect("delete control-plane Source");

        let persisted = database
            .get_usage_event_detail(&request_id)
            .await
            .expect("query usage after Source deletion")
            .expect("usage history survives Source deletion");
        assert_eq!(persisted.source_id.as_deref(), Some(source_id.as_str()));
        assert_eq!(persisted.client_source, "history-client");
        let attempts = database
            .list_attempts_for_event(&request_id)
            .await
            .expect("query attempts after Source deletion");
        assert_eq!(attempts[0].source_id.as_deref(), Some(source_id.as_str()));

        sqlx::query("DELETE FROM usage_events WHERE request_id=$1")
            .bind(&request_id)
            .execute(&database.pool)
            .await
            .expect("clean Source history usage fixture");
    }

    #[tokio::test]
    async fn postgres_cursor_handles_large_pages_without_duplicates_or_omissions() {
        let Some(database) = postgres_test_database().await else {
            eprintln!("skipping PostgreSQL pagination test: TEST_DATABASE_URL is not set");
            return;
        };
        let prefix = format!("usage-page-{}-", uuid::Uuid::new_v4());
        let logical_model = format!("logical-{prefix}");
        sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,usage_source,created_at) SELECT $1 || LPAD(i::TEXT,4,'0'),'provider-page','account-page',$2,$2,'source-page','test','openai_responses','openai_responses','native',200,TRUE,0,1,'missing','2026-02-01T00:00:00Z'::TIMESTAMPTZ FROM generate_series(1,503) AS i")
            .bind(&prefix)
            .bind(&logical_model)
            .execute(&database.pool)
            .await
            .expect("insert pagination fixtures");
        let filter = UsageFilter {
            logical_model: Some(logical_model),
            ..Default::default()
        };
        let first = database
            .list_usage_events_page(&filter, 500, None)
            .await
            .expect("first page");
        assert_eq!(first.data.len(), 500);
        assert!(first.has_more);
        let cursor = UsageCursor::decode(first.next_cursor.as_deref().unwrap()).unwrap();
        let second = database
            .list_usage_events_page(&filter, 500, Some(&cursor))
            .await
            .expect("second page");
        assert_eq!(second.data.len(), 3);
        assert!(!second.has_more);
        let ids = first
            .data
            .iter()
            .chain(&second.data)
            .map(|event| event.request_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 503);
        sqlx::query("DELETE FROM usage_events WHERE request_id LIKE $1")
            .bind(format!("{prefix}%"))
            .execute(&database.pool)
            .await
            .expect("clean pagination fixtures");
    }

    #[test]
    fn model_catalog_schema_declares_required_keys_and_routability_guard() {
        let schema = include_str!("../migrations/0003_model_catalog.sql");
        assert!(schema.contains("PRIMARY KEY (source_id, upstream_model_id, protocol)"));
        assert!(schema.contains(
            "UNIQUE (logical_model_id, source_id, account_id, upstream_model_id, protocol)"
        ));
        assert!(schema.contains("provider_preset_snapshot JSONB NOT NULL"));
        assert!(schema.contains("confirmed model binding is not routable"));
        let discovery_schema = include_str!("../migrations/0008_provider_discovery.sql");
        assert!(discovery_schema.contains("CREATE TABLE IF NOT EXISTS source_discovery_runs"));
        assert!(discovery_schema.contains("CREATE TABLE IF NOT EXISTS source_connection_tests"));
        assert!(discovery_schema.contains("raw_snapshot JSONB"));
        assert!(discovery_schema.contains("error_message TEXT"));
    }

    #[test]
    fn retention_schema_declares_independent_policies_and_operation_state() {
        let schema = include_str!("../migrations/0011_retention_backup.sql");
        for marker in [
            "CREATE TABLE IF NOT EXISTS retention_policies",
            "CREATE TABLE IF NOT EXISTS retention_cleanup_runs",
            "CREATE TABLE IF NOT EXISTS audit_logs",
            "CREATE TABLE IF NOT EXISTS backup_runs",
            "CREATE TABLE IF NOT EXISTS gateway_schema_migrations",
            "CREATE TABLE IF NOT EXISTS gateway_schema_metadata",
            "policy_key IN ('usage_events', 'usage_attempts', 'audit', 'discovery')",
            "progress JSONB",
        ] {
            assert!(
                schema.contains(marker),
                "missing migration marker: {marker}"
            );
        }
    }

    /// Set TEST_DATABASE_URL to run the PostgreSQL constraint and repository
    /// coverage. It is intentionally separate from DATABASE_URL so a normal
    /// test run cannot mutate an operator's configured gateway database.
    #[tokio::test]
    async fn model_catalog_database_refresh_constraints_and_binding_states() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("TEST_DATABASE_URL is not set; skipping PostgreSQL model catalog test");
            return;
        };
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect model catalog test database");
        let database = Database { pool: pool.clone() };
        database.migrate().await.expect("migrate test database");
        let repository = ModelCatalogRepository::new(pool.clone());
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let imported_provider_id = format!("test-import-provider-{suffix}");
        let imported_account_id = format!("test-import-account-{suffix}");
        let preset_id = format!("test-provider-{suffix}");
        let model_preset_id = format!("test-model-preset-{suffix}");
        let source_id = format!("test-source-{suffix}");
        let account_id = format!("test-account-{suffix}");
        let logical_id = format!("test-logical-{suffix}");
        let upstream_model_id = "upstream-model";
        let public_name = format!("public-{suffix}");

        let mut imported_config: GatewayConfig = serde_json::from_value(json!({
            "listen_addr": "127.0.0.1:0",
            "providers": [{
                "id": imported_provider_id,
                "name": "Imported Provider",
                "base_url": "https://imported.example"
            }],
            "accounts": [{
                "id": imported_account_id,
                "provider_id": imported_provider_id,
                "display_name": "Imported Account"
            }],
            "routes": []
        }))
        .expect("build import config");
        database
            .sync_control_plane(&imported_config)
            .await
            .expect("first config import creates source before account");
        imported_config.providers[0].base_url = "https://changed.example".into();
        database
            .sync_control_plane(&imported_config)
            .await
            .expect("repeat config import preserves independent source snapshot");
        let imported: (String, String, String) = sqlx::query_as("SELECT s.provider_preset_id,a.source_id,s.base_url FROM sources s JOIN accounts a ON a.source_id=s.id WHERE s.id=$1 AND a.id=$2")
            .bind(&imported_provider_id)
            .bind(&imported_account_id)
            .fetch_one(&pool)
            .await
            .expect("load imported source and account");
        assert_eq!(
            imported,
            (
                "custom".into(),
                imported_provider_id.clone(),
                "https://imported.example".into()
            )
        );

        repository
            .insert_provider_preset(&ProviderPresetInput {
                id: preset_id.clone(),
                version: 1,
                display_name: "Test Provider".into(),
                definition: json!({"default_base_url":"https://preset.example"}),
            })
            .await
            .expect("insert provider preset");
        assert!(repository
            .insert_provider_preset(&ProviderPresetInput {
                id: preset_id.clone(),
                version: 1,
                display_name: "Changed Provider".into(),
                definition: json!({"default_base_url":"https://changed.example"}),
            })
            .await
            .is_err());
        let source = repository
            .create_source(&SourceInput {
                id: source_id.clone(),
                display_name: "Test Source".into(),
                provider_preset_id: preset_id.clone(),
                provider_preset_version: 1,
                base_url: "https://source.example".into(),
                endpoints: json!({"openai_chat_completions":"/v1/chat/completions"}),
                auth_config: json!({}),
                protocol_capabilities: json!({}),
            })
            .await
            .expect("create source from immutable preset snapshot");
        assert_eq!(
            source.provider_preset_snapshot,
            json!({"default_base_url":"https://preset.example"})
        );

        let preset_values = MetadataValues::from_fields([
            (MetadataField::ContextWindow, json!(32_768)),
            (MetadataField::Thinking, json!("supported")),
        ])
        .unwrap();
        let model_preset = repository
            .insert_model_preset(&ModelPresetInput {
                id: model_preset_id.clone(),
                version: 1,
                canonical_model_id: upstream_model_id.into(),
                aliases: vec!["model-alias".into()],
                metadata: CatalogMetadata::resolve(
                    &MetadataValues::default(),
                    Some(&preset_values),
                )
                .unwrap(),
            })
            .await
            .expect("insert immutable model preset version");
        assert_eq!(model_preset.version, 1);
        assert!(repository
            .insert_model_preset(&ModelPresetInput {
                id: model_preset_id.clone(),
                version: 1,
                canonical_model_id: upstream_model_id.into(),
                aliases: vec!["different-alias".into()],
                metadata: CatalogMetadata::resolve(
                    &MetadataValues::default(),
                    Some(&preset_values),
                )
                .unwrap(),
            })
            .await
            .is_err());

        sqlx::query("INSERT INTO providers (id,name,base_url) VALUES ($1,$2,$3)")
            .bind(&source_id)
            .bind("Legacy provider bridge")
            .bind("https://source.example")
            .execute(&pool)
            .await
            .expect("insert current provider bridge");
        sqlx::query(
            "INSERT INTO accounts (id,provider_id,source_id,display_name) VALUES ($1,$2,$3,$4)",
        )
        .bind(&account_id)
        .bind(&source_id)
        .bind(&source_id)
        .bind("Test Account")
        .execute(&pool)
        .await
        .expect("insert source account");

        let initial_refresh = SourceModelRefresh {
            source_id: source_id.clone(),
            upstream_model_id: upstream_model_id.into(),
            raw_snapshot: json!({"id":upstream_model_id,"revision":1}),
            upstream_metadata: MetadataValues::from_fields([
                (MetadataField::ContextWindow, json!(8_192)),
                (MetadataField::Tools, json!("unsupported")),
            ])
            .unwrap(),
            matched_preset: Some(ModelPresetRef {
                id: model_preset_id.clone(),
                version: 1,
            }),
            preset_metadata: Some(preset_values),
            discovered_at: Utc::now(),
        };
        repository
            .refresh_source_model(&initial_refresh)
            .await
            .expect("insert discovered source model");
        repository
            .refresh_source_model(&initial_refresh)
            .await
            .expect("repeat refresh is idempotent");
        let row_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM source_models WHERE source_id=$1 AND upstream_model_id=$2",
        )
        .bind(&source_id)
        .bind(upstream_model_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row_count, 1);

        repository
            .confirm_source_model(
                &source_id,
                upstream_model_id,
                &MetadataValues::from_fields([(MetadataField::ContextWindow, json!(65_536))])
                    .unwrap(),
            )
            .await
            .expect("confirm source model with user override");
        let refreshed = repository
            .refresh_source_model(&SourceModelRefresh {
                raw_snapshot: json!({"id":upstream_model_id,"revision":2}),
                upstream_metadata: MetadataValues::from_fields([(
                    MetadataField::ContextWindow,
                    json!(128_000),
                )])
                .unwrap(),
                matched_preset: None,
                preset_metadata: None,
                discovered_at: Utc::now(),
                ..initial_refresh
            })
            .await
            .expect("refresh confirmed source model");
        let metadata = refreshed.catalog_metadata().unwrap();
        assert_eq!(
            metadata.values.0[&MetadataField::ContextWindow],
            json!(65_536)
        );
        assert_eq!(
            metadata.field_sources[&MetadataField::ContextWindow],
            MetadataSource::User
        );
        assert_eq!(refreshed.raw_snapshot["revision"], json!(2));
        assert_eq!(
            refreshed.matched_model_preset_id.as_deref(),
            Some(model_preset_id.as_str())
        );

        repository
            .create_logical_model(&LogicalModelInput {
                id: logical_id.clone(),
                public_name: public_name.clone(),
                display_name: "Public Model".into(),
                status: CatalogStatus::Confirmed,
                model_preset: None,
                metadata: CatalogMetadata::resolve(&MetadataValues::default(), None).unwrap(),
            })
            .await
            .expect("create confirmed logical model");

        let capability = SourceModelCapabilityInput {
            source_id: source_id.clone(),
            upstream_model_id: upstream_model_id.into(),
            protocol: Protocol::OpenAiChatCompletions,
            status: CatalogStatus::Confirmed,
            mode: SourceProtocolMode::Unsupported,
            source_protocol: None,
            adapter: None,
            feature_capabilities: BTreeMap::from([(
                "tools".into(),
                CapabilitySupport::Unsupported,
            )]),
            field_source: MetadataSource::Upstream,
            observed_at: Utc::now(),
        };
        repository
            .upsert_source_model_capability(&capability)
            .await
            .expect("record explicit unsupported capability");
        let binding = repository
            .create_model_binding(&ModelBindingInput {
                logical_model_id: logical_id.clone(),
                source_id: source_id.clone(),
                account_id: account_id.clone(),
                upstream_model_id: upstream_model_id.into(),
                protocol: Protocol::OpenAiChatCompletions,
                priority: 100,
            })
            .await
            .expect("create pending binding");
        assert!(repository
            .transition_model_binding_status(binding.id, CatalogStatus::Confirmed)
            .await
            .is_err());
        assert!(repository
            .list_routable_bindings(&public_name, Protocol::OpenAiChatCompletions)
            .await
            .unwrap()
            .is_empty());

        repository
            .upsert_source_model_capability(&SourceModelCapabilityInput {
                mode: SourceProtocolMode::Native,
                feature_capabilities: BTreeMap::from([(
                    "tools".into(),
                    CapabilitySupport::Supported,
                )]),
                field_source: MetadataSource::User,
                ..capability
            })
            .await
            .expect("confirm native capability");
        repository
            .transition_model_binding_status(binding.id, CatalogStatus::Confirmed)
            .await
            .expect("confirm routable binding");
        let routable = repository
            .list_routable_bindings(&public_name, Protocol::OpenAiChatCompletions)
            .await
            .unwrap();
        assert_eq!(routable.len(), 1);
        assert_eq!(routable[0].upstream_model_id, upstream_model_id);

        assert!(repository
            .create_model_binding(&ModelBindingInput {
                logical_model_id: logical_id,
                source_id: source_id.clone(),
                account_id,
                upstream_model_id: upstream_model_id.into(),
                protocol: Protocol::OpenAiChatCompletions,
                priority: 10,
            })
            .await
            .is_err());

        repository
            .mark_source_model_unavailable(&source_id, upstream_model_id, Utc::now())
            .await
            .expect("mark missing discovery result unavailable");
        assert!(repository
            .list_routable_bindings(&public_name, Protocol::OpenAiChatCompletions)
            .await
            .unwrap()
            .is_empty());
        repository
            .transition_model_binding_status(binding.id, CatalogStatus::Unavailable)
            .await
            .expect("mark binding unavailable");
        assert!(repository
            .transition_model_binding_status(binding.id, CatalogStatus::Confirmed)
            .await
            .is_err());

        sqlx::query("DELETE FROM logical_models WHERE public_name=$1")
            .bind(&public_name)
            .execute(&pool)
            .await
            .expect("delete test logical model cascade");
        sqlx::query("DELETE FROM sources WHERE id=$1")
            .bind(&source_id)
            .execute(&pool)
            .await
            .expect("delete test source cascade");
        sqlx::query("DELETE FROM providers WHERE id=$1")
            .bind(&source_id)
            .execute(&pool)
            .await
            .expect("delete test provider bridge");
        sqlx::query("DELETE FROM provider_presets WHERE id=$1 AND version=1")
            .bind(&preset_id)
            .execute(&pool)
            .await
            .expect("delete test provider preset");
        sqlx::query("DELETE FROM model_presets WHERE id=$1 AND version=1")
            .bind(&model_preset_id)
            .execute(&pool)
            .await
            .expect("delete test model preset");
        sqlx::query("DELETE FROM sources WHERE id=$1")
            .bind(&imported_provider_id)
            .execute(&pool)
            .await
            .expect("delete imported test source cascade");
        sqlx::query("DELETE FROM providers WHERE id=$1")
            .bind(&imported_provider_id)
            .execute(&pool)
            .await
            .expect("delete imported test provider");
    }
}
