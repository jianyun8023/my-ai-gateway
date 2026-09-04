use super::*;

impl ControlPlane {
    pub async fn list_model_bindings(&self) -> Result<Vec<ModelBindingView>, ControlPlaneError> {
        Ok(
            sqlx::query_as::<_, ModelBindingView>(model_binding_select(None))
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn get_model_binding(&self, id: i64) -> Result<ModelBindingView, ControlPlaneError> {
        sqlx::query_as::<_, ModelBindingView>(model_binding_select(Some("WHERE id=$1")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ControlPlaneError::NotFound(format!("model binding '{id}' not found")))
    }

    pub async fn create_model_binding(
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn update_model_binding(
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn set_model_binding_enabled(
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn delete_model_binding(
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

    pub async fn list_routes(&self) -> Result<Vec<RouteView>, ControlPlaneError> {
        Ok(sqlx::query_as::<_, RouteView>(route_view_select(None))
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn get_route(&self, id: &str) -> Result<RouteView, ControlPlaneError> {
        sqlx::query_as::<_, RouteView>(route_view_select(Some("WHERE r.id=$1")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ControlPlaneError::NotFound(format!("route '{id}' not found")))
    }

    pub async fn create_route(
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn update_route(
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn set_route_enabled(
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn delete_route(&self, id: &str) -> Result<RuntimeSnapshot, ControlPlaneError> {
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
