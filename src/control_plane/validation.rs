use super::error::ControlPlaneError;
use super::repository::row_exists;
use super::types::{AccountWrite, LogicalModelWrite, ModelBindingWrite, RouteWrite, SourceWrite};
use crate::domain::catalog::{CatalogAvailability, CatalogStatus, SourceProtocolMode};
use crate::domain::config::{
    adapter_definition, Capabilities, GatewayConfig, ProtocolCapabilityMatrix, ProviderConfig,
};
use crate::domain::protocol::Protocol;
use crate::source_url::{SourceUrlPolicy, SourceUrlPolicyError};
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashMap;

pub(super) fn validate_resource_id(
    path: &str,
    body: &str,
    kind: &str,
) -> Result<(), ControlPlaneError> {
    if path != body {
        return Err(ControlPlaneError::Validation(vec![format!(
            "{kind} id in path and body must match"
        )]));
    }
    Ok(())
}

pub(super) fn validate_nonempty(value: &str, field: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{field} must not be empty"));
    }
}

pub(super) fn normalize_object(value: Value, field: &str) -> Result<Value, ControlPlaneError> {
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

pub(super) async fn validate_custom_preset_not_shadowing_builtin(
    pool: &PgPool,
    source_id: &str,
    provider_preset_id: &str,
) -> Result<(), ControlPlaneError> {
    if provider_preset_id != "custom" {
        return Ok(());
    }
    let has_builtin: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM provider_presets WHERE id=$1 AND id <> 'custom')",
    )
    .bind(source_id)
    .fetch_one(pool)
    .await?;
    if has_builtin {
        return Err(ControlPlaneError::Validation(vec![format!(
            "source '{source_id}' matches a built-in provider preset; use provider_preset_id '{source_id}' instead of 'custom'"
        )]));
    }
    Ok(())
}

pub(super) fn validate_source_input(
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

pub(super) fn source_url_validation_message(field: &str, error: SourceUrlPolicyError) -> String {
    if error == SourceUrlPolicyError::InvalidUrl {
        format!("{field} must be an http(s) URL without credentials, query, or fragment")
    } else {
        format!("{field} is blocked by the server Source URL policy")
    }
}

pub(super) async fn validate_persisted_source_urls(
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

pub(super) fn validate_account_input(input: &AccountWrite) -> Result<(), ControlPlaneError> {
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

pub(super) fn validate_logical_model_input(
    input: &LogicalModelWrite,
) -> Result<(), ControlPlaneError> {
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

pub(super) fn validate_binding_input_shape(
    input: &ModelBindingWrite,
) -> Result<(), ControlPlaneError> {
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

pub(super) fn validate_route_input_shape(input: &RouteWrite) -> Result<(), ControlPlaneError> {
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

pub(super) fn validate_status_transition(
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

pub(super) async fn validate_binding_reference(
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
        validate_binding_family_constraint(tx, input).await?;
    }
    Ok(())
}

/// Reject cross-family bindings: when a LogicalModel already has confirmed
/// enabled bindings from one provider family (identified by
/// `sources.provider_preset_id`), a new binding from a different family is
/// forbidden.  This prevents silent cross-provider fallback that would
/// pollute usage attribution and cache statistics.
pub(super) async fn validate_binding_family_constraint(
    tx: &mut Transaction<'_, Postgres>,
    input: &ModelBindingWrite,
) -> Result<(), ControlPlaneError> {
    let existing_family: Option<String> = sqlx::query_scalar(
        "SELECT DISTINCT s.provider_preset_id \
         FROM model_bindings b \
         JOIN sources s ON s.id = b.source_id \
         WHERE b.logical_model_id = $1 \
           AND b.status = 'confirmed' \
           AND b.enabled \
           AND b.source_id <> $2 \
         LIMIT 1",
    )
    .bind(&input.logical_model_id)
    .bind(&input.source_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(existing) = existing_family {
        let new_family: Option<String> =
            sqlx::query_scalar("SELECT provider_preset_id FROM sources WHERE id = $1")
                .bind(&input.source_id)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some(new) = new_family {
            if new != existing {
                return Err(ControlPlaneError::Validation(vec![format!(
                    "cross-family binding rejected: logical model '{}' already has enabled \
                     bindings from provider family '{}'; cannot add binding from family '{}'",
                    input.logical_model_id, existing, new
                )]));
            }
        }
    }
    Ok(())
}

pub(super) async fn validate_route_reference(
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

pub(super) async fn validate_capability_chains(
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
