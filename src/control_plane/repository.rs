use super::error::ControlPlaneError;
use super::types::{AccountView, LogicalModelView, ModelBindingView, RouteView, SourceView};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

pub(super) fn account_view_select(filtered: bool) -> &'static str {
    match filtered {
        true => "SELECT id,source_id,display_name,credential_env,(credential_env IS NOT NULL OR credential_ciphertext IS NOT NULL) AS credential_configured,enabled,weight,health_status,health_source,health_updated_at,consecutive_failures,cooldown_until,last_error,last_success_at,last_probe_at,last_probe_status,last_probe_error,created_at,updated_at FROM accounts WHERE id=$1 ORDER BY id",
        false => "SELECT id,source_id,display_name,credential_env,(credential_env IS NOT NULL OR credential_ciphertext IS NOT NULL) AS credential_configured,enabled,weight,health_status,health_source,health_updated_at,consecutive_failures,cooldown_until,last_error,last_success_at,last_probe_at,last_probe_status,last_probe_error,created_at,updated_at FROM accounts ORDER BY id",
    }
}

pub(super) fn logical_model_select(filtered: bool) -> &'static str {
    match filtered {
        true => "SELECT id,public_name,display_name,status,metadata,field_sources,enabled,request_timeout_ms,max_retries,confirmed_at,unavailable_at,created_at,updated_at FROM logical_models WHERE id=$1 ORDER BY id",
        false => "SELECT id,public_name,display_name,status,metadata,field_sources,enabled,request_timeout_ms,max_retries,confirmed_at,unavailable_at,created_at,updated_at FROM logical_models ORDER BY id",
    }
}

pub(super) fn model_binding_select(filtered: bool) -> &'static str {
    match filtered {
        true => "SELECT id,logical_model_id,source_id,account_id,upstream_model_id,protocol,status,enabled,priority,confirmed_at,unavailable_at,created_at,updated_at FROM model_bindings WHERE id=$1 ORDER BY id",
        false => "SELECT id,logical_model_id,source_id,account_id,upstream_model_id,protocol,status,enabled,priority,confirmed_at,unavailable_at,created_at,updated_at FROM model_bindings ORDER BY id",
    }
}

pub(super) fn route_view_select(filtered: bool) -> &'static str {
    match filtered {
        true => "SELECT r.id,r.logical_model_id,lm.public_name,r.protocols,r.strategy,r.allow_lossy_conversion,r.enabled,r.created_at,r.updated_at FROM routes r JOIN logical_models lm ON lm.id=r.logical_model_id WHERE r.id=$1 ORDER BY r.id",
        false => "SELECT r.id,r.logical_model_id,lm.public_name,r.protocols,r.strategy,r.allow_lossy_conversion,r.enabled,r.created_at,r.updated_at FROM routes r JOIN logical_models lm ON lm.id=r.logical_model_id ORDER BY r.id",
    }
}

pub(super) async fn fetch_source(pool: &PgPool, id: &str) -> Result<SourceView, ControlPlaneError> {
    sqlx::query_as::<_, SourceView>(
        "SELECT id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled,created_at,updated_at FROM sources WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ControlPlaneError::NotFound(format!("source '{id}' not found")))
}

pub(super) async fn fetch_account_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<AccountView, ControlPlaneError> {
    sqlx::query_as::<_, AccountView>(account_view_select(true))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ControlPlaneError::NotFound(format!("account '{id}' not found")))
}

pub(super) async fn fetch_logical_model_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<LogicalModelView, ControlPlaneError> {
    sqlx::query_as::<_, LogicalModelView>(logical_model_select(true))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ControlPlaneError::NotFound(format!("logical model '{id}' not found")))
}

pub(super) async fn fetch_model_binding_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: i64,
) -> Result<ModelBindingView, ControlPlaneError> {
    sqlx::query_as::<_, ModelBindingView>(model_binding_select(true))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ControlPlaneError::NotFound(format!("model binding '{id}' not found")))
}

pub(super) async fn fetch_route_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<RouteView, ControlPlaneError> {
    sqlx::query_as::<_, RouteView>(route_view_select(true))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ControlPlaneError::NotFound(format!("route '{id}' not found")))
}

pub(super) async fn row_exists(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    column: &str,
    id: &str,
) -> Result<bool, ControlPlaneError> {
    let query = format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {column}=$1)");
    Ok(sqlx::query_scalar(&query)
        .bind(id)
        .fetch_one(&mut **tx)
        .await?)
}

pub(super) async fn ensure_source_exists(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<(), ControlPlaneError> {
    if !row_exists(tx, "sources", "id", id).await? {
        return Err(ControlPlaneError::NotFound(format!(
            "source '{id}' not found"
        )));
    }
    Ok(())
}

/// Apply an operator enable/disable transition while the account is locked by
/// the surrounding SERIALIZABLE control-plane transaction.
pub(super) async fn apply_manual_health_transition(
    tx: &mut Transaction<'_, Postgres>,
    account_id: &str,
    enabled: bool,
    observed_at: DateTime<Utc>,
) -> Result<(), ControlPlaneError> {
    let status = if enabled { "unknown" } else { "disabled" };
    sqlx::query(
        "UPDATE accounts SET health_status=$2,cooldown_until=NULL,consecutive_failures=0,failure_window_started_at=NULL,last_error=NULL,last_success_at=NULL,health_source='manual',health_updated_at=$3,last_probe_error=NULL WHERE id=$1",
    )
    .bind(account_id)
    .bind(status)
    .bind(observed_at)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO account_health_events (account_id,status,source,observed_at,cooldown_until,consecutive_failures) VALUES ($1,$2,'manual',$3,NULL,0)",
    )
    .bind(account_id)
    .bind(status)
    .bind(observed_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(super) async fn apply_manual_health_transition_for_source(
    tx: &mut Transaction<'_, Postgres>,
    source_id: &str,
    enabled: bool,
    observed_at: DateTime<Utc>,
) -> Result<(), ControlPlaneError> {
    let status = if enabled { "unknown" } else { "disabled" };
    sqlx::query(
        "UPDATE accounts SET health_status=$2,cooldown_until=NULL,consecutive_failures=0,failure_window_started_at=NULL,last_error=NULL,last_success_at=NULL,health_source='manual',health_updated_at=$3,last_probe_error=NULL WHERE source_id=$1",
    )
    .bind(source_id)
    .bind(status)
    .bind(observed_at)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO account_health_events (account_id,status,source,observed_at,cooldown_until,consecutive_failures) SELECT id,$2,'manual',$3,NULL,0 FROM accounts WHERE source_id=$1",
    )
    .bind(source_id)
    .bind(status)
    .bind(observed_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
