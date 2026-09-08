use super::error::ControlPlaneError;
use super::repository::{
    account_view_select, apply_manual_health_transition, ensure_source_exists, fetch_account_tx,
    row_exists,
};
use super::service::ControlPlane;
use super::snapshot::RuntimeSnapshot;
use super::types::{AccountView, AccountWrite, Mutation};
use super::validation::{validate_account_input, validate_resource_id};
use chrono::Utc;

impl ControlPlane {
    /// Rotate under the same transaction and snapshot validation as account CRUD.
    /// Any credential, SQL or snapshot failure rolls the entire write back.
    pub(crate) async fn rotate_account_credential(
        &self,
        id: &str,
        secrets: &crate::infra::secrets::SecretResolver,
    ) -> Result<RuntimeSnapshot, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let (source_id, ciphertext): (String, Option<String>) = sqlx::query_as(
            "SELECT source_id,credential_ciphertext FROM accounts WHERE id=$1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ControlPlaneError::NotFound("account not found".into()))?;
        let ciphertext = ciphertext.ok_or(ControlPlaneError::NoCiphertext)?;
        let rotated = secrets.rotate_for_account(&source_id, id, &ciphertext)?;
        sqlx::query("UPDATE accounts SET credential_ciphertext=$2,updated_at=NOW() WHERE id=$1")
            .bind(id)
            .bind(rotated)
            .execute(&mut *tx)
            .await?;
        self.finish_write(tx).await
    }

    pub(crate) async fn list_accounts(&self) -> Result<Vec<AccountView>, ControlPlaneError> {
        Ok(sqlx::query_as::<_, AccountView>(account_view_select(false))
            .fetch_all(&self.pool)
            .await?)
    }

    pub(crate) async fn get_account(&self, id: &str) -> Result<AccountView, ControlPlaneError> {
        sqlx::query_as::<_, AccountView>(account_view_select(true))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ControlPlaneError::NotFound(format!("account '{id}' not found")))
    }

    pub(crate) async fn create_account(
        &self,
        input: &AccountWrite,
    ) -> Result<Mutation<AccountView>, ControlPlaneError> {
        validate_account_input(input)?;
        let mut tx = self.begin_write().await?;
        if row_exists(&mut tx, "accounts", "id", &input.id).await? {
            return Err(ControlPlaneError::Conflict(format!(
                "account '{}' already exists",
                input.id
            )));
        }
        ensure_source_exists(&mut tx, &input.source_id).await?;
        sqlx::query("INSERT INTO accounts (id,provider_id,source_id,display_name,credential_ciphertext,credential_env,enabled,weight) VALUES ($1,NULL,$2,$3,$4,$5,$6,$7)")
            .bind(&input.id)
            .bind(&input.source_id)
            .bind(&input.display_name)
            .bind(&input.credential_ciphertext)
            .bind(&input.credential_env)
            .bind(input.enabled)
            .bind(input.weight)
            .execute(&mut *tx)
            .await?;
        if !input.enabled {
            apply_manual_health_transition(&mut tx, &input.id, false, Utc::now()).await?;
        }
        let record = fetch_account_tx(&mut tx, &input.id).await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn update_account(
        &self,
        id: &str,
        input: &AccountWrite,
    ) -> Result<Mutation<AccountView>, ControlPlaneError> {
        validate_resource_id(id, &input.id, "account")?;
        validate_account_input(input)?;
        let mut tx = self.begin_write().await?;
        ensure_source_exists(&mut tx, &input.source_id).await?;
        let previous: Option<(String, bool)> =
            sqlx::query_as("SELECT source_id,enabled FROM accounts WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let result = sqlx::query("UPDATE accounts SET provider_id=NULL,source_id=$2,display_name=$3,credential_ciphertext=$4,credential_env=$5,enabled=$6,weight=$7,updated_at=NOW() WHERE id=$1")
            .bind(id)
            .bind(&input.source_id)
            .bind(&input.display_name)
            .bind(&input.credential_ciphertext)
            .bind(&input.credential_env)
            .bind(input.enabled)
            .bind(input.weight)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ControlPlaneError::NotFound(format!(
                "account '{id}' not found"
            )));
        }
        if previous.is_some_and(|(source_id, enabled)| {
            source_id != input.source_id || enabled != input.enabled
        }) {
            apply_manual_health_transition(&mut tx, id, input.enabled, Utc::now()).await?;
        }
        let record = fetch_account_tx(&mut tx, id).await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn set_account_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<Mutation<AccountView>, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let result = sqlx::query("UPDATE accounts SET enabled=$2,updated_at=NOW() WHERE id=$1")
            .bind(id)
            .bind(enabled)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ControlPlaneError::NotFound(format!(
                "account '{id}' not found"
            )));
        }
        apply_manual_health_transition(&mut tx, id, enabled, Utc::now()).await?;
        let record = fetch_account_tx(&mut tx, id).await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn delete_account(
        &self,
        id: &str,
    ) -> Result<RuntimeSnapshot, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let result = sqlx::query("DELETE FROM accounts WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ControlPlaneError::NotFound(format!(
                "account '{id}' not found"
            )));
        }
        self.finish_write(tx).await
    }
}
