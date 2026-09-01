use super::model_catalog::{CatalogAvailability, CatalogStatus, SourceProtocolMode};
use crate::domain::{
    catalog::PublishedModel,
    config::{
        adapter_definition, AccountConfig, Capabilities, CapabilityMode, GatewayConfig,
        ProtocolCapability, ProtocolCapabilityMatrix, ProtocolMode, ProviderConfig, RouteConfig,
    },
    protocol::Protocol,
    provider_preset::ProviderPresetDefinition,
    routing::{intersect_capabilities, join_endpoint, RouteResolver, RuntimeBinding, RuntimeRoute},
};
use crate::infra::{
    db::Database,
    source_url::{SourceUrlPolicy, SourceUrlPolicyError},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Transaction};
use std::{collections::HashMap, error::Error, fmt, sync::Arc};

#[derive(Debug)]
pub enum ControlPlaneError {
    Database(sqlx::Error),
    Json(serde_json::Error),
    NotFound(String),
    Conflict(String),
    Validation(Vec<String>),
}

impl ControlPlaneError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Database(_) => "database_error",
            Self::Json(_) | Self::Validation(_) => "validation_failed",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Validation(errors) => errors.join("; "),
            _ => self.to_string(),
        }
    }
}

impl fmt::Display for ControlPlaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::NotFound(message) | Self::Conflict(message) => f.write_str(message),
            Self::Validation(errors) => f.write_str(&errors.join("; ")),
        }
    }
}

impl Error for ControlPlaneError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for ControlPlaneError {
    fn from(value: sqlx::Error) -> Self {
        if let sqlx::Error::Database(database) = &value {
            match database.code().as_deref() {
                Some("23505" | "40001") => return Self::Conflict(database.message().to_owned()),
                Some("23503" | "23514" | "22P02") => {
                    return Self::Validation(vec![database.message().to_owned()])
                }
                _ => {}
            }
        }
        Self::Database(value)
    }
}

impl From<serde_json::Error> for ControlPlaneError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Clone)]
pub struct RuntimeSnapshot {
    pub config: Arc<GatewayConfig>,
    pub resolver: RouteResolver,
    pub models: Arc<Vec<PublishedModel>>,
    pub revision: i64,
    pub generated_at: DateTime<Utc>,
}

impl RuntimeSnapshot {
    fn new(
        config: GatewayConfig,
        routes: Vec<RuntimeRoute>,
        models: Vec<PublishedModel>,
        revision: i64,
        generated_at: DateTime<Utc>,
    ) -> Self {
        let config = Arc::new(config);
        Self {
            resolver: RouteResolver::from_runtime(config.clone(), routes),
            config,
            models: Arc::new(models),
            revision,
            generated_at,
        }
    }
}

#[derive(Clone)]
pub struct ControlPlane {
    pool: PgPool,
    listen_addr: String,
    source_url_policy: Arc<SourceUrlPolicy>,
}

impl ControlPlane {
    #[cfg(test)]
    pub fn new(database: &Database, listen_addr: impl Into<String>) -> Self {
        Self::with_url_policy(database, listen_addr, Arc::new(SourceUrlPolicy::default()))
    }

    pub fn with_url_policy(
        database: &Database,
        listen_addr: impl Into<String>,
        source_url_policy: Arc<SourceUrlPolicy>,
    ) -> Self {
        Self {
            pool: database.pool().clone(),
            listen_addr: listen_addr.into(),
            source_url_policy,
        }
    }

    async fn begin_write(&self) -> Result<Transaction<'_, Postgres>, ControlPlaneError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    async fn finish_write(
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

    pub async fn load_snapshot(&self) -> Result<RuntimeSnapshot, ControlPlaneError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;
        validate_persisted_source_urls(&mut tx, &self.source_url_policy).await?;
        validate_capability_chains(&mut tx).await?;
        let (revision, generated_at): (i64, DateTime<Utc>) = sqlx::query_as(
            "SELECT revision,updated_at FROM runtime_snapshot_state WHERE singleton=TRUE",
        )
        .fetch_one(&mut *tx)
        .await?;
        let snapshot = build_snapshot(&mut tx, &self.listen_addr, revision, generated_at).await?;
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

    pub async fn is_empty(&self) -> Result<bool, ControlPlaneError> {
        let row_count: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM sources) + (SELECT COUNT(*) FROM accounts) + \
                    (SELECT COUNT(*) FROM logical_models) + (SELECT COUNT(*) FROM model_bindings) + \
                    (SELECT COUNT(*) FROM routes)",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row_count == 0)
    }

    pub async fn initialize_from_config(
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

fn account_view_select(filter: Option<&str>) -> &'static str {
    match filter {
        Some(_) => "SELECT id,source_id,display_name,credential_env,(credential_env IS NOT NULL OR credential_ciphertext IS NOT NULL) AS credential_configured,enabled,weight,health_status,health_source,health_updated_at,consecutive_failures,cooldown_until,last_error,last_success_at,last_probe_at,last_probe_status,last_probe_error,created_at,updated_at FROM accounts WHERE id=$1 ORDER BY id",
        None => "SELECT id,source_id,display_name,credential_env,(credential_env IS NOT NULL OR credential_ciphertext IS NOT NULL) AS credential_configured,enabled,weight,health_status,health_source,health_updated_at,consecutive_failures,cooldown_until,last_error,last_success_at,last_probe_at,last_probe_status,last_probe_error,created_at,updated_at FROM accounts ORDER BY id",
    }
}

fn logical_model_select(filter: Option<&str>) -> &'static str {
    match filter {
        Some(_) => "SELECT id,public_name,display_name,status,metadata,field_sources,enabled,confirmed_at,unavailable_at,created_at,updated_at FROM logical_models WHERE id=$1 ORDER BY id",
        None => "SELECT id,public_name,display_name,status,metadata,field_sources,enabled,confirmed_at,unavailable_at,created_at,updated_at FROM logical_models ORDER BY id",
    }
}

fn model_binding_select(filter: Option<&str>) -> &'static str {
    match filter {
        Some(_) => "SELECT id,logical_model_id,source_id,account_id,upstream_model_id,protocol,status,enabled,priority,confirmed_at,unavailable_at,created_at,updated_at FROM model_bindings WHERE id=$1 ORDER BY id",
        None => "SELECT id,logical_model_id,source_id,account_id,upstream_model_id,protocol,status,enabled,priority,confirmed_at,unavailable_at,created_at,updated_at FROM model_bindings ORDER BY id",
    }
}

fn route_view_select(filter: Option<&str>) -> &'static str {
    match filter {
        Some(_) => "SELECT r.id,r.logical_model_id,lm.public_name,r.protocols,r.strategy,r.allow_lossy_conversion,r.enabled,r.created_at,r.updated_at FROM routes r JOIN logical_models lm ON lm.id=r.logical_model_id WHERE r.id=$1 ORDER BY r.id",
        None => "SELECT r.id,r.logical_model_id,lm.public_name,r.protocols,r.strategy,r.allow_lossy_conversion,r.enabled,r.created_at,r.updated_at FROM routes r JOIN logical_models lm ON lm.id=r.logical_model_id ORDER BY r.id",
    }
}

async fn fetch_source(pool: &PgPool, id: &str) -> Result<SourceView, ControlPlaneError> {
    sqlx::query_as::<_, SourceView>(
        "SELECT id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled,created_at,updated_at FROM sources WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ControlPlaneError::NotFound(format!("source '{id}' not found")))
}

async fn fetch_account_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<AccountView, ControlPlaneError> {
    sqlx::query_as::<_, AccountView>(account_view_select(Some("WHERE id=$1")))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ControlPlaneError::NotFound(format!("account '{id}' not found")))
}

async fn fetch_logical_model_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<LogicalModelView, ControlPlaneError> {
    sqlx::query_as::<_, LogicalModelView>(logical_model_select(Some("WHERE id=$1")))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ControlPlaneError::NotFound(format!("logical model '{id}' not found")))
}

async fn fetch_model_binding_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: i64,
) -> Result<ModelBindingView, ControlPlaneError> {
    sqlx::query_as::<_, ModelBindingView>(model_binding_select(Some("WHERE id=$1")))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ControlPlaneError::NotFound(format!("model binding '{id}' not found")))
}

async fn fetch_route_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<RouteView, ControlPlaneError> {
    sqlx::query_as::<_, RouteView>(route_view_select(Some("WHERE r.id=$1")))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ControlPlaneError::NotFound(format!("route '{id}' not found")))
}

async fn row_exists(
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

async fn ensure_source_exists(
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
async fn apply_manual_health_transition(
    tx: &mut Transaction<'_, Postgres>,
    account_id: &str,
    enabled: bool,
    observed_at: DateTime<Utc>,
) -> Result<(), ControlPlaneError> {
    let status = if enabled { "unknown" } else { "disabled" };
    sqlx::query(
        "UPDATE accounts SET health_status=$2,cooldown_until=NULL,consecutive_failures=0,last_error=NULL,last_success_at=NULL,health_source='manual',health_updated_at=$3,last_probe_error=NULL WHERE id=$1",
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

async fn apply_manual_health_transition_for_source(
    tx: &mut Transaction<'_, Postgres>,
    source_id: &str,
    enabled: bool,
    observed_at: DateTime<Utc>,
) -> Result<(), ControlPlaneError> {
    let status = if enabled { "unknown" } else { "disabled" };
    sqlx::query(
        "UPDATE accounts SET health_status=$2,cooldown_until=NULL,consecutive_failures=0,last_error=NULL,last_success_at=NULL,health_source='manual',health_updated_at=$3,last_probe_error=NULL WHERE source_id=$1",
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

fn validate_resource_id(path: &str, body: &str, kind: &str) -> Result<(), ControlPlaneError> {
    if path != body {
        return Err(ControlPlaneError::Validation(vec![format!(
            "{kind} id in path and body must match"
        )]));
    }
    Ok(())
}

fn validate_nonempty(value: &str, field: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{field} must not be empty"));
    }
}

fn normalize_object(value: Value, field: &str) -> Result<Value, ControlPlaneError> {
    if value.is_null() {
        return Ok(json!({}));
    }
    if !value.is_object() {
        return Err(ControlPlaneError::Validation(vec![format!(
            "{field} must be a JSON object"
        )]));
    }
    Ok(value)
}

fn validate_source_input(
    input: &SourceWrite,
    source_url_policy: &SourceUrlPolicy,
) -> Result<(), ControlPlaneError> {
    let mut errors = Vec::new();
    validate_nonempty(&input.id, "source.id", &mut errors);
    validate_nonempty(&input.display_name, "source.display_name", &mut errors);
    validate_nonempty(
        &input.provider_preset_id,
        "source.provider_preset_id",
        &mut errors,
    );
    if input.provider_preset_version <= 0 {
        errors.push("source.provider_preset_version must be positive".to_owned());
    }
    if let Err(error) = source_url_policy.validate_base_url(&input.base_url) {
        errors.push(source_url_validation_message("source.base_url", error));
    }
    if !input.auth_config.is_null() && !input.auth_config.is_object() {
        errors.push("source.auth_config must be a JSON object".to_owned());
    }
    if !input.protocol_capabilities.is_null() && !input.protocol_capabilities.is_object() {
        errors.push("source.protocol_capabilities must be a JSON object".to_owned());
    }
    for (protocol, endpoint) in &input.endpoints {
        if endpoint.trim().is_empty() {
            errors.push(format!("source.endpoints.{protocol} must not be empty"));
        }
    }
    let protocol_capabilities = match serde_json::from_value::<ProtocolCapabilityMatrix>(
        input.protocol_capabilities.clone(),
    ) {
        Ok(matrix) => matrix,
        Err(error) => {
            errors.push(format!("source.protocol_capabilities is invalid: {error}"));
            ProtocolCapabilityMatrix::new()
        }
    };
    let provider = ProviderConfig {
        id: input.id.clone(),
        name: input.display_name.clone(),
        base_url: input.base_url.clone(),
        models: Vec::new(),
        native_protocols: Vec::new(),
        endpoints: input.endpoints.clone(),
        capabilities: Capabilities::default(),
        protocol_capabilities,
        model_overrides: HashMap::new(),
    };
    let config = GatewayConfig {
        listen_addr: "127.0.0.1:0".to_owned(),
        providers: vec![provider],
        accounts: Vec::new(),
        routes: Vec::new(),
    };
    if let Err(mut validation_errors) = config.validate() {
        errors.append(&mut validation_errors);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ControlPlaneError::Validation(errors))
    }
}

fn source_url_validation_message(field: &str, error: SourceUrlPolicyError) -> String {
    if error == SourceUrlPolicyError::InvalidUrl {
        format!("{field} must be an http(s) URL without credentials, query, or fragment")
    } else {
        format!("{field} is blocked by the server Source URL policy")
    }
}

async fn validate_persisted_source_urls(
    tx: &mut Transaction<'_, Postgres>,
    source_url_policy: &SourceUrlPolicy,
) -> Result<(), ControlPlaneError> {
    let sources =
        sqlx::query_as::<_, (String, String)>("SELECT id,base_url FROM sources ORDER BY id")
            .fetch_all(&mut **tx)
            .await?;
    let errors = sources
        .into_iter()
        .filter_map(|(id, base_url)| {
            source_url_policy
                .validate_base_url(&base_url)
                .err()
                .map(|error| {
                    source_url_validation_message(&format!("source '{id}' base_url"), error)
                })
        })
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ControlPlaneError::Validation(errors))
    }
}

fn validate_account_input(input: &AccountWrite) -> Result<(), ControlPlaneError> {
    let mut errors = Vec::new();
    validate_nonempty(&input.id, "account.id", &mut errors);
    validate_nonempty(&input.source_id, "account.source_id", &mut errors);
    validate_nonempty(&input.display_name, "account.display_name", &mut errors);
    if input.weight <= 0 {
        errors.push("account.weight must be positive".to_owned());
    }
    let env_set = input
        .credential_env
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let encrypted_set = input
        .credential_ciphertext
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    if env_set == encrypted_set {
        errors.push(
            "account must set exactly one of credential_env or credential_ciphertext".to_owned(),
        );
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ControlPlaneError::Validation(errors))
    }
}

fn validate_logical_model_input(input: &LogicalModelWrite) -> Result<(), ControlPlaneError> {
    let mut errors = Vec::new();
    validate_nonempty(&input.id, "logical_model.id", &mut errors);
    validate_nonempty(&input.public_name, "logical_model.public_name", &mut errors);
    validate_nonempty(
        &input.display_name,
        "logical_model.display_name",
        &mut errors,
    );
    if !input.metadata.is_null() && !input.metadata.is_object() {
        errors.push("logical_model.metadata must be a JSON object".to_owned());
    }
    if !input.field_sources.is_null() && !input.field_sources.is_object() {
        errors.push("logical_model.field_sources must be a JSON object".to_owned());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ControlPlaneError::Validation(errors))
    }
}

fn validate_binding_input_shape(input: &ModelBindingWrite) -> Result<(), ControlPlaneError> {
    let mut errors = Vec::new();
    validate_nonempty(
        &input.logical_model_id,
        "model_binding.logical_model_id",
        &mut errors,
    );
    validate_nonempty(&input.source_id, "model_binding.source_id", &mut errors);
    validate_nonempty(&input.account_id, "model_binding.account_id", &mut errors);
    validate_nonempty(
        &input.upstream_model_id,
        "model_binding.upstream_model_id",
        &mut errors,
    );
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ControlPlaneError::Validation(errors))
    }
}

fn validate_route_input_shape(input: &RouteWrite) -> Result<(), ControlPlaneError> {
    let mut errors = Vec::new();
    validate_nonempty(&input.id, "route.id", &mut errors);
    validate_nonempty(
        &input.logical_model_id,
        "route.logical_model_id",
        &mut errors,
    );
    validate_nonempty(&input.strategy, "route.strategy", &mut errors);
    if input.protocols.is_empty() {
        errors.push("route.protocols must contain at least one protocol".to_owned());
    }
    let mut deduplicated = input.protocols.clone();
    deduplicated.sort_by_key(ToString::to_string);
    deduplicated.dedup();
    if deduplicated.len() != input.protocols.len() {
        errors.push("route.protocols must not contain duplicates".to_owned());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ControlPlaneError::Validation(errors))
    }
}

fn validate_status_transition(
    current: CatalogStatus,
    next: CatalogStatus,
    kind: &str,
) -> Result<(), ControlPlaneError> {
    if !current.can_transition_to(next) {
        return Err(ControlPlaneError::Validation(vec![format!(
            "{kind} status cannot transition from {current:?} to {next:?}; move unavailable records to pending before reconfirming"
        )]));
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct BindingValidationRow {
    logical_status: CatalogStatus,
    logical_enabled: bool,
    source_enabled: bool,
    account_source_id: String,
    account_enabled: bool,
    model_confirmation: CatalogStatus,
    model_availability: CatalogAvailability,
    capability_status: CatalogStatus,
    capability_mode: SourceProtocolMode,
}

async fn validate_binding_reference(
    tx: &mut Transaction<'_, Postgres>,
    input: &ModelBindingWrite,
) -> Result<(), ControlPlaneError> {
    let row = sqlx::query_as::<_, BindingValidationRow>(
        "SELECT lm.status AS logical_status,lm.enabled AS logical_enabled,s.enabled AS source_enabled,a.source_id AS account_source_id,a.enabled AS account_enabled,sm.confirmation_status AS model_confirmation,sm.availability_status AS model_availability,cap.status AS capability_status,cap.mode AS capability_mode FROM logical_models lm CROSS JOIN sources s JOIN accounts a ON a.id=$3 JOIN source_models sm ON sm.source_id=s.id AND sm.upstream_model_id=$4 JOIN source_model_capabilities cap ON cap.source_id=s.id AND cap.upstream_model_id=sm.upstream_model_id AND cap.protocol=$5 WHERE lm.id=$1 AND s.id=$2",
    )
    .bind(&input.logical_model_id)
    .bind(&input.source_id)
    .bind(&input.account_id)
    .bind(&input.upstream_model_id)
    .bind(input.protocol)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        ControlPlaneError::Validation(vec![
            "model binding references a missing LogicalModel, Source, Account, SourceModel, or SourceModelCapability"
                .to_owned(),
        ])
    })?;
    if row.account_source_id != input.source_id {
        return Err(ControlPlaneError::Validation(vec![format!(
            "account '{}' does not belong to source '{}'",
            input.account_id, input.source_id
        )]));
    }
    if input.status == CatalogStatus::Confirmed && input.enabled {
        let mut errors = Vec::new();
        if row.logical_status != CatalogStatus::Confirmed || !row.logical_enabled {
            errors.push(
                "confirmed enabled binding requires a confirmed enabled LogicalModel".to_owned(),
            );
        }
        if !row.source_enabled {
            errors.push("confirmed enabled binding requires an enabled Source".to_owned());
        }
        if !row.account_enabled {
            errors.push("confirmed enabled binding requires an enabled Account".to_owned());
        }
        if row.model_confirmation != CatalogStatus::Confirmed
            || row.model_availability != CatalogAvailability::Available
        {
            errors.push(
                "confirmed enabled binding requires a confirmed available SourceModel".to_owned(),
            );
        }
        if row.capability_status != CatalogStatus::Confirmed || !row.capability_mode.is_routable() {
            errors.push(
                "confirmed enabled binding requires a confirmed native or adapter capability"
                    .to_owned(),
            );
        }
        if !errors.is_empty() {
            return Err(ControlPlaneError::Validation(errors));
        }
    }
    Ok(())
}

async fn validate_route_reference(
    tx: &mut Transaction<'_, Postgres>,
    input: &RouteWrite,
) -> Result<(), ControlPlaneError> {
    if !row_exists(tx, "logical_models", "id", &input.logical_model_id).await? {
        return Err(ControlPlaneError::NotFound(format!(
            "logical model '{}' not found",
            input.logical_model_id
        )));
    }
    let mut errors = Vec::new();
    for protocol in &input.protocols {
        if input.enabled {
            let conflict: Option<String> = sqlx::query_scalar(
                "SELECT id FROM routes WHERE logical_model_id=$1 AND id<>$2 AND enabled AND protocols ? $3 LIMIT 1",
            )
            .bind(&input.logical_model_id)
            .bind(&input.id)
            .bind(protocol.to_string())
            .fetch_optional(&mut **tx)
            .await?;
            if let Some(conflict) = conflict {
                errors.push(format!(
                    "route protocol {protocol} conflicts with enabled route '{conflict}'"
                ));
            }
        }
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM model_bindings b JOIN source_models sm ON sm.source_id=b.source_id AND sm.upstream_model_id=b.upstream_model_id JOIN source_model_capabilities cap ON cap.source_id=b.source_id AND cap.upstream_model_id=b.upstream_model_id AND cap.protocol=b.protocol WHERE b.logical_model_id=$1 AND b.protocol=$2 AND b.status='confirmed' AND sm.confirmation_status='confirmed' AND sm.availability_status='available' AND cap.status='confirmed' AND cap.mode IN ('native','adapter'))",
        )
        .bind(&input.logical_model_id)
        .bind(*protocol)
        .fetch_one(&mut **tx)
        .await?;
        if !exists {
            errors.push(format!(
                "route protocol {protocol} has no confirmed, available model binding"
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ControlPlaneError::Validation(errors))
    }
}

#[derive(sqlx::FromRow)]
struct CapabilityValidationRow {
    source_id: String,
    upstream_model_id: String,
    protocol: Protocol,
    mode: SourceProtocolMode,
    source_protocol: Option<Protocol>,
    adapter: Option<String>,
    endpoints: Value,
}

async fn validate_capability_chains(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), ControlPlaneError> {
    let rows = sqlx::query_as::<_, CapabilityValidationRow>(
        "SELECT cap.source_id,cap.upstream_model_id,cap.protocol,cap.mode,cap.source_protocol,cap.adapter,s.endpoints FROM source_model_capabilities cap JOIN sources s ON s.id=cap.source_id WHERE cap.status='confirmed' AND cap.mode IN ('native','adapter') ORDER BY cap.source_id,cap.upstream_model_id,cap.protocol",
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut errors = Vec::new();
    for row in rows {
        let endpoints: HashMap<Protocol, String> = match serde_json::from_value(row.endpoints) {
            Ok(endpoints) => endpoints,
            Err(error) => {
                errors.push(format!(
                    "source '{}'.endpoints is invalid: {error}",
                    row.source_id
                ));
                continue;
            }
        };
        let upstream = match row.mode {
            SourceProtocolMode::Native => row.protocol,
            SourceProtocolMode::Adapter => {
                let Some(source_protocol) = row.source_protocol else {
                    errors.push(format!(
                        "capability {}/{}/{} requires source_protocol",
                        row.source_id, row.upstream_model_id, row.protocol
                    ));
                    continue;
                };
                let Some(adapter_name) = row.adapter.as_deref() else {
                    errors.push(format!(
                        "capability {}/{}/{} requires adapter",
                        row.source_id, row.upstream_model_id, row.protocol
                    ));
                    continue;
                };
                match adapter_definition(adapter_name) {
                    Some(definition)
                        if definition.from_protocol == row.protocol
                            && definition.to_protocol == source_protocol => {}
                    Some(definition) => errors.push(format!(
                        "adapter '{adapter_name}' implements {} -> {}, not {} -> {source_protocol}",
                        definition.from_protocol, definition.to_protocol, row.protocol
                    )),
                    None => errors.push(format!("unknown adapter '{adapter_name}'")),
                }
                let source_native: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM source_model_capabilities WHERE source_id=$1 AND upstream_model_id=$2 AND protocol=$3 AND status='confirmed' AND mode='native')",
                )
                .bind(&row.source_id)
                .bind(&row.upstream_model_id)
                .bind(source_protocol)
                .fetch_one(&mut **tx)
                .await?;
                if !source_native {
                    errors.push(format!(
                        "adapter capability {}/{}/{} requires confirmed native source protocol {source_protocol}",
                        row.source_id, row.upstream_model_id, row.protocol
                    ));
                }
                source_protocol
            }
            SourceProtocolMode::Unknown | SourceProtocolMode::Unsupported => continue,
        };
        if !endpoints
            .get(&upstream)
            .is_some_and(|endpoint| !endpoint.trim().is_empty())
        {
            errors.push(format!(
                "source '{}' has no endpoint for upstream protocol {upstream}",
                row.source_id
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ControlPlaneError::Validation(errors))
    }
}

#[derive(sqlx::FromRow)]
struct SnapshotRow {
    route_id: String,
    public_name: String,
    display_name: String,
    route_protocols: Value,
    allow_lossy_conversion: bool,
    binding_id: i64,
    source_id: String,
    provider_preset_id: String,
    source_display_name: String,
    base_url: String,
    endpoints: Value,
    account_id: String,
    account_display_name: String,
    credential_env: Option<String>,
    credential_ciphertext: Option<String>,
    account_enabled: bool,
    source_enabled: bool,
    weight: i32,
    upstream_model_id: String,
    protocol: Protocol,
    mode: SourceProtocolMode,
    source_protocol: Option<Protocol>,
    adapter: Option<String>,
    feature_capabilities: Value,
}

async fn build_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    listen_addr: &str,
    revision: i64,
    generated_at: DateTime<Utc>,
) -> Result<RuntimeSnapshot, ControlPlaneError> {
    let rows = sqlx::query_as::<_, SnapshotRow>(
        "SELECT r.id AS route_id,lm.public_name,lm.display_name,r.protocols AS route_protocols,r.allow_lossy_conversion,b.id AS binding_id,b.source_id,s.provider_preset_id,s.display_name AS source_display_name,s.base_url,s.endpoints,b.account_id,a.display_name AS account_display_name,a.credential_env,a.credential_ciphertext,a.enabled AS account_enabled,s.enabled AS source_enabled,a.weight,b.upstream_model_id,b.protocol,cap.mode,cap.source_protocol,cap.adapter,cap.feature_capabilities FROM routes r JOIN logical_models lm ON lm.id=r.logical_model_id JOIN model_bindings b ON b.logical_model_id=lm.id JOIN sources s ON s.id=b.source_id JOIN accounts a ON a.id=b.account_id AND a.source_id=b.source_id JOIN source_models sm ON sm.source_id=b.source_id AND sm.upstream_model_id=b.upstream_model_id JOIN source_model_capabilities cap ON cap.source_id=b.source_id AND cap.upstream_model_id=b.upstream_model_id AND cap.protocol=b.protocol WHERE r.enabled AND lm.enabled AND lm.status='confirmed' AND b.enabled AND b.status='confirmed' AND sm.confirmation_status='confirmed' AND sm.availability_status='available' AND cap.status='confirmed' AND cap.mode IN ('native','adapter') ORDER BY r.id,b.protocol,b.priority DESC,CASE cap.mode WHEN 'native' THEN 0 ELSE 1 END,b.id",
    )
    .fetch_all(&mut **tx)
    .await?;

    let mut providers: HashMap<String, ProviderConfig> = HashMap::new();
    let mut accounts: HashMap<String, AccountConfig> = HashMap::new();
    let mut routes: Vec<RuntimeRoute> = Vec::new();
    let mut models: Vec<PublishedModel> = Vec::new();
    let mut errors = Vec::new();

    for row in rows {
        let route_protocols: Vec<Protocol> = match serde_json::from_value(row.route_protocols) {
            Ok(protocols) => protocols,
            Err(error) => {
                errors.push(format!(
                    "route '{}'.protocols is invalid: {error}",
                    row.route_id
                ));
                continue;
            }
        };
        if !route_protocols.contains(&row.protocol) {
            continue;
        }
        let endpoints: HashMap<Protocol, String> = match serde_json::from_value(row.endpoints) {
            Ok(endpoints) => endpoints,
            Err(error) => {
                errors.push(format!(
                    "source '{}'.endpoints is invalid: {error}",
                    row.source_id
                ));
                continue;
            }
        };
        let protocol_upstream = match row.mode {
            SourceProtocolMode::Native => row.protocol,
            SourceProtocolMode::Adapter => match row.source_protocol {
                Some(protocol) => protocol,
                None => {
                    errors.push(format!(
                        "binding {} adapter capability is missing source_protocol",
                        row.binding_id
                    ));
                    continue;
                }
            },
            SourceProtocolMode::Unknown | SourceProtocolMode::Unsupported => continue,
        };
        let Some(endpoint) = endpoints
            .get(&protocol_upstream)
            .filter(|endpoint| !endpoint.trim().is_empty())
        else {
            errors.push(format!(
                "binding {} source '{}' has no endpoint for {protocol_upstream}",
                row.binding_id, row.source_id
            ));
            continue;
        };
        let source_capabilities = match capabilities_from_catalog(&row.feature_capabilities) {
            Ok(capabilities) => capabilities,
            Err(error) => {
                errors.push(format!(
                    "binding {} feature_capabilities is invalid: {error}",
                    row.binding_id
                ));
                continue;
            }
        };
        let adapter_features = if row.mode == SourceProtocolMode::Adapter {
            let Some(name) = row.adapter.as_deref() else {
                errors.push(format!("binding {} is missing adapter", row.binding_id));
                continue;
            };
            let Some(definition) = adapter_definition(name) else {
                errors.push(format!(
                    "binding {} uses unknown adapter '{name}'",
                    row.binding_id
                ));
                continue;
            };
            if definition.from_protocol != row.protocol
                || definition.to_protocol != protocol_upstream
            {
                errors.push(format!(
                    "binding {} adapter '{name}' direction does not match {} -> {protocol_upstream}",
                    row.binding_id, row.protocol
                ));
                continue;
            }
            Some(definition.features)
        } else {
            None
        };
        let (effective_capabilities, degraded_features) = match intersect_capabilities(
            &source_capabilities,
            adapter_features.as_ref(),
            row.allow_lossy_conversion,
        ) {
            Ok(result) => result,
            Err(feature) => {
                errors.push(format!(
                    "route '{}' binding {} would lose feature '{feature}' without allow_lossy_conversion",
                    row.route_id, row.binding_id
                ));
                continue;
            }
        };
        let provider = providers
            .entry(row.source_id.clone())
            .or_insert_with(|| ProviderConfig {
                id: row.source_id.clone(),
                name: row.source_display_name.clone(),
                base_url: row.base_url.clone(),
                models: Vec::new(),
                native_protocols: Vec::new(),
                endpoints: endpoints.clone(),
                capabilities: Capabilities::default(),
                protocol_capabilities: HashMap::new(),
                model_overrides: HashMap::new(),
            });
        if !provider.models.contains(&row.upstream_model_id) {
            provider.models.push(row.upstream_model_id.clone());
        }
        if row.weight <= 0 {
            errors.push(format!(
                "account '{}' has non-positive weight",
                row.account_id
            ));
            continue;
        }
        let account = accounts
            .entry(row.account_id.clone())
            .or_insert_with(|| AccountConfig {
                id: row.account_id.clone(),
                provider_id: row.source_id.clone(),
                display_name: row.account_display_name.clone(),
                credential_env: row.credential_env.clone(),
                credential_ciphertext: row.credential_ciphertext.clone(),
                credential: None,
                enabled: row.account_enabled && row.source_enabled,
                weight: row.weight as u32,
                protocol_capabilities: HashMap::new(),
                capabilities: None,
                model_overrides: HashMap::new(),
                model_map: HashMap::new(),
            });
        if let Some(existing) = account.model_map.get(&row.public_name) {
            if existing != &row.upstream_model_id {
                errors.push(format!(
                    "account '{}' has multiple upstream models for logical model '{}'",
                    row.account_id, row.public_name
                ));
                continue;
            }
        } else {
            account
                .model_map
                .insert(row.public_name.clone(), row.upstream_model_id.clone());
        }
        let runtime_binding = RuntimeBinding {
            binding_id: row.binding_id,
            source_id: row.source_id.clone(),
            provider_id: row.provider_preset_id,
            account_id: row.account_id.clone(),
            upstream_model_id: row.upstream_model_id,
            protocol_upstream,
            upstream_endpoint: join_endpoint(&row.base_url, endpoint),
            mode: match row.mode {
                SourceProtocolMode::Native => "native",
                SourceProtocolMode::Adapter => "adapter",
                SourceProtocolMode::Unknown | SourceProtocolMode::Unsupported => unreachable!(),
            }
            .to_owned(),
            adapter: row.adapter,
            effective_capabilities,
            degraded_features,
        };
        if let Some(route) = routes.iter_mut().find(|route| {
            route.route_id == row.route_id
                && route.model == row.public_name
                && route.protocol == row.protocol
        }) {
            route.bindings.push(runtime_binding);
        } else {
            routes.push(RuntimeRoute {
                route_id: row.route_id.clone(),
                model: row.public_name.clone(),
                protocol: row.protocol,
                allow_lossy_conversion: row.allow_lossy_conversion,
                bindings: vec![runtime_binding],
            });
        }
        if let Some(model) = models.iter_mut().find(|model| model.id == row.public_name) {
            if !model.account_ids.contains(&row.account_id) {
                model.account_ids.push(row.account_id);
            }
        } else {
            models.push(PublishedModel {
                id: row.public_name,
                display_name: row.display_name,
                account_ids: vec![row.account_id],
            });
        }
    }
    if !errors.is_empty() {
        return Err(ControlPlaneError::Validation(errors));
    }
    let mut providers = providers.into_values().collect::<Vec<_>>();
    providers.sort_by(|left, right| left.id.cmp(&right.id));
    let mut accounts = accounts.into_values().collect::<Vec<_>>();
    accounts.sort_by(|left, right| left.id.cmp(&right.id));
    models.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(RuntimeSnapshot::new(
        GatewayConfig {
            listen_addr: listen_addr.to_owned(),
            providers,
            accounts,
            routes: Vec::new(),
        },
        routes,
        models,
        revision,
        generated_at,
    ))
}

fn capabilities_from_catalog(value: &Value) -> Result<Capabilities, String> {
    if value.is_null() {
        return Ok(Capabilities::default());
    }
    let object = value
        .as_object()
        .ok_or_else(|| "expected a JSON object".to_owned())?;
    fn mode(value: Option<&Value>) -> Result<CapabilityMode, String> {
        let Some(value) = value else {
            return Ok(CapabilityMode::Unsupported);
        };
        if let Some(boolean) = value.as_bool() {
            return Ok(if boolean {
                CapabilityMode::Native
            } else {
                CapabilityMode::Unsupported
            });
        }
        match value.as_str() {
            Some("supported" | "native") => Ok(CapabilityMode::Native),
            Some("translated") => Ok(CapabilityMode::Translated),
            Some("unsupported" | "unknown") => Ok(CapabilityMode::Unsupported),
            Some(other) => Err(format!("unknown capability value '{other}'")),
            None => Err("capability values must be booleans or strings".to_owned()),
        }
    }
    Ok(Capabilities {
        streaming: mode(object.get("streaming"))?,
        tools: mode(object.get("tools"))?,
        tool_streaming: mode(object.get("tool_streaming"))?,
        thinking: mode(object.get("thinking"))?,
        web_search: mode(object.get("web_search"))?,
        file_search: mode(object.get("file_search"))?,
        vision: mode(object.get("vision"))?,
        usage: mode(object.get("usage"))?,
    })
}

async fn import_gateway_config(
    tx: &mut Transaction<'_, Postgres>,
    config: &GatewayConfig,
) -> Result<(), ControlPlaneError> {
    for provider in &config.providers {
        let endpoints = serde_json::to_value(&provider.endpoints)?;
        let protocol_capabilities = serde_json::to_value(&provider.protocol_capabilities)?;
        let snapshot = json!({
            "base_url": provider.base_url,
            "endpoints": provider.endpoints,
            "protocol_capabilities": provider.protocol_capabilities,
            "native_protocols": provider.native_protocols,
            "capabilities": provider.capabilities,
        });
        sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled) VALUES ($1,$2,'custom',1,$3,$4,$5,'{}'::jsonb,$6,TRUE)")
            .bind(&provider.id)
            .bind(&provider.name)
            .bind(snapshot)
            .bind(&provider.base_url)
            .bind(endpoints)
            .bind(protocol_capabilities)
            .execute(&mut **tx)
            .await?;
    }
    for account in &config.accounts {
        sqlx::query("INSERT INTO accounts (id,provider_id,source_id,display_name,credential_env,credential_ciphertext,enabled,weight) VALUES ($1,NULL,$2,$3,$4,NULL,$5,$6)")
            .bind(&account.id)
            .bind(&account.provider_id)
            .bind(&account.display_name)
            .bind(&account.credential_env)
            .bind(account.enabled)
            .bind(account.weight as i32)
            .execute(&mut **tx)
            .await?;
    }
    for route in &config.routes {
        let models = expand_route_models(route, config)?;
        for (model_index, logical_model) in models.iter().enumerate() {
            let logical_model_id = logical_model_id(logical_model);
            sqlx::query("INSERT INTO logical_models (id,public_name,display_name,status,metadata,field_sources,enabled,confirmed_at) VALUES ($1,$2,$2,'confirmed','{}'::jsonb,'{}'::jsonb,TRUE,NOW()) ON CONFLICT (public_name) DO UPDATE SET display_name=EXCLUDED.display_name,status='confirmed',enabled=TRUE,confirmed_at=COALESCE(logical_models.confirmed_at,NOW()),updated_at=NOW()")
                .bind(&logical_model_id)
                .bind(logical_model)
                .execute(&mut **tx)
                .await?;
            let stored_logical_id: String =
                sqlx::query_scalar("SELECT id FROM logical_models WHERE public_name=$1")
                    .bind(logical_model)
                    .fetch_one(&mut **tx)
                    .await?;
            let route_id = if models.len() == 1 {
                route.id.clone()
            } else {
                format!("{}:{model_index}", route.id)
            };
            sqlx::query("INSERT INTO routes (id,logical_model_id,model_pattern,provider_id,protocols,primary_account_id,fallback_accounts,strategy,mode,adapter,allow_lossy_conversion,enabled) VALUES ($1,$2,$3,NULL,$4,NULL,'[]'::jsonb,$5,'binding',NULL,$6,TRUE)")
                .bind(route_id)
                .bind(&stored_logical_id)
                .bind(logical_model)
                .bind(serde_json::to_value(&route.protocols)?)
                .bind(&route.strategy)
                .bind(route.allow_lossy_conversion)
                .execute(&mut **tx)
                .await?;
            let mut account_ids = Vec::with_capacity(1 + route.fallback_accounts.len());
            account_ids.push(route.primary_account_id.as_str());
            account_ids.extend(route.fallback_accounts.iter().map(String::as_str));
            for (index, account_id) in account_ids.into_iter().enumerate() {
                let account = config.account(account_id).ok_or_else(|| {
                    ControlPlaneError::Validation(vec![format!(
                        "route '{}' references unknown account '{account_id}'",
                        route.id
                    )])
                })?;
                let upstream_model_id = account
                    .model_map
                    .get(logical_model)
                    .cloned()
                    .unwrap_or_else(|| logical_model.clone());
                ensure_imported_source_model(tx, &account.provider_id, &upstream_model_id).await?;
                for protocol in &route.protocols {
                    let capability = config.protocol_capability(
                        &account.provider_id,
                        Some(&account.id),
                        logical_model,
                        *protocol,
                    );
                    ensure_imported_capability(
                        tx,
                        config,
                        account,
                        logical_model,
                        &upstream_model_id,
                        *protocol,
                        &capability,
                    )
                    .await?;
                    let priority = if index == 0 {
                        10_000
                    } else {
                        1_000 - index as i32
                    };
                    sqlx::query("INSERT INTO model_bindings (logical_model_id,source_id,account_id,upstream_model_id,protocol,status,enabled,priority,confirmed_at) VALUES ($1,$2,$3,$4,$5,'confirmed',$6,$7,NOW()) ON CONFLICT (logical_model_id,source_id,account_id,upstream_model_id,protocol) DO UPDATE SET status='confirmed',enabled=EXCLUDED.enabled,priority=GREATEST(model_bindings.priority,EXCLUDED.priority),confirmed_at=COALESCE(model_bindings.confirmed_at,NOW()),updated_at=NOW()")
                        .bind(&stored_logical_id)
                        .bind(&account.provider_id)
                        .bind(&account.id)
                        .bind(&upstream_model_id)
                        .bind(*protocol)
                        .bind(account.enabled)
                        .bind(priority)
                        .execute(&mut **tx)
                        .await?;
                }
            }
        }
    }
    Ok(())
}

fn expand_route_models(
    route: &RouteConfig,
    config: &GatewayConfig,
) -> Result<Vec<String>, ControlPlaneError> {
    let provider = config.provider(&route.provider_id).ok_or_else(|| {
        ControlPlaneError::Validation(vec![format!(
            "route '{}' references unknown provider '{}'",
            route.id, route.provider_id
        )])
    })?;
    let mut models = if route.model == "*" {
        provider.models.clone()
    } else if let Some(prefix) = route.model.strip_suffix('*') {
        provider
            .models
            .iter()
            .filter(|model| model.starts_with(prefix))
            .cloned()
            .collect()
    } else {
        vec![route.model.clone()]
    };
    models.sort();
    models.dedup();
    if models.is_empty() {
        return Err(ControlPlaneError::Validation(vec![format!(
            "route '{}' pattern '{}' matched no provider models",
            route.id, route.model
        )]));
    }
    Ok(models)
}

fn logical_model_id(public_name: &str) -> String {
    format!("logical:{public_name}")
}

async fn ensure_imported_source_model(
    tx: &mut Transaction<'_, Postgres>,
    source_id: &str,
    upstream_model_id: &str,
) -> Result<(), ControlPlaneError> {
    sqlx::query("INSERT INTO source_models (source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,confirmed_at) VALUES ($1,$2,'confirmed','available',$3,'{}'::jsonb,'{}'::jsonb,NOW()) ON CONFLICT (source_id,upstream_model_id) DO UPDATE SET confirmation_status='confirmed',availability_status='available',raw_snapshot=EXCLUDED.raw_snapshot,confirmed_at=COALESCE(source_models.confirmed_at,NOW()),unavailable_at=NULL,updated_at=NOW()")
        .bind(source_id)
        .bind(upstream_model_id)
        .bind(json!({"origin":"GATEWAY_CONFIG_JSON"}))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn ensure_imported_capability(
    tx: &mut Transaction<'_, Postgres>,
    config: &GatewayConfig,
    account: &AccountConfig,
    logical_model: &str,
    upstream_model_id: &str,
    protocol: Protocol,
    capability: &ProtocolCapability,
) -> Result<(), ControlPlaneError> {
    let features = serde_json::to_value(config.capabilities(
        &account.provider_id,
        Some(&account.id),
        logical_model,
    ))?;
    let (mode, source_protocol, adapter) = match capability.mode {
        ProtocolMode::Native => (SourceProtocolMode::Native, None, None),
        ProtocolMode::Adapter => {
            let source_protocol = capability.source_protocol.ok_or_else(|| {
                ControlPlaneError::Validation(vec![format!(
                    "imported adapter capability {}/{protocol} is missing source_protocol",
                    account.provider_id
                )])
            })?;
            ensure_imported_native_capability(
                tx,
                &account.provider_id,
                upstream_model_id,
                source_protocol,
                &features,
            )
            .await?;
            (
                SourceProtocolMode::Adapter,
                Some(source_protocol),
                capability.adapter.clone(),
            )
        }
        ProtocolMode::Unsupported => {
            return Err(ControlPlaneError::Validation(vec![format!(
                "route import cannot bind unsupported protocol {protocol} on source '{}'",
                account.provider_id
            )]));
        }
    };
    sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,source_protocol,adapter,feature_capabilities,field_source,confirmed_at) VALUES ($1,$2,$3,'confirmed',$4,$5,$6,$7,'user',NOW()) ON CONFLICT (source_id,upstream_model_id,protocol) DO UPDATE SET status='confirmed',mode=EXCLUDED.mode,source_protocol=EXCLUDED.source_protocol,adapter=EXCLUDED.adapter,feature_capabilities=EXCLUDED.feature_capabilities,field_source='user',confirmed_at=COALESCE(source_model_capabilities.confirmed_at,NOW()),unavailable_at=NULL,updated_at=NOW()")
        .bind(&account.provider_id)
        .bind(upstream_model_id)
        .bind(protocol)
        .bind(mode)
        .bind(source_protocol)
        .bind(adapter)
        .bind(features)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn ensure_imported_native_capability(
    tx: &mut Transaction<'_, Postgres>,
    source_id: &str,
    upstream_model_id: &str,
    protocol: Protocol,
    features: &Value,
) -> Result<(), ControlPlaneError> {
    sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,feature_capabilities,field_source,confirmed_at) VALUES ($1,$2,$3,'confirmed','native',$4,'user',NOW()) ON CONFLICT (source_id,upstream_model_id,protocol) DO UPDATE SET status='confirmed',mode='native',source_protocol=NULL,adapter=NULL,feature_capabilities=EXCLUDED.feature_capabilities,field_source='user',confirmed_at=COALESCE(source_model_capabilities.confirmed_at,NOW()),unavailable_at=NULL,updated_at=NOW()")
        .bind(source_id)
        .bind(upstream_model_id)
        .bind(protocol)
        .bind(features)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
pub struct SourceWrite {
    pub id: String,
    pub display_name: String,
    #[serde(default = "default_custom_preset")]
    pub provider_preset_id: String,
    #[serde(default = "default_preset_version")]
    pub provider_preset_version: i32,
    pub base_url: String,
    #[serde(default)]
    pub endpoints: HashMap<Protocol, String>,
    #[serde(default)]
    pub auth_config: Value,
    #[serde(default)]
    pub protocol_capabilities: Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// POST contract shared with the Provider discovery workflow. Omitted values
/// are filled from the selected immutable ProviderPreset snapshot.
#[derive(Clone, Debug, Deserialize)]
pub struct SourceCreateWrite {
    pub id: String,
    pub display_name: String,
    pub provider_preset_id: String,
    #[serde(default)]
    pub provider_preset_version: Option<i32>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub endpoints: Option<HashMap<Protocol, String>>,
    #[serde(default)]
    pub endpoint_overrides: HashMap<Protocol, String>,
    #[serde(default)]
    pub auth_config: Option<Value>,
    #[serde(default)]
    pub protocol_capabilities: Option<Value>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct SourceView {
    pub id: String,
    pub display_name: String,
    pub provider_preset_id: String,
    pub provider_preset_version: i32,
    pub provider_preset_snapshot: Value,
    pub base_url: String,
    pub endpoints: Value,
    pub auth_config: Value,
    pub protocol_capabilities: Value,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountWrite {
    pub id: String,
    pub source_id: String,
    pub display_name: String,
    #[serde(default)]
    pub credential_env: Option<String>,
    #[serde(default)]
    pub credential_ciphertext: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_weight")]
    pub weight: i32,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct AccountView {
    pub id: String,
    pub source_id: String,
    pub display_name: String,
    pub credential_env: Option<String>,
    pub credential_configured: bool,
    pub enabled: bool,
    pub weight: i32,
    pub health_status: String,
    pub health_source: String,
    pub health_updated_at: Option<DateTime<Utc>>,
    pub consecutive_failures: i32,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_probe_at: Option<DateTime<Utc>>,
    pub last_probe_status: Option<String>,
    pub last_probe_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LogicalModelWrite {
    pub id: String,
    pub public_name: String,
    pub display_name: String,
    #[serde(default)]
    pub status: CatalogStatus,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub field_sources: Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct LogicalModelView {
    pub id: String,
    pub public_name: String,
    pub display_name: String,
    pub status: CatalogStatus,
    pub metadata: Value,
    pub field_sources: Value,
    pub enabled: bool,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub unavailable_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelBindingWrite {
    pub logical_model_id: String,
    pub source_id: String,
    pub account_id: String,
    pub upstream_model_id: String,
    pub protocol: Protocol,
    #[serde(default)]
    pub status: CatalogStatus,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct ModelBindingView {
    pub id: i64,
    pub logical_model_id: String,
    pub source_id: String,
    pub account_id: String,
    pub upstream_model_id: String,
    pub protocol: Protocol,
    pub status: CatalogStatus,
    pub enabled: bool,
    pub priority: i32,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub unavailable_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RouteWrite {
    pub id: String,
    pub logical_model_id: String,
    pub protocols: Vec<Protocol>,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default)]
    pub allow_lossy_conversion: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct RouteView {
    pub id: String,
    pub logical_model_id: String,
    pub public_name: String,
    pub protocols: Value,
    pub strategy: String,
    pub allow_lossy_conversion: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct EnabledWrite {
    pub enabled: bool,
}

pub struct Mutation<T> {
    pub record: T,
    pub snapshot: RuntimeSnapshot,
}

fn default_true() -> bool {
    true
}

fn default_weight() -> i32 {
    100
}

fn default_strategy() -> String {
    "primary_then_weighted_fallback".to_owned()
}

fn default_custom_preset() -> String {
    "custom".to_owned()
}

fn default_preset_version() -> i32 {
    1
}

impl ControlPlane {
    pub async fn list_sources(&self) -> Result<Vec<SourceView>, ControlPlaneError> {
        Ok(sqlx::query_as::<_, SourceView>(
            "SELECT id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled,created_at,updated_at FROM sources ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn get_source(&self, id: &str) -> Result<SourceView, ControlPlaneError> {
        fetch_source(&self.pool, id).await
    }

    pub async fn create_source_from_request(
        &self,
        input: &SourceCreateWrite,
    ) -> Result<Mutation<SourceView>, ControlPlaneError> {
        let mut errors = Vec::new();
        validate_nonempty(&input.id, "source.id", &mut errors);
        validate_nonempty(&input.display_name, "source.display_name", &mut errors);
        validate_nonempty(
            &input.provider_preset_id,
            "source.provider_preset_id",
            &mut errors,
        );
        if input
            .provider_preset_version
            .is_some_and(|version| version <= 0)
        {
            errors.push("source.provider_preset_version must be positive".to_owned());
        }
        for (protocol, endpoint) in &input.endpoint_overrides {
            if !endpoint.starts_with('/') || endpoint.starts_with("//") {
                errors.push(format!(
                    "source.endpoint_overrides.{protocol} must be an absolute path"
                ));
            }
        }
        if !errors.is_empty() {
            return Err(ControlPlaneError::Validation(errors));
        }
        let preset: Option<(i32, Value)> = match input.provider_preset_version {
            Some(version) => {
                sqlx::query_as(
                    "SELECT version,definition FROM provider_presets WHERE id=$1 AND version=$2",
                )
                .bind(&input.provider_preset_id)
                .bind(version)
                .fetch_optional(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as(
                    "SELECT version,definition FROM provider_presets WHERE id=$1 ORDER BY version DESC LIMIT 1",
                )
                .bind(&input.provider_preset_id)
                .fetch_optional(&self.pool)
                .await?
            }
        };
        let Some((preset_version, preset_snapshot)) = preset else {
            return Err(ControlPlaneError::NotFound(format!(
                "provider preset '{}'{} not found",
                input.provider_preset_id,
                input
                    .provider_preset_version
                    .map(|version| format!("@{version}"))
                    .unwrap_or_default()
            )));
        };
        let definition = match serde_json::from_value::<ProviderPresetDefinition>(preset_snapshot) {
            Ok(definition) => {
                definition.validate().map_err(|error| {
                    ControlPlaneError::Validation(vec![format!(
                        "provider preset '{}@{preset_version}' is invalid: {error}",
                        input.provider_preset_id
                    )])
                })?;
                Some(definition)
            }
            Err(_) if input.provider_preset_id == "custom" => None,
            Err(error) => {
                return Err(ControlPlaneError::Validation(vec![format!(
                    "provider preset '{}@{preset_version}' is invalid: {error}",
                    input.provider_preset_id
                )]))
            }
        };
        if definition.is_none() && (input.base_url.is_none() || input.endpoints.is_none()) {
            return Err(ControlPlaneError::Validation(vec![
                "custom Source creation requires base_url and endpoints".to_owned(),
            ]));
        }
        let mut endpoints = input.endpoints.clone().unwrap_or_else(|| {
            definition
                .as_ref()
                .expect("managed preset definition was validated")
                .protocols
                .iter()
                .map(|(protocol, preset)| (*protocol, preset.endpoint.clone()))
                .collect()
        });
        endpoints.extend(input.endpoint_overrides.clone());
        let effective = SourceWrite {
            id: input.id.clone(),
            display_name: input.display_name.clone(),
            provider_preset_id: input.provider_preset_id.clone(),
            provider_preset_version: preset_version,
            base_url: input.base_url.clone().unwrap_or_else(|| {
                definition
                    .as_ref()
                    .expect("managed preset definition was validated")
                    .default_base_url
                    .clone()
            }),
            endpoints,
            auth_config: input.auth_config.clone().unwrap_or_else(|| {
                definition
                    .as_ref()
                    .map_or_else(|| json!({}), ProviderPresetDefinition::auth_snapshot)
            }),
            protocol_capabilities: input.protocol_capabilities.clone().unwrap_or_else(|| {
                definition.as_ref().map_or_else(
                    || json!({}),
                    ProviderPresetDefinition::protocol_capabilities_snapshot,
                )
            }),
            enabled: input.enabled,
        };
        self.create_source(&effective).await
    }

    pub async fn create_source(
        &self,
        input: &SourceWrite,
    ) -> Result<Mutation<SourceView>, ControlPlaneError> {
        validate_source_input(input, &self.source_url_policy)?;
        let mut tx = self.begin_write().await?;
        if row_exists(&mut tx, "sources", "id", &input.id).await? {
            return Err(ControlPlaneError::Conflict(format!(
                "source '{}' already exists",
                input.id
            )));
        }
        let preset_snapshot: Value = sqlx::query_scalar(
            "SELECT definition FROM provider_presets WHERE id=$1 AND version=$2",
        )
        .bind(&input.provider_preset_id)
        .bind(input.provider_preset_version)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            ControlPlaneError::NotFound(format!(
                "provider preset '{}@{}' not found",
                input.provider_preset_id, input.provider_preset_version
            ))
        })?;
        let record = sqlx::query_as::<_, SourceView>(
            "INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled,created_at,updated_at",
        )
        .bind(&input.id)
        .bind(&input.display_name)
        .bind(&input.provider_preset_id)
        .bind(input.provider_preset_version)
        .bind(preset_snapshot)
        .bind(&input.base_url)
        .bind(serde_json::to_value(&input.endpoints)?)
        .bind(normalize_object(input.auth_config.clone(), "source auth_config")?)
        .bind(normalize_object(
            input.protocol_capabilities.clone(),
            "source protocol_capabilities",
        )?)
        .bind(input.enabled)
        .fetch_one(&mut *tx)
        .await?;
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn update_source(
        &self,
        id: &str,
        input: &SourceWrite,
    ) -> Result<Mutation<SourceView>, ControlPlaneError> {
        validate_resource_id(id, &input.id, "source")?;
        validate_source_input(input, &self.source_url_policy)?;
        let mut tx = self.begin_write().await?;
        let preset: Option<(String, i32, bool)> = sqlx::query_as(
            "SELECT provider_preset_id,provider_preset_version,enabled FROM sources WHERE id=$1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(preset) = preset else {
            return Err(ControlPlaneError::NotFound(format!(
                "source '{id}' not found"
            )));
        };
        if (preset.0.clone(), preset.1)
            != (
                input.provider_preset_id.clone(),
                input.provider_preset_version,
            )
        {
            return Err(ControlPlaneError::Validation(vec![
                "a Source provider preset snapshot is immutable after creation".to_owned(),
            ]));
        }
        let record = sqlx::query_as::<_, SourceView>(
            "UPDATE sources SET display_name=$2,base_url=$3,endpoints=$4,auth_config=$5,protocol_capabilities=$6,enabled=$7,updated_at=NOW() WHERE id=$1 RETURNING id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled,created_at,updated_at",
        )
        .bind(id)
        .bind(&input.display_name)
        .bind(&input.base_url)
        .bind(serde_json::to_value(&input.endpoints)?)
        .bind(normalize_object(input.auth_config.clone(), "source auth_config")?)
        .bind(normalize_object(
            input.protocol_capabilities.clone(),
            "source protocol_capabilities",
        )?)
        .bind(input.enabled)
        .fetch_one(&mut *tx)
        .await?;
        if preset.2 != input.enabled {
            apply_manual_health_transition_for_source(&mut tx, id, input.enabled, Utc::now())
                .await?;
        }
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn set_source_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<Mutation<SourceView>, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let previous_enabled: Option<bool> =
            sqlx::query_scalar("SELECT enabled FROM sources WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let record = sqlx::query_as::<_, SourceView>(
            "UPDATE sources SET enabled=$2,updated_at=NOW() WHERE id=$1 RETURNING id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled,created_at,updated_at",
        )
        .bind(id)
        .bind(enabled)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ControlPlaneError::NotFound(format!("source '{id}' not found")))?;
        if previous_enabled.is_some_and(|previous| previous != enabled) {
            apply_manual_health_transition_for_source(&mut tx, id, enabled, Utc::now()).await?;
        }
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn delete_source(&self, id: &str) -> Result<RuntimeSnapshot, ControlPlaneError> {
        let mut tx = self.begin_write().await?;
        let result = sqlx::query("DELETE FROM sources WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ControlPlaneError::NotFound(format!(
                "source '{id}' not found"
            )));
        }
        self.finish_write(tx).await
    }

    pub async fn list_accounts(&self) -> Result<Vec<AccountView>, ControlPlaneError> {
        Ok(sqlx::query_as::<_, AccountView>(account_view_select(None))
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn get_account(&self, id: &str) -> Result<AccountView, ControlPlaneError> {
        sqlx::query_as::<_, AccountView>(account_view_select(Some("WHERE id=$1")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ControlPlaneError::NotFound(format!("account '{id}' not found")))
    }

    pub async fn create_account(
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn update_account(
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn set_account_enabled(
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn delete_account(&self, id: &str) -> Result<RuntimeSnapshot, ControlPlaneError> {
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

    pub async fn list_logical_models(&self) -> Result<Vec<LogicalModelView>, ControlPlaneError> {
        Ok(
            sqlx::query_as::<_, LogicalModelView>(logical_model_select(None))
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn get_logical_model(&self, id: &str) -> Result<LogicalModelView, ControlPlaneError> {
        sqlx::query_as::<_, LogicalModelView>(logical_model_select(Some("WHERE id=$1")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ControlPlaneError::NotFound(format!("logical model '{id}' not found")))
    }

    pub async fn create_logical_model(
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
            "INSERT INTO logical_models (id,public_name,display_name,status,metadata,field_sources,enabled,confirmed_at,unavailable_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id,public_name,display_name,status,metadata,field_sources,enabled,confirmed_at,unavailable_at,created_at,updated_at",
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn update_logical_model(
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
            "UPDATE logical_models SET public_name=$2,display_name=$3,status=$4,metadata=$5,field_sources=$6,enabled=$7,confirmed_at=CASE WHEN $4='confirmed' THEN COALESCE(confirmed_at,NOW()) ELSE confirmed_at END,unavailable_at=CASE WHEN $4='unavailable' THEN NOW() ELSE NULL END,updated_at=NOW() WHERE id=$1 RETURNING id,public_name,display_name,status,metadata,field_sources,enabled,confirmed_at,unavailable_at,created_at,updated_at",
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn set_logical_model_enabled(
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
        let snapshot = self.finish_write(tx).await?;
        Ok(Mutation { record, snapshot })
    }

    pub async fn delete_logical_model(
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        extract::State,
        http::{Request, StatusCode},
    };
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::{str::FromStr, time::Duration};
    use tower::ServiceExt;

    #[test]
    fn source_validation_uses_server_policy_and_does_not_echo_blocked_targets() {
        let input = SourceWrite {
            id: "private-source".into(),
            display_name: "Private Source".into(),
            provider_preset_id: "custom".into(),
            provider_preset_version: 1,
            base_url: "http://10.20.30.40:8080".into(),
            endpoints: HashMap::new(),
            auth_config: json!({}),
            protocol_capabilities: json!({}),
            enabled: true,
        };
        let error = validate_source_input(&input, &SourceUrlPolicy::default()).unwrap_err();
        let message = error.message();
        assert!(message.contains("server Source URL policy"));
        assert!(!message.contains("10.20.30.40"));

        let allowlisted =
            SourceUrlPolicy::from_allowlist("10.20.0.0/16").expect("private test CIDR");
        assert!(validate_source_input(&input, &allowlisted).is_ok());
    }

    async fn isolated_database() -> (Database, PgPool, String) {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for the PostgreSQL control-plane test");
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect PostgreSQL test admin database");
        let schema = format!("control_plane_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create isolated test schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse TEST_DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect isolated test schema");
        let database = Database::from_test_pool(pool.clone())
            .await
            .expect("migrate isolated test schema");
        (database, admin, schema)
    }

    fn bootstrap_config(base_url: &str) -> GatewayConfig {
        serde_json::from_value(json!({
            "listen_addr": "127.0.0.1:0",
            "providers": [{
                "id": "source-a",
                "name": "Source A",
                "base_url": base_url,
                "models": ["logical-a"],
                "endpoints": {"openai_chat_completions": "/v1/chat/completions"},
                "protocol_capabilities": {
                    "openai_chat_completions": {"mode": "unsupported"}
                },
                "capabilities": {"streaming":"native","tools":"native","usage":"native"},
                "model_overrides": {
                    "logical-a": {
                        "protocol_capabilities": {
                            "openai_chat_completions": {"mode": "native"}
                        },
                        "capabilities": {
                            "streaming": "translated",
                            "tools": "translated",
                            "usage": "translated"
                        }
                    }
                }
            }],
            "accounts": [{
                "id": "account-a",
                "provider_id": "source-a",
                "display_name": "Account A",
                "credential_env": "SOURCE_A_API_KEY",
                "enabled": true,
                "weight": 100,
                "protocol_capabilities": {
                    "openai_chat_completions": {"mode": "unsupported"}
                },
                "capabilities": {
                    "streaming": "unsupported",
                    "tools": "unsupported",
                    "usage": "unsupported"
                }
            }],
            "routes": [{
                "id": "route-a",
                "model": "logical-a",
                "provider_id": "source-a",
                "protocols": ["openai_chat_completions"],
                "primary_account_id": "account-a",
                "mode": "native"
            }]
        }))
        .expect("bootstrap config")
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL and runs against an isolated PostgreSQL schema"]
    async fn postgres_db_first_crud_rollback_snapshot_and_models_contract() {
        let (database, admin, schema) = isolated_database().await;
        let control_plane = ControlPlane::new(&database, "127.0.0.1:0");

        let first = control_plane
            .initialize_from_config(&bootstrap_config("https://source-a.example"), false)
            .await
            .expect("initial DB import")
            .expect("empty control plane imports once");
        assert_eq!(first.models.len(), 1);
        let resolved = first
            .resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-a")
            .expect("imported binding is routable");
        assert_eq!(resolved.upstream_model_id, "logical-a");
        assert_eq!(resolved.protocol_upstream, Protocol::OpenAiChatCompletions);
        let (imported_mode, imported_features): (String, Value) = sqlx::query_as(
            "SELECT mode::text,feature_capabilities FROM source_model_capabilities WHERE source_id='source-a' AND upstream_model_id='logical-a' AND protocol='openai_chat_completions'",
        )
        .fetch_one(database.pool())
        .await
        .expect("load imported model capability precedence fixture");
        assert_eq!(imported_mode, "native");
        assert_eq!(
            imported_features,
            json!({
                "streaming": "translated",
                "tools": "translated",
                "tool_streaming": "unsupported",
                "thinking": "unsupported",
                "web_search": "unsupported",
                "file_search": "unsupported",
                "vision": "unsupported",
                "usage": "translated"
            })
        );

        assert!(control_plane
            .initialize_from_config(
                &bootstrap_config("https://must-not-overwrite.example"),
                false
            )
            .await
            .expect("repeat startup import check")
            .is_none());
        assert_eq!(
            control_plane
                .get_source("source-a")
                .await
                .expect("load source after repeat startup")
                .base_url,
            "https://source-a.example"
        );

        let invalid_adapter_source = SourceWrite {
            id: "invalid-adapter-source".into(),
            display_name: "Invalid Adapter".into(),
            provider_preset_id: "custom".into(),
            provider_preset_version: 1,
            base_url: "https://invalid.example".into(),
            endpoints: HashMap::from([(Protocol::AnthropicMessages, "/v1/messages".into())]),
            auth_config: json!({}),
            protocol_capabilities: serde_json::to_value(HashMap::from([(
                Protocol::OpenAiResponses,
                ProtocolCapability::adapter(Protocol::AnthropicMessages, "missing_adapter"),
            )]))
            .unwrap(),
            enabled: true,
        };
        assert!(matches!(
            control_plane.create_source(&invalid_adapter_source).await,
            Err(ControlPlaneError::Validation(_))
        ));
        assert!(matches!(
            control_plane.get_source("invalid-adapter-source").await,
            Err(ControlPlaneError::NotFound(_))
        ));

        let source_input = SourceWrite {
            id: "source-b".into(),
            display_name: "Source B".into(),
            provider_preset_id: "custom".into(),
            provider_preset_version: 1,
            base_url: "https://source-b.example".into(),
            endpoints: HashMap::from([
                (
                    Protocol::OpenAiChatCompletions,
                    "/v1/chat/completions".into(),
                ),
                (Protocol::AnthropicMessages, "/v1/messages".into()),
            ]),
            auth_config: json!({}),
            protocol_capabilities: json!({}),
            enabled: true,
        };
        control_plane
            .create_source(&source_input)
            .await
            .expect("create source");
        let mut updated_source = source_input.clone();
        updated_source.display_name = "Source B Updated".into();
        control_plane
            .update_source("source-b", &updated_source)
            .await
            .expect("update source");
        assert_eq!(control_plane.list_sources().await.unwrap().len(), 2);

        let account_input = AccountWrite {
            id: "account-b".into(),
            source_id: "source-b".into(),
            display_name: "Account B".into(),
            credential_env: Some("SOURCE_B_API_KEY".into()),
            credential_ciphertext: None,
            enabled: true,
            weight: 50,
        };
        let account = control_plane
            .create_account(&account_input)
            .await
            .expect("create account")
            .record;
        let serialized_account = serde_json::to_value(&account).unwrap();
        assert!(serialized_account.get("credential_ciphertext").is_none());
        assert_eq!(serialized_account["credential_configured"], true);
        let mut updated_account = account_input.clone();
        updated_account.display_name = "Account B Updated".into();
        updated_account.weight = 75;
        assert_eq!(
            control_plane
                .update_account("account-b", &updated_account)
                .await
                .expect("update account")
                .record
                .weight,
            75
        );
        assert_eq!(control_plane.list_accounts().await.unwrap().len(), 2);

        let logical_input = LogicalModelWrite {
            id: "logical-b-id".into(),
            public_name: "logical-b".into(),
            display_name: "Logical B".into(),
            status: CatalogStatus::Confirmed,
            metadata: json!({}),
            field_sources: json!({}),
            enabled: true,
        };
        control_plane
            .create_logical_model(&logical_input)
            .await
            .expect("create logical model");
        let mut updated_logical = logical_input.clone();
        updated_logical.display_name = "Logical B Updated".into();
        control_plane
            .update_logical_model("logical-b-id", &updated_logical)
            .await
            .expect("update logical model");
        assert_eq!(
            control_plane
                .get_logical_model("logical-b-id")
                .await
                .unwrap()
                .display_name,
            "Logical B Updated"
        );
        let pool = database.pool().clone();
        sqlx::query("INSERT INTO source_models (source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,confirmed_at) VALUES ('source-b','upstream-b','confirmed','available','{}'::jsonb,'{}'::jsonb,'{}'::jsonb,NOW())")
            .execute(&pool)
            .await
            .expect("insert confirmed source model fixture");
        sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,feature_capabilities,field_source,confirmed_at) VALUES ('source-b','upstream-b','openai_chat_completions','confirmed','native','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW())")
            .execute(&pool)
            .await
            .expect("insert confirmed capability fixture");
        sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,feature_capabilities,field_source,confirmed_at) VALUES ('source-b','upstream-b','anthropic_messages','confirmed','native','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW()),('source-b','upstream-b','openai_responses','confirmed','adapter','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW())")
            .execute(&pool)
            .await
            .expect_err("adapter fixture without source_protocol/adapter must be rejected");
        sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,feature_capabilities,field_source,confirmed_at) VALUES ('source-b','upstream-b','anthropic_messages','confirmed','native','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW())")
            .execute(&pool)
            .await
            .expect("insert adapter source protocol fixture");
        sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,source_protocol,adapter,feature_capabilities,field_source,confirmed_at) VALUES ('source-b','upstream-b','openai_responses','confirmed','adapter','anthropic_messages','kimi_responses_adapter','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW())")
            .execute(&pool)
            .await
            .expect("insert confirmed adapter capability fixture");
        let binding_input = ModelBindingWrite {
            logical_model_id: "logical-b-id".into(),
            source_id: "source-b".into(),
            account_id: "account-b".into(),
            upstream_model_id: "upstream-b".into(),
            protocol: Protocol::OpenAiChatCompletions,
            status: CatalogStatus::Confirmed,
            enabled: true,
            priority: 100,
        };
        let binding = control_plane
            .create_model_binding(&binding_input)
            .await
            .expect("create confirmed binding")
            .record;
        let adapter_binding = control_plane
            .create_model_binding(&ModelBindingWrite {
                logical_model_id: "logical-b-id".into(),
                source_id: "source-b".into(),
                account_id: "account-b".into(),
                upstream_model_id: "upstream-b".into(),
                protocol: Protocol::OpenAiResponses,
                status: CatalogStatus::Confirmed,
                enabled: true,
                priority: 100,
            })
            .await
            .expect("create confirmed adapter binding")
            .record;
        let mut updated_binding = binding_input.clone();
        updated_binding.priority = 200;
        assert_eq!(
            control_plane
                .update_model_binding(binding.id, &updated_binding)
                .await
                .expect("update binding")
                .record
                .priority,
            200
        );
        let route_input = RouteWrite {
            id: "route-b".into(),
            logical_model_id: "logical-b-id".into(),
            protocols: vec![Protocol::OpenAiChatCompletions, Protocol::OpenAiResponses],
            strategy: "primary_then_weighted_fallback".into(),
            allow_lossy_conversion: false,
            enabled: true,
        };
        control_plane
            .create_route(&route_input)
            .await
            .expect("create route");
        let mut updated_route = route_input.clone();
        updated_route.strategy = "priority_then_fallback".into();
        let route = control_plane
            .update_route("route-b", &updated_route)
            .await
            .expect("update route");
        assert_eq!(control_plane.list_model_bindings().await.unwrap().len(), 3);
        assert_eq!(control_plane.list_routes().await.unwrap().len(), 2);
        let stable_snapshot = route.snapshot;
        let resolved = stable_snapshot
            .resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b")
            .expect("DB binding route resolves");
        assert_eq!(resolved.source_id, "source-b");
        assert_eq!(resolved.provider_id, "custom");
        assert_eq!(resolved.upstream_model_id, "upstream-b");
        assert_eq!(resolved.binding_id, Some(binding.id));
        let adapter_route = stable_snapshot
            .resolver
            .resolve_detailed(Protocol::OpenAiResponses, "logical-b")
            .expect("adapter binding route resolves");
        assert_eq!(adapter_route.source_id, "source-b");
        assert_eq!(adapter_route.provider_id, "custom");
        assert_eq!(adapter_route.protocol_upstream, Protocol::AnthropicMessages);
        assert_eq!(
            adapter_route.adapter.as_deref(),
            Some("kimi_responses_adapter")
        );

        let mut invalid_source = updated_source.clone();
        invalid_source.endpoints.clear();
        assert!(matches!(
            control_plane
                .update_source("source-b", &invalid_source)
                .await,
            Err(ControlPlaneError::Validation(_))
        ));
        let persisted_endpoints = control_plane
            .get_source("source-b")
            .await
            .expect("failed source update rolled back")
            .endpoints;
        assert_eq!(
            persisted_endpoints["openai_chat_completions"],
            "/v1/chat/completions"
        );
        let binding_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM model_bindings WHERE logical_model_id='logical-b-id'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(control_plane
            .create_model_binding(&ModelBindingWrite {
                logical_model_id: "logical-b-id".into(),
                source_id: "source-b".into(),
                account_id: "account-b".into(),
                upstream_model_id: "missing-model".into(),
                protocol: Protocol::OpenAiChatCompletions,
                status: CatalogStatus::Confirmed,
                enabled: true,
                priority: 1,
            })
            .await
            .is_err());
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM model_bindings WHERE logical_model_id='logical-b-id'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            binding_count
        );

        let disabled_route = control_plane
            .set_route_enabled("route-b", false)
            .await
            .expect("disable route");
        assert!(disabled_route
            .snapshot
            .resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b")
            .is_err());
        assert!(stable_snapshot
            .resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b")
            .is_ok());
        let live_resolver = Arc::new(std::sync::RwLock::new(stable_snapshot.resolver.clone()));
        let mut readers = Vec::new();
        for _ in 0..8 {
            let live_resolver = live_resolver.clone();
            readers.push(tokio::spawn(async move {
                for _ in 0..100 {
                    let result = live_resolver
                        .read()
                        .unwrap()
                        .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b");
                    if let Err(error) = result {
                        assert_eq!(error.code, "route_not_found");
                    }
                    tokio::task::yield_now().await;
                }
            }));
        }
        *live_resolver.write().unwrap() = disabled_route.snapshot.resolver.clone();
        for reader in readers {
            reader.await.expect("snapshot reader task");
        }
        control_plane
            .set_route_enabled("route-b", true)
            .await
            .expect("reenable route");

        for snapshot in [
            control_plane
                .set_model_binding_enabled(binding.id, false)
                .await
                .expect("disable binding")
                .snapshot,
            control_plane
                .set_account_enabled("account-b", false)
                .await
                .expect("disable account")
                .snapshot,
            control_plane
                .set_source_enabled("source-b", false)
                .await
                .expect("disable source")
                .snapshot,
            control_plane
                .set_logical_model_enabled("logical-b-id", false)
                .await
                .expect("disable logical model")
                .snapshot,
        ] {
            assert!(snapshot
                .resolver
                .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b")
                .is_err());
        }
        control_plane
            .set_logical_model_enabled("logical-b-id", true)
            .await
            .expect("reenable logical model");
        control_plane
            .set_source_enabled("source-b", true)
            .await
            .expect("reenable source");
        control_plane
            .set_account_enabled("account-b", true)
            .await
            .expect("reenable account");
        let active_snapshot = control_plane
            .set_model_binding_enabled(binding.id, true)
            .await
            .expect("reenable binding")
            .snapshot;

        sqlx::query("UPDATE sources SET endpoints='{}'::jsonb WHERE id='source-b'")
            .execute(&pool)
            .await
            .expect("corrupt candidate endpoint for failed reload test");
        assert!(control_plane.load_snapshot().await.is_err());
        assert!(active_snapshot
            .resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b")
            .is_ok());
        sqlx::query("UPDATE sources SET endpoints='{\"openai_chat_completions\":\"/v1/chat/completions\",\"anthropic_messages\":\"/v1/messages\"}'::jsonb WHERE id='source-b'")
            .execute(&pool)
            .await
            .expect("restore valid endpoint");
        sqlx::query("UPDATE sources SET base_url='http://127.0.0.1:8787' WHERE id='source-b'")
            .execute(&pool)
            .await
            .expect("corrupt Source URL for failed reload test");
        let blocked = match control_plane.load_snapshot().await {
            Ok(_) => panic!("unsafe persisted Source URL must block snapshot publication"),
            Err(error) => error,
        };
        assert!(blocked.message().contains("server Source URL policy"));
        assert!(!blocked.message().contains("127.0.0.1"));
        sqlx::query("UPDATE sources SET base_url='https://source-b.example' WHERE id='source-b'")
            .execute(&pool)
            .await
            .expect("restore valid Source URL");
        let active_snapshot = control_plane
            .load_snapshot()
            .await
            .expect("reload restored snapshot");
        assert!(active_snapshot.revision > stable_snapshot.revision);

        let health = crate::infra::health::HealthRegistry::new(Duration::from_secs(60));
        let active_revision = active_snapshot.revision;
        let state = crate::state::AppState {
            live: Arc::new(std::sync::RwLock::new(
                crate::state::LiveConfig::from_snapshot(active_snapshot),
            )),
            http: crate::proxy::transport::test_client().expect("HTTP client"),
            db: Some(database.clone()),
            control_plane: Some(control_plane.clone()),
            health: health.clone(),
            admin_auth: crate::state::AdminAuth::test(),
            secrets: crate::infra::secrets::SecretResolver::empty(),
            prometheus_handle: crate::infra::observability::prometheus_handle(),
        };
        state.reload_snapshot(stable_snapshot.clone());
        assert_eq!(state.snapshot().revision, active_revision);

        let capability_response = crate::admin_capabilities_response(true, &state.snapshot());
        assert_eq!(capability_response.status(), StatusCode::OK);
        let capability_body = to_bytes(capability_response.into_body(), 1024 * 1024)
            .await
            .expect("read DB-backed capability matrix response");
        let capability_text = String::from_utf8_lossy(&capability_body);
        assert!(!capability_text.contains("SOURCE_B_API_KEY"));
        assert!(!capability_text.contains("credential_env"));
        let capability_body: Value =
            serde_json::from_slice(&capability_body).expect("parse capability matrix JSON");
        assert_eq!(capability_body["version"], "v1");
        assert_eq!(capability_body["fact_source"], "runtime_snapshot");
        assert_eq!(capability_body["snapshot_revision"], active_revision);
        let capability_row = capability_body["data"]
            .as_array()
            .expect("capability matrix data")
            .iter()
            .find(|row| {
                row["route_id"] == "route-b"
                    && row["model"] == "logical-b"
                    && row["source"]["source_id"] == "source-b"
                    && row["account"]["account_id"] == "account-b"
            })
            .expect("DB runtime route capability row");
        let protocol_cell = |protocol: &str| {
            capability_row["protocols"]
                .as_array()
                .expect("three protocol cells")
                .iter()
                .find(|cell| cell["protocol_in"] == protocol)
                .unwrap_or_else(|| panic!("missing capability cell for {protocol}"))
        };
        let chat_cell = protocol_cell("openai_chat_completions");
        assert_eq!(chat_cell["status"], "routable");
        assert_eq!(chat_cell["mode"], "native");
        assert_eq!(chat_cell["binding_id"], binding.id);
        let responses_cell = protocol_cell("openai_responses");
        assert_eq!(responses_cell["status"], "routable");
        assert_eq!(responses_cell["mode"], "adapter");
        assert_eq!(responses_cell["binding_id"], adapter_binding.id);
        assert_eq!(responses_cell["adapter"], "kimi_responses_adapter");
        assert_eq!(responses_cell["protocol_upstream"], "anthropic_messages");
        assert_eq!(responses_cell["degraded"], true);
        assert_eq!(
            responses_cell["conversion_chain"][0]["protocol_from"],
            "openai_responses"
        );
        assert_eq!(
            responses_cell["conversion_chain"][0]["protocol_to"],
            "anthropic_messages"
        );
        let messages_cell = protocol_cell("anthropic_messages");
        assert_eq!(messages_cell["status"], "unroutable");
        assert!(messages_cell["mode"].is_null());
        assert_eq!(messages_cell["error"]["code"], "route_not_found");

        let model_response = crate::models(State(state.clone())).await.0;
        assert!(model_response["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "logical-b"));
        health.mark_failure("account-b").await;
        let model_response = crate::models(State(state.clone())).await.0;
        assert!(!model_response["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "logical-b"));

        let app = crate::application(state);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/accounts")
                    .header(
                        "authorization",
                        format!("Bearer {}", crate::state::TEST_ADMIN_KEY),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("list accounts API response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(!body.contains("credential_ciphertext"));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/sources")
                    .header("content-type", "application/json")
                    .header(
                        "authorization",
                        format!("Bearer {}", crate::state::TEST_ADMIN_KEY),
                    )
                    .body(Body::from(
                        json!({
                            "id":"source-api",
                            "display_name":"Source API",
                            "provider_preset_id":"custom",
                            "base_url":"https://source-api.example",
                            "endpoints":{"openai_chat_completions":"/v1/chat/completions"}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .expect("create Source through merged control-plane API");
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body["snapshot_revision"].as_i64().unwrap() > active_revision);
        control_plane
            .delete_source("source-api")
            .await
            .expect("delete Source created through API");

        control_plane
            .delete_route("route-b")
            .await
            .expect("delete route");
        control_plane
            .delete_model_binding(binding.id)
            .await
            .expect("delete binding");
        control_plane
            .delete_model_binding(adapter_binding.id)
            .await
            .expect("delete adapter binding");
        control_plane
            .delete_logical_model("logical-b-id")
            .await
            .expect("delete logical model");
        control_plane
            .delete_account("account-b")
            .await
            .expect("delete account");
        control_plane
            .delete_source("source-b")
            .await
            .expect("delete source");
        assert!(matches!(
            control_plane.get_source("source-b").await,
            Err(ControlPlaneError::NotFound(_))
        ));

        drop(control_plane);
        drop(database);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop isolated test schema");
        admin.close().await;
    }
}
