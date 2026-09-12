use super::repository::fetch_logical_model_tx;
use super::service::ControlPlane;
use super::types::{
    LogicalModelWrite, ModelBindingWrite, ModelRoutingLineView, ModelRoutingView,
    ModelRoutingWrite, Mutation, RouteWrite,
};
use super::validation::{
    validate_binding_reference, validate_logical_model_input, validate_route_reference,
    validate_status_transition,
};
use super::ControlPlaneError;
use crate::domain::{catalog::CatalogStatus, protocol::Protocol};
use serde_json::json;
use sqlx::{Postgres, Transaction};
use std::collections::HashSet;

impl ControlPlane {
    pub(crate) async fn get_model_routing(
        &self,
        id: &str,
    ) -> Result<ModelRoutingView, ControlPlaneError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;
        let record = fetch_model_routing_tx(&mut tx, id).await?;
        tx.commit().await?;
        Ok(record)
    }

    pub(crate) async fn create_model_routing(
        &self,
        input: &ModelRoutingWrite,
    ) -> Result<Mutation<ModelRoutingView>, ControlPlaneError> {
        let id = format!("model-{}", uuid::Uuid::new_v4());
        self.write_model_routing(&id, input, true).await
    }

    /// Compile a complete model intent in one transaction. The existing CRUD
    /// endpoints remain available; this replaces only this model's bindings
    /// and routes and publishes one fully validated runtime snapshot.
    pub(crate) async fn put_model_routing(
        &self,
        id: &str,
        input: &ModelRoutingWrite,
    ) -> Result<Mutation<ModelRoutingView>, ControlPlaneError> {
        self.write_model_routing(id, input, false).await
    }

    async fn write_model_routing(
        &self,
        id: &str,
        input: &ModelRoutingWrite,
        create: bool,
    ) -> Result<Mutation<ModelRoutingView>, ControlPlaneError> {
        validate_model_routing(id, input)?;
        let mut tx = self.begin_write().await?;
        let current: Option<CatalogStatus> =
            sqlx::query_scalar("SELECT status FROM logical_models WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        if !create && current.is_none() {
            return Err(ControlPlaneError::NotFound(format!(
                "logical model '{id}' not found"
            )));
        }
        let next_status = if input.lines.is_empty() {
            current.unwrap_or(CatalogStatus::Confirmed)
        } else {
            CatalogStatus::Confirmed
        };
        if let Some(mut status) = current {
            // Saving confirmed, available lines is the user's reconfirmation
            // action. Follow both catalog transitions inside this transaction;
            // any later line or snapshot failure rolls them back together.
            if status == CatalogStatus::Unavailable && next_status == CatalogStatus::Confirmed {
                validate_status_transition(status, CatalogStatus::Pending, "logical model")?;
                sqlx::query("UPDATE logical_models SET status='pending' WHERE id=$1")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                status = CatalogStatus::Pending;
            }
            validate_status_transition(status, next_status, "logical model")?;
        }
        // Keep confirmed bindings enabled even when the model is paused. The
        // final model enable flag is written before validation/publication;
        // no intermediate state can escape this transaction.
        let model_write = if create {
            "INSERT INTO logical_models (id,public_name,display_name,status,enabled,confirmed_at,unavailable_at,request_timeout_ms,max_retries) VALUES ($1,$2,$3,$6,TRUE,CASE WHEN $6='confirmed' THEN NOW() ELSE NULL END,CASE WHEN $6='unavailable' THEN NOW() ELSE NULL END,$4,$5)"
        } else {
            "UPDATE logical_models SET public_name=$2,display_name=$3,status=$6,enabled=TRUE,confirmed_at=CASE WHEN $6='confirmed' THEN COALESCE(confirmed_at,NOW()) ELSE confirmed_at END,unavailable_at=CASE WHEN $6='unavailable' THEN unavailable_at ELSE NULL END,request_timeout_ms=$4,max_retries=$5,updated_at=NOW() WHERE id=$1"
        };
        sqlx::query(model_write)
            .bind(id)
            .bind(&input.public_name)
            .bind(&input.display_name)
            .bind(input.request_timeout_ms)
            .bind(input.max_retries)
            .bind(next_status)
            .execute(&mut *tx)
            .await?;
        let route_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM routes WHERE logical_model_id=$1 ORDER BY id LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let route_id =
            route_id.unwrap_or_else(|| format!("model-routing-{}", uuid::Uuid::new_v4()));
        sqlx::query("DELETE FROM routes WHERE logical_model_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM model_bindings WHERE logical_model_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        let mut protocols = Vec::new();
        for (position, line) in input.lines.iter().enumerate() {
            let line_protocols: Vec<Protocol> = sqlx::query_scalar(
                "SELECT cap.protocol FROM source_model_capabilities cap JOIN source_models sm ON sm.source_id=cap.source_id AND sm.upstream_model_id=cap.upstream_model_id JOIN sources s ON s.id=cap.source_id JOIN accounts a ON a.id=$2 AND a.source_id=s.id WHERE cap.source_id=$1 AND cap.upstream_model_id=$3 AND sm.confirmation_status='confirmed' AND sm.availability_status='available' AND cap.status='confirmed' AND cap.mode IN ('native','adapter') AND s.enabled AND a.enabled ORDER BY cap.protocol",
            )
            .bind(&line.source_id)
            .bind(&line.account_id)
            .bind(&line.upstream_model_id)
            .fetch_all(&mut *tx)
            .await?;
            if line_protocols.is_empty() {
                return Err(ControlPlaneError::Validation(vec![format!(
                    "lines[{position}] requires an enabled source/account and a confirmed available source model with a confirmed routable protocol"
                )]));
            }
            for protocol in line_protocols {
                let binding = ModelBindingWrite {
                    logical_model_id: id.to_owned(),
                    source_id: line.source_id.clone(),
                    account_id: line.account_id.clone(),
                    upstream_model_id: line.upstream_model_id.clone(),
                    protocol,
                    status: CatalogStatus::Confirmed,
                    enabled: true,
                    priority: -(position as i32),
                };
                validate_binding_reference(&mut tx, &binding).await?;
                sqlx::query("INSERT INTO model_bindings (logical_model_id,source_id,account_id,upstream_model_id,protocol,status,enabled,priority,confirmed_at) VALUES ($1,$2,$3,$4,$5,'confirmed',TRUE,$6,NOW())")
                    .bind(id)
                    .bind(&line.source_id)
                    .bind(&line.account_id)
                    .bind(&line.upstream_model_id)
                    .bind(protocol)
                    .bind(binding.priority)
                    .execute(&mut *tx)
                    .await?;
                if !protocols.contains(&protocol) {
                    protocols.push(protocol);
                }
            }
        }
        protocols.sort_by_key(ToString::to_string);
        if !protocols.is_empty() {
            let route = RouteWrite {
                id: route_id,
                logical_model_id: id.to_owned(),
                protocols,
                strategy: "ordered_fallback".to_owned(),
                allow_lossy_conversion: false,
                enabled: true,
            };
            validate_route_reference(&mut tx, &route).await?;
            sqlx::query("INSERT INTO routes (id,logical_model_id,model_pattern,provider_id,protocols,primary_account_id,fallback_accounts,strategy,mode,adapter,allow_lossy_conversion,enabled) VALUES ($1,$2,$3,NULL,$4,NULL,'[]'::jsonb,'ordered_fallback','binding',NULL,FALSE,TRUE)")
                .bind(&route.id)
                .bind(id)
                .bind(&input.public_name)
                .bind(serde_json::to_value(&route.protocols)?)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE logical_models SET enabled=$2 WHERE id=$1")
            .bind(id)
            .bind(input.enabled)
            .execute(&mut *tx)
            .await?;
        let record = fetch_model_routing_tx(&mut tx, id).await?;
        self.finish_mutation(tx, record).await
    }
}

fn validate_model_routing(id: &str, input: &ModelRoutingWrite) -> Result<(), ControlPlaneError> {
    validate_logical_model_input(&LogicalModelWrite {
        id: id.to_owned(),
        public_name: input.public_name.clone(),
        display_name: input.display_name.clone(),
        status: CatalogStatus::Confirmed,
        metadata: json!({}),
        field_sources: json!({}),
        enabled: input.enabled,
    })?;
    let mut errors = Vec::new();
    if input.enabled && input.lines.is_empty() {
        errors.push("an enabled logical model requires at least one upstream line".to_owned());
    }
    if input.request_timeout_ms.is_some_and(|value| value <= 0) {
        errors.push("request_timeout_ms must be a positive integer or null".to_owned());
    }
    if input.max_retries.is_some_and(|value| value < 0) {
        errors.push("max_retries must be a nonnegative integer or null".to_owned());
    }
    let mut seen = HashSet::new();
    for (position, line) in input.lines.iter().enumerate() {
        if [&line.source_id, &line.account_id, &line.upstream_model_id]
            .iter()
            .any(|value| value.trim().is_empty())
        {
            errors.push(format!(
                "lines[{position}] source, account and upstream model are required"
            ));
        }
        if !seen.insert(line) {
            errors.push(format!("lines[{position}] duplicates an upstream line"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ControlPlaneError::Validation(errors))
    }
}

#[derive(sqlx::FromRow)]
struct RoutingBindingRow {
    source_id: String,
    account_id: String,
    upstream_model_id: String,
    protocol: Protocol,
    routable: bool,
}

async fn fetch_model_routing_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<ModelRoutingView, ControlPlaneError> {
    let logical_model = fetch_logical_model_tx(tx, id).await?;
    let request_timeout_ms = logical_model.request_timeout_ms;
    let max_retries = logical_model.max_retries;
    let strategies: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT strategy FROM routes WHERE logical_model_id=$1 ORDER BY strategy",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    let strategy = match strategies.as_slice() {
        [] => "ordered_fallback".to_owned(),
        [strategy] => strategy.clone(),
        _ => "mixed".to_owned(),
    };
    let rows = sqlx::query_as::<_, RoutingBindingRow>(
        "SELECT b.source_id,b.account_id,b.upstream_model_id,b.protocol,COALESCE(b.enabled AND b.status='confirmed' AND s.enabled AND a.enabled AND sm.confirmation_status='confirmed' AND sm.availability_status='available' AND cap.status='confirmed' AND cap.mode IN ('native','adapter') AND EXISTS(SELECT 1 FROM routes r WHERE r.logical_model_id=b.logical_model_id AND r.enabled AND r.protocols ? b.protocol::text),FALSE) AS routable FROM model_bindings b JOIN sources s ON s.id=b.source_id JOIN accounts a ON a.id=b.account_id AND a.source_id=b.source_id LEFT JOIN source_models sm ON sm.source_id=b.source_id AND sm.upstream_model_id=b.upstream_model_id LEFT JOIN source_model_capabilities cap ON cap.source_id=b.source_id AND cap.upstream_model_id=b.upstream_model_id AND cap.protocol=b.protocol WHERE b.logical_model_id=$1 ORDER BY b.priority DESC,b.id",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    let mut lines: Vec<ModelRoutingLineView> = Vec::new();
    let mut protocols = Vec::new();
    for row in rows {
        let index = lines
            .iter()
            .position(|line| {
                line.source_id == row.source_id
                    && line.account_id == row.account_id
                    && line.upstream_model_id == row.upstream_model_id
            })
            .unwrap_or_else(|| {
                lines.push(ModelRoutingLineView {
                    source_id: row.source_id,
                    account_id: row.account_id,
                    upstream_model_id: row.upstream_model_id,
                    protocols: Vec::new(),
                });
                lines.len() - 1
            });
        if row.routable && !lines[index].protocols.contains(&row.protocol) {
            lines[index].protocols.push(row.protocol);
            if !protocols.contains(&row.protocol) {
                protocols.push(row.protocol);
            }
        }
    }
    protocols.sort_by_key(ToString::to_string);
    for line in &mut lines {
        line.protocols.sort_by_key(ToString::to_string);
    }
    Ok(ModelRoutingView {
        logical_model,
        lines,
        protocols,
        strategy,
        request_timeout_ms,
        max_retries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_routing_rejects_invalid_request_limits_and_duplicate_lines() {
        let mut input: ModelRoutingWrite = serde_json::from_value(json!({
            "public_name":"model", "display_name":"Model", "enabled":true,
            "lines":[{"source_id":"source","account_id":"account","upstream_model_id":"upstream"}],
            "request_timeout_ms":null,"max_retries":null
        }))
        .unwrap();
        assert!(validate_model_routing("model", &input).is_ok());
        input.max_retries = Some(0);
        assert!(validate_model_routing("model", &input).is_ok());
        input.request_timeout_ms = Some(0);
        assert!(validate_model_routing("model", &input).is_err());
        input.request_timeout_ms = Some(1000);
        input.max_retries = Some(-1);
        assert!(validate_model_routing("model", &input).is_err());
        input.max_retries = None;
        input.lines.push(input.lines[0].clone());
        assert!(validate_model_routing("model", &input).is_err());
        input.lines.clear();
        assert!(validate_model_routing("model", &input).is_err());
        input.enabled = false;
        assert!(validate_model_routing("model", &input).is_ok());
    }
}
