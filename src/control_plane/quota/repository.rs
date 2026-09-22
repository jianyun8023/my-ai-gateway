use serde_json::Value;
use sqlx::{FromRow, PgPool};

#[derive(Clone, Debug, FromRow)]
pub(super) struct QuotaTarget {
    pub(super) account_id: String,
    pub(super) source_id: String,
    pub(super) account_display_name: String,
    pub(super) credential_env: Option<String>,
    pub(super) credential_ciphertext: Option<String>,
    pub(super) account_enabled: bool,
    pub(super) source_display_name: String,
    pub(super) provider_preset_id: String,
    pub(super) base_url: String,
    pub(super) auth_config: Value,
    pub(super) source_enabled: bool,
}

pub(super) async fn list_targets(pool: &PgPool) -> Result<Vec<QuotaTarget>, sqlx::Error> {
    sqlx::query_as::<_, QuotaTarget>(
        "SELECT a.id AS account_id,a.source_id,a.display_name AS account_display_name,a.credential_env,a.credential_ciphertext,a.enabled AS account_enabled,s.display_name AS source_display_name,s.provider_preset_id,s.base_url,s.auth_config,s.enabled AS source_enabled FROM accounts a JOIN sources s ON s.id=a.source_id ORDER BY s.display_name,a.display_name,a.id",
    )
    .fetch_all(pool)
    .await
}

pub(super) async fn get_target(
    pool: &PgPool,
    account_id: &str,
) -> Result<Option<QuotaTarget>, sqlx::Error> {
    sqlx::query_as::<_, QuotaTarget>(
        "SELECT a.id AS account_id,a.source_id,a.display_name AS account_display_name,a.credential_env,a.credential_ciphertext,a.enabled AS account_enabled,s.display_name AS source_display_name,s.provider_preset_id,s.base_url,s.auth_config,s.enabled AS source_enabled FROM accounts a JOIN sources s ON s.id=a.source_id WHERE a.id=$1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
}
