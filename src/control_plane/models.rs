use super::error::ControlPlaneError;
use super::repository::{fetch_logical_model_tx, logical_model_select, row_exists};
use super::service::ControlPlane;
use super::snapshot::RuntimeSnapshot;
use super::types::{LogicalModelView, LogicalModelWrite, Mutation};
use super::validation::{
    normalize_object, validate_logical_model_input, validate_resource_id,
    validate_status_transition,
};
use crate::domain::catalog::CatalogStatus;
use chrono::Utc;

impl ControlPlane {
    pub(crate) async fn list_logical_models(
        &self,
    ) -> Result<Vec<LogicalModelView>, ControlPlaneError> {
        Ok(
            sqlx::query_as::<_, LogicalModelView>(logical_model_select(false))
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub(crate) async fn get_logical_model(
        &self,
        id: &str,
    ) -> Result<LogicalModelView, ControlPlaneError> {
        sqlx::query_as::<_, LogicalModelView>(logical_model_select(true))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ControlPlaneError::NotFound(format!("logical model '{id}' not found")))
    }

    pub(crate) async fn create_logical_model(
        &self,
        input: &LogicalModelWrite,
    ) -> Result<Mutation<LogicalModelView>, ControlPlaneError> {
        validate_logical_model_input(input)?;
        let mut tx = self.begin_write().await?;
        if row_exists(&mut tx, "logical_models", "id", &input.id).await? {
            return Err(ControlPlaneError::Conflict(format!(
                "logical model '{}' already exists",
                input.id
            )));
        }
        let confirmed_at = (input.status == CatalogStatus::Confirmed).then(Utc::now);
        let unavailable_at = (input.status == CatalogStatus::Unavailable).then(Utc::now);
        let record = sqlx::query_as::<_, LogicalModelView>(
            "INSERT INTO logical_models (id,public_name,display_name,status,metadata,field_sources,enabled,confirmed_at,unavailable_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id,public_name,display_name,status,metadata,field_sources,enabled,request_timeout_ms,max_retries,confirmed_at,unavailable_at,created_at,updated_at",
        )
        .bind(&input.id)
        .bind(&input.public_name)
        .bind(&input.display_name)
        .bind(input.status)
        .bind(normalize_object(input.metadata.clone(), "logical model metadata")?)
        .bind(normalize_object(
            input.field_sources.clone(),
            "logical model field_sources",
        )?)
        .bind(input.enabled)
        .bind(confirmed_at)
        .bind(unavailable_at)
        .fetch_one(&mut *tx)
        .await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn update_logical_model(
        &self,
        id: &str,
        input: &LogicalModelWrite,
    ) -> Result<Mutation<LogicalModelView>, ControlPlaneError> {
        validate_resource_id(id, &input.id, "logical model")?;
        validate_logical_model_input(input)?;
        let mut tx = self.begin_write().await?;
        let current: CatalogStatus =
            sqlx::query_scalar("SELECT status FROM logical_models WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| {
                    ControlPlaneError::NotFound(format!("logical model '{id}' not found"))
                })?;
        validate_status_transition(current, input.status, "logical model")?;
        let record = sqlx::query_as::<_, LogicalModelView>(
            "UPDATE logical_models SET public_name=$2,display_name=$3,status=$4,metadata=$5,field_sources=$6,enabled=$7,confirmed_at=CASE WHEN $4='confirmed' THEN COALESCE(confirmed_at,NOW()) ELSE confirmed_at END,unavailable_at=CASE WHEN $4='unavailable' THEN NOW() ELSE NULL END,updated_at=NOW() WHERE id=$1 RETURNING id,public_name,display_name,status,metadata,field_sources,enabled,request_timeout_ms,max_retries,confirmed_at,unavailable_at,created_at,updated_at",
        )
        .bind(id)
        .bind(&input.public_name)
        .bind(&input.display_name)
        .bind(input.status)
        .bind(normalize_object(input.metadata.clone(), "logical model metadata")?)
        .bind(normalize_object(
            input.field_sources.clone(),
            "logical model field_sources",
        )?)
        .bind(input.enabled)
        .fetch_one(&mut *tx)
        .await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn set_logical_model_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<Mutation<LogicalModelView>, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let result =
            sqlx::query("UPDATE logical_models SET enabled=$2,updated_at=NOW() WHERE id=$1")
                .bind(id)
                .bind(enabled)
                .execute(&mut *tx)
                .await?;
        if result.rows_affected() == 0 {
            return Err(ControlPlaneError::NotFound(format!(
                "logical model '{id}' not found"
            )));
        }
        let record = fetch_logical_model_tx(&mut tx, id).await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn delete_logical_model(
        &self,
        id: &str,
    ) -> Result<RuntimeSnapshot, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let result = sqlx::query("DELETE FROM logical_models WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ControlPlaneError::NotFound(format!(
                "logical model '{id}' not found"
            )));
        }
        self.finish_write(tx).await
    }
}
