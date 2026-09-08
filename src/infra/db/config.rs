#[cfg(test)]
use super::*;

#[cfg(test)]
impl Database {
    #[cfg(test)]
    pub(crate) async fn sync_control_plane(
        &self,
        config: &GatewayConfig,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        for provider in &config.providers {
            self.upsert_provider_tx(&mut tx, provider).await?;
        }
        for account in &config.accounts {
            self.upsert_account_tx(&mut tx, account).await?;
        }
        for route in &config.routes {
            self.upsert_route_tx(&mut tx, route).await?;
        }
        tx.commit().await
    }

    #[cfg(test)]
    async fn upsert_provider_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        provider: &ProviderConfig,
    ) -> Result<(), sqlx::Error> {
        let endpoints =
            serde_json::to_value(&provider.endpoints).unwrap_or(Value::Object(Default::default()));
        let feature_capabilities = serde_json::to_value(&provider.capabilities)
            .unwrap_or(Value::Object(Default::default()));
        let protocol_capabilities = serde_json::to_value(&provider.protocol_capabilities)
            .unwrap_or(Value::Object(Default::default()));
        let models =
            serde_json::to_value(&provider.models).unwrap_or(Value::Array(Default::default()));
        let native_protocols = serde_json::to_value(&provider.native_protocols)
            .unwrap_or(Value::Array(Default::default()));
        let model_overrides = serde_json::to_value(&provider.model_overrides)
            .unwrap_or(Value::Object(Default::default()));
        sqlx::query(
            "INSERT INTO providers (id,name,base_url,capabilities,endpoints,models,native_protocols,protocol_capabilities,model_overrides) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) \
             ON CONFLICT (id) DO UPDATE SET name=EXCLUDED.name,base_url=EXCLUDED.base_url,\
             capabilities=EXCLUDED.capabilities,endpoints=EXCLUDED.endpoints,\
             models=EXCLUDED.models,native_protocols=EXCLUDED.native_protocols,\
             protocol_capabilities=EXCLUDED.protocol_capabilities,\
             model_overrides=EXCLUDED.model_overrides,updated_at=NOW()"
        )
        .bind(&provider.id)
        .bind(&provider.name)
        .bind(&provider.base_url)
        .bind(&feature_capabilities)
        .bind(&endpoints)
        .bind(&models)
        .bind(&native_protocols)
        .bind(&protocol_capabilities)
        .bind(&model_overrides)
        .execute(&mut **tx)
        .await?;
        let snapshot = serde_json::json!({
            "base_url": provider.base_url,
            "endpoints": endpoints,
            "feature_capabilities": feature_capabilities,
            "protocol_capabilities": protocol_capabilities,
            "native_protocols": provider.native_protocols,
        });
        sqlx::query(
            "INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,protocol_capabilities) \
             VALUES ($1,$2,'custom',1,$3,$4,$5,$6) ON CONFLICT (id) DO NOTHING"
        )
        .bind(&provider.id)
        .bind(&provider.name)
        .bind(snapshot)
        .bind(&provider.base_url)
        .bind(&endpoints)
        .bind(&protocol_capabilities)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    async fn upsert_account_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account: &AccountConfig,
    ) -> Result<(), sqlx::Error> {
        let protocol_capabilities = serde_json::to_value(&account.protocol_capabilities)
            .unwrap_or(Value::Object(Default::default()));
        let capabilities_val = serde_json::to_value(&account.capabilities).ok();
        let model_overrides = serde_json::to_value(&account.model_overrides)
            .unwrap_or(Value::Object(Default::default()));
        let model_map =
            serde_json::to_value(&account.model_map).unwrap_or(Value::Object(Default::default()));
        sqlx::query(
            "INSERT INTO accounts (id,provider_id,source_id,display_name,enabled,weight,protocol_capabilities,capabilities,model_overrides,model_map,credential_env) \
             VALUES ($1,$2,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT (id) DO UPDATE SET provider_id=EXCLUDED.provider_id,\
             display_name=EXCLUDED.display_name,enabled=EXCLUDED.enabled,weight=EXCLUDED.weight,\
             protocol_capabilities=EXCLUDED.protocol_capabilities,\
             capabilities=EXCLUDED.capabilities,model_overrides=EXCLUDED.model_overrides,\
             model_map=EXCLUDED.model_map,credential_env=EXCLUDED.credential_env,updated_at=NOW()"
        )
        .bind(&account.id)
        .bind(&account.provider_id)
        .bind(&account.display_name)
        .bind(account.enabled)
        .bind(account.weight as i32)
        .bind(&protocol_capabilities)
        .bind(&capabilities_val)
        .bind(&model_overrides)
        .bind(&model_map)
        .bind(&account.credential_env)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    async fn upsert_route_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        route: &RouteConfig,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO routes (id,model_pattern,provider_id,protocols,primary_account_id,fallback_accounts,strategy,mode,adapter,allow_lossy_conversion) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT (id) DO UPDATE SET model_pattern=EXCLUDED.model_pattern,\
             provider_id=EXCLUDED.provider_id,protocols=EXCLUDED.protocols,\
             primary_account_id=EXCLUDED.primary_account_id,\
             fallback_accounts=EXCLUDED.fallback_accounts,strategy=EXCLUDED.strategy,\
             mode=EXCLUDED.mode,adapter=EXCLUDED.adapter,\
             allow_lossy_conversion=EXCLUDED.allow_lossy_conversion,updated_at=NOW()"
        )
        .bind(&route.id)
        .bind(&route.model)
        .bind(&route.provider_id)
        .bind(serde_json::to_value(&route.protocols).unwrap_or(Value::Array(vec![])))
        .bind(&route.primary_account_id)
        .bind(serde_json::to_value(&route.fallback_accounts).unwrap_or(Value::Array(vec![])))
        .bind(&route.strategy)
        .bind(&route.mode)
        .bind(&route.adapter)
        .bind(route.allow_lossy_conversion)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}
