#[cfg(test)]
use super::*;

#[cfg(test)]
impl Database {
    #[cfg(test)]
    pub async fn sync_control_plane(&self, config: &GatewayConfig) -> Result<(), sqlx::Error> {
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

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn upsert_provider(&self, provider: &ProviderConfig) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        self.upsert_provider_tx(&mut tx, provider).await?;
        tx.commit().await
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn upsert_account(&self, account: &AccountConfig) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        self.upsert_account_tx(&mut tx, account).await?;
        tx.commit().await
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn upsert_route(&self, route: &RouteConfig) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        self.upsert_route_tx(&mut tx, route).await?;
        tx.commit().await
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn delete_provider(&self, id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM providers WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn delete_account(&self, id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM accounts WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn delete_route(&self, id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM routes WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    #[cfg(test)]
    #[allow(clippy::type_complexity, dead_code)]
    pub async fn load_gateway_config(
        &self,
        listen_addr: &str,
    ) -> Result<GatewayConfig, sqlx::Error> {
        let provider_rows: Vec<(String, String, String, bool, Value, Value, Value, Value, Value)> =
            sqlx::query_as(
                "SELECT id,name,base_url,enabled,capabilities,endpoints,models,native_protocols,protocol_capabilities \
                 FROM providers ORDER BY id"
            )
            .fetch_all(&self.pool)
            .await?;
        let mut providers = Vec::with_capacity(provider_rows.len());
        for (
            id,
            name,
            base_url,
            _enabled,
            capabilities_val,
            endpoints_val,
            models_val,
            native_val,
            proto_cap_val,
        ) in provider_rows
        {
            let model_overrides_val: Value = sqlx::query_scalar(
                "SELECT COALESCE(model_overrides, '{}'::jsonb) FROM providers WHERE id=$1",
            )
            .bind(&id)
            .fetch_one(&self.pool)
            .await?;
            providers.push(ProviderConfig {
                id,
                name,
                base_url,
                models: serde_json::from_value(models_val).unwrap_or_default(),
                native_protocols: serde_json::from_value(native_val).unwrap_or_default(),
                endpoints: serde_json::from_value(endpoints_val).unwrap_or_default(),
                capabilities: serde_json::from_value(capabilities_val).unwrap_or_default(),
                protocol_capabilities: serde_json::from_value(proto_cap_val).unwrap_or_default(),
                model_overrides: serde_json::from_value(model_overrides_val).unwrap_or_default(),
            });
        }

        let account_rows: Vec<(String, String, String, bool, i32, Option<String>, Option<String>, Value, Value, Value, Value)> =
            sqlx::query_as(
                "SELECT id,provider_id,display_name,enabled,weight,credential_env,credential_ciphertext,\
                 protocol_capabilities,capabilities,model_overrides,model_map \
                 FROM accounts ORDER BY id"
            )
            .fetch_all(&self.pool)
            .await?;
        let mut accounts = Vec::with_capacity(account_rows.len());
        for (
            id,
            provider_id,
            display_name,
            enabled,
            weight,
            credential_env,
            credential_ciphertext,
            proto_cap,
            cap_val,
            model_ov,
            model_map_val,
        ) in account_rows
        {
            accounts.push(AccountConfig {
                id,
                provider_id,
                display_name,
                credential_env,
                credential_ciphertext,
                credential: None,
                enabled,
                weight: weight as u32,
                protocol_capabilities: serde_json::from_value(proto_cap).unwrap_or_default(),
                capabilities: serde_json::from_value(cap_val).ok().flatten(),
                model_overrides: serde_json::from_value(model_ov).unwrap_or_default(),
                model_map: serde_json::from_value(model_map_val).unwrap_or_default(),
            });
        }

        let route_rows: Vec<(
            String,
            String,
            String,
            Value,
            String,
            Value,
            String,
            String,
            Option<String>,
            bool,
        )> = sqlx::query_as(
            "SELECT id,model_pattern,provider_id,protocols,primary_account_id,fallback_accounts,\
                 strategy,mode,adapter,allow_lossy_conversion \
                 FROM routes WHERE enabled=TRUE ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut routes = Vec::with_capacity(route_rows.len());
        for (
            id,
            model,
            provider_id,
            protocols_val,
            primary_account_id,
            fallback_val,
            strategy,
            mode,
            adapter,
            allow_lossy,
        ) in route_rows
        {
            routes.push(RouteConfig {
                id,
                model,
                provider_id,
                protocols: serde_json::from_value(protocols_val).unwrap_or_default(),
                primary_account_id,
                fallback_accounts: serde_json::from_value(fallback_val).unwrap_or_default(),
                strategy,
                mode,
                adapter,
                allow_lossy_conversion: allow_lossy,
            });
        }

        Ok(GatewayConfig {
            listen_addr: listen_addr.to_string(),
            providers,
            accounts,
            routes,
        })
    }
}
