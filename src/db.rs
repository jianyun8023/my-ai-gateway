use crate::config::GatewayConfig;
use chrono::{DateTime, Utc};
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
    pub provider_id: String,
    pub account_id: String,
    pub model: String,
    pub logical_model: String,
    pub upstream_model_id: Option<String>,
    pub source: String,
    pub protocol_in: String,
    pub mode: String,
    pub status_code: i32,
    pub success: bool,
    pub retry_count: i32,
    pub latency_ms: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub total_tokens: i64,
    pub usage_source: String,
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
    pub model: Option<String>,
    pub provider_id: Option<String>,
    pub account_id: Option<String>,
    pub protocol: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct UsageAggregate {
    pub requests: i64,
    pub successes: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct UsageTimeBucket {
    pub bucket: DateTime<Utc>,
    pub requests: i64,
    pub successes: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct UsageBreakdown {
    pub dimension: String,
    pub requests: i64,
    pub successes: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub total_tokens: i64,
}

impl Database {
    pub async fn connect_from_env() -> Result<Option<Self>, sqlx::Error> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            return Ok(None);
        };
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(&url)
            .await?;
        let db = Self { pool };
        db.migrate().await?;
        Ok(Some(db))
    }

    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(include_str!("../migrations/0001_init.sql"))
            .execute(&self.pool)
            .await?;
        sqlx::query(include_str!("../migrations/0002_control_plane.sql"))
            .execute(&self.pool)
            .await?;
        Ok(())
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
        sqlx::query("INSERT INTO usage_events (request_id, provider_id, account_id, model, logical_model, upstream_model_id, source, protocol_in, protocol_upstream, mode, status_code, success, retry_count, latency_ms, ttft_ms, input_tokens, output_tokens, reasoning_tokens, cached_tokens, total_tokens, usage_source, degraded, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23) ON CONFLICT (request_id) DO NOTHING")
            .bind(&event.request_id).bind(&event.provider_id).bind(&event.account_id).bind(&event.model)
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
            sqlx::query("INSERT INTO providers (id,name,base_url,capabilities,endpoints) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (id) DO UPDATE SET name=EXCLUDED.name,base_url=EXCLUDED.base_url,capabilities=EXCLUDED.capabilities,endpoints=EXCLUDED.endpoints,updated_at=NOW()")
                .bind(&provider.id).bind(&provider.name).bind(&provider.base_url)
                .bind(serde_json::to_value(&provider.capabilities).unwrap_or(Value::Object(Default::default())))
                .bind(serde_json::to_value(&provider.endpoints).unwrap_or(Value::Object(Default::default())))
                .execute(&mut *tx).await?;
        }
        for account in &config.accounts {
            sqlx::query("INSERT INTO accounts (id,provider_id,display_name,enabled,weight) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (id) DO UPDATE SET provider_id=EXCLUDED.provider_id,display_name=EXCLUDED.display_name,enabled=EXCLUDED.enabled,weight=EXCLUDED.weight,updated_at=NOW()")
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
    ) -> Result<bool, sqlx::Error> {
        let hash = hash_key(raw);
        let row = sqlx::query_as::<_, (bool, Value)>("SELECT enabled,allowed_models FROM virtual_keys WHERE key_hash=$1 AND revoked_at IS NULL").bind(&hash).fetch_optional(&self.pool).await?;
        let Some((enabled, allowed)) = row else {
            return Ok(false);
        };
        if !enabled {
            return Ok(false);
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
        Ok(permitted)
    }

    pub async fn usage_summary(&self) -> Result<(i64, i64, i64, i64), sqlx::Error> {
        sqlx::query_as::<_, (i64, i64, i64, i64)>("SELECT COUNT(*), COUNT(*) FILTER (WHERE success), COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0) FROM usage_events")
            .fetch_one(&self.pool).await
    }

    pub async fn list_usage_events(
        &self,
        limit: i64,
    ) -> Result<Vec<UsageEventRecord>, sqlx::Error> {
        sqlx::query_as::<_, UsageEventRecord>("SELECT request_id,provider_id,account_id,model,logical_model,upstream_model_id,source,protocol_in,mode,status_code,success,retry_count,latency_ms,input_tokens,output_tokens,reasoning_tokens,cached_tokens,total_tokens,usage_source,created_at FROM usage_events ORDER BY created_at DESC LIMIT $1")
            .bind(limit.clamp(1, 500)).fetch_all(&self.pool).await
    }

    pub async fn usage_aggregate(
        &self,
        filter: &UsageFilter,
    ) -> Result<UsageAggregate, sqlx::Error> {
        let (where_sql, binds) = filter_sql(filter);
        let query = format!("SELECT COUNT(*)::BIGINT AS requests, COUNT(*) FILTER (WHERE success)::BIGINT AS successes, COALESCE(SUM(input_tokens),0)::BIGINT AS input_tokens, COALESCE(SUM(output_tokens),0)::BIGINT AS output_tokens, COALESCE(SUM(reasoning_tokens),0)::BIGINT AS reasoning_tokens, COALESCE(SUM(cached_tokens),0)::BIGINT AS cached_tokens, COALESCE(SUM(total_tokens),0)::BIGINT AS total_tokens FROM usage_events {where_sql}");
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
        let query = format!("SELECT date_trunc('{trunc}', created_at AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' AS bucket, COUNT(*)::BIGINT AS requests, COUNT(*) FILTER (WHERE success)::BIGINT AS successes, COALESCE(SUM(input_tokens),0)::BIGINT AS input_tokens, COALESCE(SUM(output_tokens),0)::BIGINT AS output_tokens, COALESCE(SUM(reasoning_tokens),0)::BIGINT AS reasoning_tokens, COALESCE(SUM(cached_tokens),0)::BIGINT AS cached_tokens, COALESCE(SUM(total_tokens),0)::BIGINT AS total_tokens FROM usage_events {where_sql} GROUP BY 1 ORDER BY 1");
        let mut q = sqlx::query_as::<_, UsageTimeBucket>(&query);
        q = bind_filter(q, binds);
        q.fetch_all(&self.pool).await
    }

    pub async fn usage_breakdown(
        &self,
        filter: &UsageFilter,
        dimension: &str,
    ) -> Result<Vec<UsageBreakdown>, sqlx::Error> {
        let column = match dimension {
            "model" => "logical_model",
            "provider" => "provider_id",
            "account" => "account_id",
            "protocol" => "protocol_in",
            "source" => "source",
            _ => "logical_model",
        };
        let (where_sql, binds) = filter_sql(filter);
        let query = format!("SELECT {column} AS dimension, COUNT(*)::BIGINT AS requests, COUNT(*) FILTER (WHERE success)::BIGINT AS successes, COALESCE(SUM(input_tokens),0)::BIGINT AS input_tokens, COALESCE(SUM(output_tokens),0)::BIGINT AS output_tokens, COALESCE(SUM(reasoning_tokens),0)::BIGINT AS reasoning_tokens, COALESCE(SUM(cached_tokens),0)::BIGINT AS cached_tokens, COALESCE(SUM(total_tokens),0)::BIGINT AS total_tokens FROM usage_events {where_sql} GROUP BY {column} ORDER BY requests DESC");
        let mut q = sqlx::query_as::<_, UsageBreakdown>(&query);
        q = bind_filter(q, binds);
        q.fetch_all(&self.pool).await
    }
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
        ("logical_model", &filter.model),
        ("provider_id", &filter.provider_id),
        ("account_id", &filter.account_id),
        ("protocol_in", &filter.protocol),
        ("source", &filter.source),
    ] {
        if let Some(value) = value {
            clauses.push(format!("{column} = ${}", binds.len() + 1));
            binds.push(FilterBind::Text(value.clone()));
        }
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

    #[test]
    fn filters_support_combined_dimensions_and_utc_bounds() {
        let filter = UsageFilter {
            from: Some("2026-01-01T00:00:00Z".parse().unwrap()),
            to: Some("2026-01-02T00:00:00Z".parse().unwrap()),
            model: Some("m".into()),
            provider_id: Some("p".into()),
            account_id: Some("a".into()),
            protocol: Some("openai_chat_completions".into()),
            source: None,
        };
        let (sql, binds) = filter_sql(&filter);
        assert!(sql.contains("created_at >= $1"));
        assert!(sql.contains("logical_model = $3"));
        assert!(sql.contains("protocol_in = $6"));
        assert_eq!(binds.len(), 6);
    }

    #[test]
    fn initial_schema_keeps_logical_request_and_attempt_idempotency() {
        let schema = include_str!("../migrations/0001_init.sql");
        assert!(schema.contains("logical_model TEXT NOT NULL"));
        assert!(schema.contains("UNIQUE (request_id, attempt_no)"));
        assert!(schema.contains("request_id TEXT NOT NULL UNIQUE"));
    }
}
