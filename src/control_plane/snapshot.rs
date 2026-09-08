use super::error::ControlPlaneError;
use crate::domain::catalog::{PublishedModel, SourceProtocolMode};
use crate::domain::config::{
    adapter_definition, AccountConfig, Capabilities, CapabilityMode, GatewayConfig, ProviderConfig,
};
use crate::domain::protocol::Protocol;
use crate::domain::routing::{
    intersect_capabilities, join_endpoint, RouteResolver, RuntimeBinding, RuntimeRoute,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct RuntimeSnapshot {
    pub(crate) config: Arc<GatewayConfig>,
    pub(crate) resolver: RouteResolver,
    pub(crate) models: Arc<Vec<PublishedModel>>,
    pub(crate) revision: i64,
    pub(crate) generated_at: DateTime<Utc>,
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

pub(super) async fn build_snapshot(
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
