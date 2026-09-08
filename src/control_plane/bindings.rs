use super::error::ControlPlaneError;
use super::model_catalog;
use super::repository::{fetch_model_binding_tx, model_binding_select};
use super::service::ControlPlane;
use super::snapshot::RuntimeSnapshot;
use super::types::{ModelBindingView, ModelBindingWrite, Mutation};
use super::validation::{
    validate_binding_input_shape, validate_binding_reference, validate_status_transition,
};
use crate::domain::catalog::{CatalogStatus, SourceModelCapabilityInput};
use chrono::Utc;
use serde_json::Value;

/// 能力声明缺省时从 SourceModel metadata 推导 feature 能力；
/// 元数据缺失或值非法的 feature 不写入（保持 unknown 语义，不猜测）。
fn derive_feature_capabilities(
    metadata: &Value,
) -> std::collections::BTreeMap<String, crate::domain::catalog::CapabilitySupport> {
    const FEATURE_KEYS: [&str; 6] = [
        "tools",
        "thinking",
        "web_search",
        "structured_output",
        "streaming",
        "usage",
    ];
    FEATURE_KEYS
        .iter()
        .filter_map(|key| {
            let value = metadata.get(*key)?.as_str()?;
            let support = serde_json::from_value::<crate::domain::catalog::CapabilitySupport>(
                Value::String(value.to_owned()),
            )
            .ok()?;
            Some(((*key).to_owned(), support))
        })
        .collect()
}

impl ControlPlane {
    pub(crate) async fn list_model_bindings(
        &self,
    ) -> Result<Vec<ModelBindingView>, ControlPlaneError> {
        Ok(
            sqlx::query_as::<_, ModelBindingView>(model_binding_select(false))
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub(crate) async fn list_source_model_capabilities(
        &self,
        source_id: &str,
        upstream_model_id: &str,
    ) -> Result<Vec<model_catalog::SourceModelCapabilityRecord>, ControlPlaneError> {
        let repository = model_catalog::ModelCatalogRepository::new(self.pool.clone());
        repository
            .list_source_model_capabilities(source_id, upstream_model_id)
            .await
            .map_err(ControlPlaneError::from)
    }

    /// UI/API 驱动的能力声明与确认。upsert 与 runtime snapshot 发布在同一事务
    /// 中完成；`finish_write` 内的 `validate_capability_chains` 兜底 adapter 链合法性。
    pub(crate) async fn upsert_source_model_capability(
        &self,
        input: &SourceModelCapabilityInput,
    ) -> Result<Mutation<model_catalog::SourceModelCapabilityRecord>, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let model_metadata = sqlx::query_scalar::<_, Option<Value>>(
            "SELECT metadata FROM source_models WHERE source_id=$1 AND upstream_model_id=$2",
        )
        .bind(&input.source_id)
        .bind(&input.upstream_model_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(model_metadata) = model_metadata.flatten() else {
            return Err(ControlPlaneError::Validation(vec![format!(
                "source model {}/{} not found",
                input.source_id, input.upstream_model_id
            )]));
        };
        let mut resolved = input.clone();
        if resolved.feature_capabilities.is_empty() {
            resolved.feature_capabilities = derive_feature_capabilities(&model_metadata);
        }
        let record = model_catalog::upsert_source_model_capability_conn(&mut tx, &resolved)
            .await
            .map_err(ControlPlaneError::from)?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn get_model_binding(
        &self,
        id: i64,
    ) -> Result<ModelBindingView, ControlPlaneError> {
        sqlx::query_as::<_, ModelBindingView>(model_binding_select(true))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ControlPlaneError::NotFound(format!("model binding '{id}' not found")))
    }

    pub(crate) async fn create_model_binding(
        &self,
        input: &ModelBindingWrite,
    ) -> Result<Mutation<ModelBindingView>, ControlPlaneError> {
        validate_binding_input_shape(input)?;
        let mut tx = self.begin_write().await?;
        validate_binding_reference(&mut tx, input).await?;
        let confirmed_at = (input.status == CatalogStatus::Confirmed).then(Utc::now);
        let unavailable_at = (input.status == CatalogStatus::Unavailable).then(Utc::now);
        let record = sqlx::query_as::<_, ModelBindingView>(
            "INSERT INTO model_bindings (logical_model_id,source_id,account_id,upstream_model_id,protocol,status,enabled,priority,confirmed_at,unavailable_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id,logical_model_id,source_id,account_id,upstream_model_id,protocol,status,enabled,priority,confirmed_at,unavailable_at,created_at,updated_at",
        )
        .bind(&input.logical_model_id)
        .bind(&input.source_id)
        .bind(&input.account_id)
        .bind(&input.upstream_model_id)
        .bind(input.protocol)
        .bind(input.status)
        .bind(input.enabled)
        .bind(input.priority)
        .bind(confirmed_at)
        .bind(unavailable_at)
        .fetch_one(&mut *tx)
        .await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn update_model_binding(
        &self,
        id: i64,
        input: &ModelBindingWrite,
    ) -> Result<Mutation<ModelBindingView>, ControlPlaneError> {
        validate_binding_input_shape(input)?;
        let mut tx = self.begin_write().await?;
        let current: CatalogStatus =
            sqlx::query_scalar("SELECT status FROM model_bindings WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| {
                    ControlPlaneError::NotFound(format!("model binding '{id}' not found"))
                })?;
        validate_status_transition(current, input.status, "model binding")?;
        validate_binding_reference(&mut tx, input).await?;
        let record = sqlx::query_as::<_, ModelBindingView>(
            "UPDATE model_bindings SET logical_model_id=$2,source_id=$3,account_id=$4,upstream_model_id=$5,protocol=$6,status=$7,enabled=$8,priority=$9,confirmed_at=CASE WHEN $7='confirmed' THEN COALESCE(confirmed_at,NOW()) ELSE confirmed_at END,unavailable_at=CASE WHEN $7='unavailable' THEN NOW() ELSE NULL END,updated_at=NOW() WHERE id=$1 RETURNING id,logical_model_id,source_id,account_id,upstream_model_id,protocol,status,enabled,priority,confirmed_at,unavailable_at,created_at,updated_at",
        )
        .bind(id)
        .bind(&input.logical_model_id)
        .bind(&input.source_id)
        .bind(&input.account_id)
        .bind(&input.upstream_model_id)
        .bind(input.protocol)
        .bind(input.status)
        .bind(input.enabled)
        .bind(input.priority)
        .fetch_one(&mut *tx)
        .await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn set_model_binding_enabled(
        &self,
        id: i64,
        enabled: bool,
    ) -> Result<Mutation<ModelBindingView>, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let existing = fetch_model_binding_tx(&mut tx, id).await?;
        if enabled {
            validate_binding_reference(
                &mut tx,
                &ModelBindingWrite {
                    logical_model_id: existing.logical_model_id.clone(),
                    source_id: existing.source_id.clone(),
                    account_id: existing.account_id.clone(),
                    upstream_model_id: existing.upstream_model_id.clone(),
                    protocol: existing.protocol,
                    status: existing.status,
                    enabled,
                    priority: existing.priority,
                },
            )
            .await?;
        }
        let record = sqlx::query_as::<_, ModelBindingView>(
            "UPDATE model_bindings SET enabled=$2,updated_at=NOW() WHERE id=$1 RETURNING id,logical_model_id,source_id,account_id,upstream_model_id,protocol,status,enabled,priority,confirmed_at,unavailable_at,created_at,updated_at",
        )
        .bind(id)
        .bind(enabled)
        .fetch_one(&mut *tx)
        .await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn delete_model_binding(
        &self,
        id: i64,
    ) -> Result<RuntimeSnapshot, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let result = sqlx::query("DELETE FROM model_bindings WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ControlPlaneError::NotFound(format!(
                "model binding '{id}' not found"
            )));
        }
        self.finish_write(tx).await
    }
}
