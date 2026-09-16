use super::error::ControlPlaneError;
use crate::domain::catalog::SourceProtocolMode;
use crate::domain::config::{
    AccountConfig, GatewayConfig, ProtocolCapability, ProtocolMode, ProviderConfig, RouteConfig,
};
use crate::domain::protocol::Protocol;
use crate::domain::provider_preset::ProviderPresetDefinition;
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};

pub(super) async fn resolve_import_provider_preset(
    tx: &mut Transaction<'_, Postgres>,
    provider: &ProviderConfig,
) -> Result<(String, i32, Value), ControlPlaneError> {
    if let Some((version, definition)) = sqlx::query_as::<_, (i32, Value)>(
        "SELECT version, definition FROM provider_presets WHERE id=$1 AND id <> 'custom' ORDER BY version DESC LIMIT 1",
    )
    .bind(&provider.id)
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok((provider.id.clone(), version, definition));
    }
    let snapshot = json!({
        "base_url": provider.base_url,
        "endpoints": provider.endpoints,
        "protocol_capabilities": provider.protocol_capabilities,
        "native_protocols": provider.native_protocols,
        "capabilities": provider.capabilities,
    });
    Ok(("custom".to_owned(), 1, snapshot))
}

pub(super) async fn import_gateway_config(
    tx: &mut Transaction<'_, Postgres>,
    config: &GatewayConfig,
) -> Result<(), ControlPlaneError> {
    for provider in &config.providers {
        let endpoints = serde_json::to_value(&provider.endpoints)?;
        let protocol_capabilities = serde_json::to_value(&provider.protocol_capabilities)?;
        let (preset_id, preset_version, preset_snapshot) =
            resolve_import_provider_preset(tx, provider).await?;
        let auth_config = if preset_id == "custom" {
            json!({})
        } else {
            serde_json::from_value::<ProviderPresetDefinition>(preset_snapshot.clone())?
                .auth_snapshot()
        };
        sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities,enabled) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,TRUE)")
            .bind(&provider.id)
            .bind(&provider.name)
            .bind(preset_id)
            .bind(preset_version)
            .bind(preset_snapshot)
            .bind(&provider.base_url)
            .bind(endpoints)
            .bind(auth_config)
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

pub(super) fn expand_route_models(
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

pub(super) fn logical_model_id(public_name: &str) -> String {
    format!("logical:{public_name}")
}

pub(super) async fn ensure_imported_source_model(
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

pub(super) async fn ensure_imported_capability(
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

pub(super) async fn ensure_imported_native_capability(
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
