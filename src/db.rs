use crate::config::GatewayConfig;
use crate::model_catalog::ModelCatalogRepository;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool};

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
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
    pub source: String,
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
    pub source: String,
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
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UsageAttempt {
    pub attempt_no: i32,
    pub provider_id: String,
    pub account_id: String,
    pub upstream_model_id: Option<String>,
    pub status_code: i32,
    pub success: bool,
    pub latency_ms: i64,
}

#[derive(Debug, Clone, Default)]
pub struct UsageFilter {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub logical_model: Option<String>,
    pub upstream_model_id: Option<String>,
    pub provider_id: Option<String>,
    pub source: Option<String>,
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
        tx.commit().await
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
        sqlx::query("INSERT INTO usage_events (request_id, virtual_key_id, provider_id, account_id, model, logical_model, upstream_model_id, source, protocol_in, protocol_upstream, mode, status_code, success, retry_count, latency_ms, ttft_ms, input_tokens, output_tokens, reasoning_tokens, cached_tokens, total_tokens, usage_source, degraded, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24) ON CONFLICT (request_id) DO NOTHING")
            .bind(&event.request_id).bind(event.virtual_key_id).bind(&event.provider_id).bind(&event.account_id).bind(&event.model)
            .bind(&event.logical_model).bind(&event.upstream_model_id).bind(&event.source)
            .bind(&event.protocol_in).bind(&event.protocol_upstream).bind(&event.mode).bind(event.status_code)
            .bind(event.success).bind(event.retry_count).bind(event.latency_ms).bind(event.ttft_ms)
            .bind(event.input_tokens).bind(event.output_tokens).bind(event.reasoning_tokens)
            .bind(event.cached_tokens).bind(event.total_tokens).bind(&event.usage_source).bind(event.degraded).bind(now)
            .execute(&mut *tx).await?;
        for attempt in attempts {
            sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,account_id,upstream_model_id,status_code,success,latency_ms) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (request_id,attempt_no) DO NOTHING")
                .bind(&event.request_id).bind(attempt.attempt_no).bind(&attempt.provider_id)
                .bind(&attempt.account_id).bind(&attempt.upstream_model_id).bind(attempt.status_code)
                .bind(attempt.success).bind(attempt.latency_ms).execute(&mut *tx).await?;
        }
        tx.commit().await
    }

    pub async fn sync_control_plane(&self, config: &GatewayConfig) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        for provider in &config.providers {
            let endpoints = serde_json::to_value(&provider.endpoints)
                .unwrap_or(Value::Object(Default::default()));
            let feature_capabilities = serde_json::to_value(&provider.capabilities)
                .unwrap_or(Value::Object(Default::default()));
            let protocol_capabilities = serde_json::to_value(&provider.protocol_capabilities)
                .unwrap_or(Value::Object(Default::default()));
            sqlx::query("INSERT INTO providers (id,name,base_url,capabilities,endpoints) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (id) DO UPDATE SET name=EXCLUDED.name,base_url=EXCLUDED.base_url,capabilities=EXCLUDED.capabilities,endpoints=EXCLUDED.endpoints,updated_at=NOW()")
                .bind(&provider.id).bind(&provider.name).bind(&provider.base_url)
                .bind(&feature_capabilities)
                .bind(&endpoints)
                .execute(&mut *tx).await?;
            let snapshot = serde_json::json!({
                "base_url": provider.base_url,
                "endpoints": endpoints,
                "feature_capabilities": feature_capabilities,
                "protocol_capabilities": protocol_capabilities,
                "native_protocols": provider.native_protocols,
            });
            sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,protocol_capabilities) VALUES ($1,$2,'custom',1,$3,$4,$5,$6) ON CONFLICT (id) DO NOTHING")
                .bind(&provider.id)
                .bind(&provider.name)
                .bind(snapshot)
                .bind(&provider.base_url)
                .bind(&endpoints)
                .bind(&protocol_capabilities)
                .execute(&mut *tx).await?;
        }
        for account in &config.accounts {
            sqlx::query("INSERT INTO accounts (id,provider_id,source_id,display_name,enabled,weight) VALUES ($1,$2,$2,$3,$4,$5) ON CONFLICT (id) DO UPDATE SET provider_id=EXCLUDED.provider_id,display_name=EXCLUDED.display_name,enabled=EXCLUDED.enabled,weight=EXCLUDED.weight,updated_at=NOW()")
                .bind(&account.id).bind(&account.provider_id).bind(&account.display_name).bind(account.enabled).bind(account.weight as i32)
                .execute(&mut *tx).await?;
        }
        for route in &config.routes {
            sqlx::query("INSERT INTO routes (id,model_pattern,provider_id,protocols,primary_account_id,fallback_accounts,strategy,mode,adapter,allow_lossy_conversion) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT (id) DO UPDATE SET model_pattern=EXCLUDED.model_pattern,provider_id=EXCLUDED.provider_id,protocols=EXCLUDED.protocols,primary_account_id=EXCLUDED.primary_account_id,fallback_accounts=EXCLUDED.fallback_accounts,strategy=EXCLUDED.strategy,mode=EXCLUDED.mode,adapter=EXCLUDED.adapter,allow_lossy_conversion=EXCLUDED.allow_lossy_conversion,updated_at=NOW()")
                .bind(&route.id).bind(&route.model).bind(&route.provider_id)
                .bind(serde_json::to_value(&route.protocols).unwrap_or(Value::Array(vec![])))
                .bind(&route.primary_account_id)
                .bind(serde_json::to_value(&route.fallback_accounts).unwrap_or(Value::Array(vec![])))
                .bind(&route.strategy).bind(&route.mode).bind(&route.adapter).bind(route.allow_lossy_conversion)
                .execute(&mut *tx).await?;
        }
        tx.commit().await
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
    ) -> Result<Vec<UsageEventRecord>, sqlx::Error> {
        let (where_sql, binds) = filter_sql(filter);
        let query = format!(
            "{} {where_sql} ORDER BY created_at DESC, request_id DESC",
            usage_event_select()
        );
        let mut q = sqlx::query_as::<_, UsageEventRecord>(&query);
        q = bind_filter(q, binds);
        q.fetch_all(&self.pool).await
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
            "source" => ("source", "f.source"),
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
    "SELECT request_id,virtual_key_id,provider_id,account_id,logical_model,upstream_model_id,source,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,ttft_ms,input_tokens,output_tokens,reasoning_tokens,cached_tokens,total_tokens,usage_source,degraded,created_at FROM usage_events"
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
        ("source", &filter.source),
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
    use std::collections::BTreeMap;

    #[test]
    fn filters_support_combined_dimensions_and_utc_bounds() {
        let filter = UsageFilter {
            from: Some("2026-01-01T00:00:00Z".parse().unwrap()),
            to: Some("2026-01-02T00:00:00Z".parse().unwrap()),
            logical_model: Some("m".into()),
            upstream_model_id: Some("upstream-m".into()),
            provider_id: Some("p".into()),
            source: Some("cli".into()),
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
        assert!(sql.contains("protocol_upstream = $9"));
        assert!(sql.contains("virtual_key_id = $11"));
        assert!(sql.contains("status_code = $13"));
        assert_eq!(binds.len(), 13);
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
            sqlx::query("INSERT INTO usage_events (request_id,virtual_key_id,provider_id,account_id,model,logical_model,upstream_model_id,source,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,input_tokens,output_tokens,total_tokens,usage_source,created_at) VALUES ($1,$2,'provider-a','account-a',$3,$3,'upstream-a','test','openai_responses','anthropic_messages','adapter',$4,$5,$6,25,$7,0,$7,$8,$9)")
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
                sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,account_id,status_code,success,latency_ms) VALUES ($1,$2,'provider-a','account-a',$3,$4,10)")
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
            source: Some("test".into()),
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
        let exported = database.export_usage_events(&filter).await.expect("export");
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
    async fn postgres_cursor_handles_large_pages_without_duplicates_or_omissions() {
        let Some(database) = postgres_test_database().await else {
            eprintln!("skipping PostgreSQL pagination test: TEST_DATABASE_URL is not set");
            return;
        };
        let prefix = format!("usage-page-{}-", uuid::Uuid::new_v4());
        let logical_model = format!("logical-{prefix}");
        sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,source,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,usage_source,created_at) SELECT $1 || LPAD(i::TEXT,4,'0'),'provider-page','account-page',$2,$2,'test','openai_responses','openai_responses','native',200,TRUE,0,1,'missing','2026-02-01T00:00:00Z'::TIMESTAMPTZ FROM generate_series(1,503) AS i")
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
