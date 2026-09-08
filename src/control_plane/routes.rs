use super::error::ControlPlaneError;
use super::repository::{fetch_route_tx, route_view_select, row_exists};
use super::service::ControlPlane;
use super::snapshot::RuntimeSnapshot;
use super::types::{Mutation, RouteView, RouteWrite};
use super::validation::{
    validate_resource_id, validate_route_input_shape, validate_route_reference,
};
use crate::domain::protocol::Protocol;

impl ControlPlane {
    pub(crate) async fn list_routes(&self) -> Result<Vec<RouteView>, ControlPlaneError> {
        Ok(sqlx::query_as::<_, RouteView>(route_view_select(false))
            .fetch_all(&self.pool)
            .await?)
    }

    pub(crate) async fn get_route(&self, id: &str) -> Result<RouteView, ControlPlaneError> {
        sqlx::query_as::<_, RouteView>(route_view_select(true))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ControlPlaneError::NotFound(format!("route '{id}' not found")))
    }

    pub(crate) async fn create_route(
        &self,
        input: &RouteWrite,
    ) -> Result<Mutation<RouteView>, ControlPlaneError> {
        validate_route_input_shape(input)?;
        let mut tx = self.begin_write().await?;
        if row_exists(&mut tx, "routes", "id", &input.id).await? {
            return Err(ControlPlaneError::Conflict(format!(
                "route '{}' already exists",
                input.id
            )));
        }
        validate_route_reference(&mut tx, input).await?;
        let public_name: String =
            sqlx::query_scalar("SELECT public_name FROM logical_models WHERE id=$1")
                .bind(&input.logical_model_id)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query("INSERT INTO routes (id,logical_model_id,model_pattern,provider_id,protocols,primary_account_id,fallback_accounts,strategy,mode,adapter,allow_lossy_conversion,enabled) VALUES ($1,$2,$3,NULL,$4,NULL,'[]'::jsonb,$5,'binding',NULL,$6,$7)")
            .bind(&input.id)
            .bind(&input.logical_model_id)
            .bind(public_name)
            .bind(serde_json::to_value(&input.protocols)?)
            .bind(&input.strategy)
            .bind(input.allow_lossy_conversion)
            .bind(input.enabled)
            .execute(&mut *tx)
            .await?;
        let record = fetch_route_tx(&mut tx, &input.id).await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn update_route(
        &self,
        id: &str,
        input: &RouteWrite,
    ) -> Result<Mutation<RouteView>, ControlPlaneError> {
        validate_resource_id(id, &input.id, "route")?;
        validate_route_input_shape(input)?;
        let mut tx = self.begin_write().await?;
        validate_route_reference(&mut tx, input).await?;
        let public_name: String =
            sqlx::query_scalar("SELECT public_name FROM logical_models WHERE id=$1")
                .bind(&input.logical_model_id)
                .fetch_one(&mut *tx)
                .await?;
        let result = sqlx::query("UPDATE routes SET logical_model_id=$2,model_pattern=$3,provider_id=NULL,protocols=$4,primary_account_id=NULL,fallback_accounts='[]'::jsonb,strategy=$5,mode='binding',adapter=NULL,allow_lossy_conversion=$6,enabled=$7,updated_at=NOW() WHERE id=$1")
            .bind(id)
            .bind(&input.logical_model_id)
            .bind(public_name)
            .bind(serde_json::to_value(&input.protocols)?)
            .bind(&input.strategy)
            .bind(input.allow_lossy_conversion)
            .bind(input.enabled)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ControlPlaneError::NotFound(format!(
                "route '{id}' not found"
            )));
        }
        let record = fetch_route_tx(&mut tx, id).await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn set_route_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<Mutation<RouteView>, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let current = fetch_route_tx(&mut tx, id).await?;
        if enabled {
            let protocols: Vec<Protocol> = serde_json::from_value(current.protocols.clone())?;
            validate_route_reference(
                &mut tx,
                &RouteWrite {
                    id: current.id.clone(),
                    logical_model_id: current.logical_model_id.clone(),
                    protocols,
                    strategy: current.strategy.clone(),
                    allow_lossy_conversion: current.allow_lossy_conversion,
                    enabled,
                },
            )
            .await?;
        }
        let record = sqlx::query_as::<_, RouteView>(
            "UPDATE routes r SET enabled=$2,updated_at=NOW() FROM logical_models lm WHERE r.id=$1 AND lm.id=r.logical_model_id RETURNING r.id,r.logical_model_id,lm.public_name,r.protocols,r.strategy,r.allow_lossy_conversion,r.enabled,r.created_at,r.updated_at",
        )
        .bind(id)
        .bind(enabled)
        .fetch_one(&mut *tx)
        .await?;
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn delete_route(
        &self,
        id: &str,
    ) -> Result<RuntimeSnapshot, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let result = sqlx::query("DELETE FROM routes WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ControlPlaneError::NotFound(format!(
                "route '{id}' not found"
            )));
        }
        self.finish_write(tx).await
    }
}
