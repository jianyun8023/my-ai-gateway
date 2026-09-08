use super::error::ControlPlaneError;
use super::repository::{apply_manual_health_transition_for_source, fetch_source, row_exists};
use super::service::ControlPlane;
use super::snapshot::RuntimeSnapshot;
use super::types::{Mutation, SourceCreateWrite, SourceView, SourceWrite};
use super::validation::{
    normalize_object, validate_custom_preset_not_shadowing_builtin, validate_nonempty,
    validate_resource_id, validate_source_input,
};
use crate::domain::provider_preset::ProviderPresetDefinition;
use chrono::Utc;
use serde_json::{json, Value};

impl ControlPlane {
    pub(crate) async fn list_sources(&self) -> Result<Vec<SourceView>, ControlPlaneError> {
        Ok(sqlx::query_as::<_, SourceView>(
            "SELECT id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled,created_at,updated_at FROM sources ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub(crate) async fn get_source(&self, id: &str) -> Result<SourceView, ControlPlaneError> {
        fetch_source(&self.pool, id).await
    }

    pub(crate) async fn create_source_from_request(
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
        validate_custom_preset_not_shadowing_builtin(
            &self.pool,
            &input.id,
            &input.provider_preset_id,
        )
        .await?;
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

    pub(crate) async fn create_source(
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
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn update_source(
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
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn set_source_enabled(
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
        self.finish_mutation(tx, record).await
    }

    pub(crate) async fn delete_source(
        &self,
        id: &str,
    ) -> Result<RuntimeSnapshot, ControlPlaneError> {
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
}
