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

    pub async fn insert_usage(&self, event: &UsageEvent) -> Result<(), sqlx::Error> {
        let now: DateTime<Utc> = Utc::now();
        sqlx::query("INSERT INTO usage_events (request_id, provider_id, account_id, model, protocol_in, protocol_upstream, mode, status_code, success, retry_count, latency_ms, ttft_ms, input_tokens, output_tokens, reasoning_tokens, cached_tokens, total_tokens, usage_source, degraded, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20) ON CONFLICT (request_id) DO NOTHING")
            .bind(&event.request_id).bind(&event.provider_id).bind(&event.account_id).bind(&event.model)
            .bind(&event.protocol_in).bind(&event.protocol_upstream).bind(&event.mode).bind(event.status_code)
            .bind(event.success).bind(event.retry_count).bind(event.latency_ms).bind(event.ttft_ms)
            .bind(event.input_tokens).bind(event.output_tokens).bind(event.reasoning_tokens)
            .bind(event.cached_tokens).bind(event.total_tokens).bind(&event.usage_source).bind(event.degraded).bind(now)
            .execute(&self.pool).await?;
        Ok(())
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
        sqlx::query_as::<_, UsageEventRecord>("SELECT request_id,provider_id,account_id,model,protocol_in,mode,status_code,success,retry_count,latency_ms,input_tokens,output_tokens,reasoning_tokens,cached_tokens,total_tokens,usage_source,created_at FROM usage_events ORDER BY created_at DESC LIMIT $1")
            .bind(limit.clamp(1, 500)).fetch_all(&self.pool).await
    }
}

fn hash_key(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}
