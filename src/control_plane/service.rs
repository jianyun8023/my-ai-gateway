use super::error::ControlPlaneError;
use super::import::import_gateway_config;
use super::snapshot::{build_snapshot, RuntimeSnapshot};
use super::types::Mutation;
use super::validation::{
    source_url_validation_message, validate_capability_chains, validate_persisted_source_urls,
};
use crate::domain::config::GatewayConfig;
use crate::source_url::SourceUrlPolicy;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct ControlPlane {
    pub(super) pool: PgPool,
    pub(super) listen_addr: String,
    pub(super) source_url_policy: Arc<SourceUrlPolicy>,
}

impl ControlPlane {
    #[cfg(test)]
    pub(crate) fn new(pool: PgPool, listen_addr: impl Into<String>) -> Self {
        Self::with_url_policy(pool, listen_addr, Arc::new(SourceUrlPolicy::default()))
    }

    pub(crate) fn with_url_policy(
        pool: PgPool,
        listen_addr: impl Into<String>,
        source_url_policy: Arc<SourceUrlPolicy>,
    ) -> Self {
        Self {
            pool,
            listen_addr: listen_addr.into(),
            source_url_policy,
        }
    }

    pub(super) async fn begin_write(&self) -> Result<Transaction<'_, Postgres>, ControlPlaneError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    pub(super) async fn finish_write(
        &self,
        mut tx: Transaction<'_, Postgres>,
    ) -> Result<RuntimeSnapshot, ControlPlaneError> {
        validate_persisted_source_urls(&mut tx, &self.source_url_policy).await?;
        validate_capability_chains(&mut tx).await?;
        let (revision, generated_at): (i64, DateTime<Utc>) = sqlx::query_as(
            "UPDATE runtime_snapshot_state SET revision=revision+1,updated_at=clock_timestamp() WHERE singleton=TRUE RETURNING revision,updated_at",
        )
        .fetch_one(&mut *tx)
        .await?;
        let snapshot = build_snapshot(&mut tx, &self.listen_addr, revision, generated_at).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    pub(super) async fn finish_mutation<T>(
        &self,
        tx: Transaction<'_, Postgres>,
        record: T,
    ) -> Result<Mutation<T>, ControlPlaneError> {
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub(crate) async fn load_snapshot(&self) -> Result<RuntimeSnapshot, ControlPlaneError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;
        let snapshot = self.load_snapshot_in_transaction(&mut tx).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    /// Build a candidate snapshot from an already-open transaction. Operations
    /// that import/restore the control plane use this before commit so a
    /// fingerprint mismatch can roll the entire restore back atomically.
    pub(crate) async fn load_snapshot_in_transaction(
        &self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<RuntimeSnapshot, ControlPlaneError> {
        validate_persisted_source_urls(tx, &self.source_url_policy).await?;
        validate_capability_chains(tx).await?;
        let (revision, generated_at): (i64, DateTime<Utc>) = sqlx::query_as(
            "SELECT revision,updated_at FROM runtime_snapshot_state WHERE singleton=TRUE",
        )
        .fetch_one(&mut **tx)
        .await?;
        build_snapshot(tx, &self.listen_addr, revision, generated_at).await
    }

    pub(crate) async fn is_empty(&self) -> Result<bool, ControlPlaneError> {
        let row_count: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM sources) + (SELECT COUNT(*) FROM accounts) + \
                    (SELECT COUNT(*) FROM logical_models) + (SELECT COUNT(*) FROM model_bindings) + \
                    (SELECT COUNT(*) FROM routes)",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row_count == 0)
    }

    pub(crate) async fn initialize_from_config(
        &self,
        config: &GatewayConfig,
        force: bool,
    ) -> Result<Option<RuntimeSnapshot>, ControlPlaneError> {
        let mut validation_errors = config.validate().err().unwrap_or_default();
        for (index, provider) in config.providers.iter().enumerate() {
            if let Err(error) = self.source_url_policy.validate_base_url(&provider.base_url) {
                validation_errors.push(source_url_validation_message(
                    &format!("providers[{index}].base_url"),
                    error,
                ));
            }
        }
        if !validation_errors.is_empty() {
            return Err(ControlPlaneError::Validation(validation_errors));
        }
        if config
            .accounts
            .iter()
            .any(|account| account.credential.is_some())
        {
            return Err(ControlPlaneError::Validation(vec![
                "GATEWAY_CONFIG_JSON cannot import plaintext account credentials; use credential_env"
                    .to_owned(),
            ]));
        }
        let mut tx = self.begin_write().await?;
        let row_count: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM sources) + (SELECT COUNT(*) FROM accounts) + \
                    (SELECT COUNT(*) FROM logical_models) + (SELECT COUNT(*) FROM model_bindings) + \
                    (SELECT COUNT(*) FROM routes)",
        )
        .fetch_one(&mut *tx)
        .await?;
        if row_count > 0 && !force {
            tx.rollback().await?;
            return Ok(None);
        }
        if force && row_count > 0 {
            sqlx::query("DELETE FROM routes").execute(&mut *tx).await?;
            sqlx::query("DELETE FROM logical_models")
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM accounts")
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM sources").execute(&mut *tx).await?;
        }
        import_gateway_config(&mut tx, config).await?;
        self.finish_write(tx).await.map(Some)
    }
}
