use super::*;
use sha2::{Digest, Sha256};
use uuid::Uuid;

impl Database {
    /// Compatibility helper that creates a hash-only, unrecoverable key. The
    /// Admin API uses encrypted recovery material instead.
    #[allow(dead_code)]
    pub async fn create_virtual_key(
        &self,
        name: &str,
        allowed_models: &[String],
    ) -> Result<(i64, String), sqlx::Error> {
        self.create_virtual_key_with_options(
            name,
            allowed_models,
            &[VIRTUAL_KEY_INVOKE_SCOPE.to_owned()],
            None,
            None,
            "created",
        )
        .await
    }

    #[allow(dead_code)]
    pub async fn create_virtual_key_with_options(
        &self,
        name: &str,
        allowed_models: &[String],
        scopes: &[String],
        expires_at: Option<DateTime<Utc>>,
        key_group: Option<&str>,
        origin: &str,
    ) -> Result<(i64, String), sqlx::Error> {
        let material = Self::generate_virtual_key_material();
        self.create_virtual_key_with_material(
            name,
            allowed_models,
            scopes,
            expires_at,
            key_group,
            origin,
            &material,
            None,
        )
        .await
    }

    pub fn generate_virtual_key_material() -> VirtualKeyMaterial {
        let raw = format!("gw_{}", uuid::Uuid::new_v4().simple());
        let prefix = raw.chars().take(11).collect::<String>();
        let hash = hash_key(&raw);
        VirtualKeyMaterial { raw, prefix, hash }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_virtual_key_with_material(
        &self,
        name: &str,
        allowed_models: &[String],
        scopes: &[String],
        expires_at: Option<DateTime<Utc>>,
        key_group: Option<&str>,
        origin: &str,
        material: &VirtualKeyMaterial,
        key_ciphertext: Option<&str>,
    ) -> Result<(i64, String), sqlx::Error> {
        let scopes = scopes_json(scopes);
        let allowed_models =
            serde_json::to_value(allowed_models).unwrap_or_else(|_| Value::Array(vec![]));
        let row = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO virtual_keys
             (name,key_prefix,key_hash,key_ciphertext,allowed_models,scopes,key_group,expires_at,origin)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id",
        )
        .bind(name)
        .bind(&material.prefix)
        .bind(&material.hash)
        .bind(key_ciphertext)
        .bind(allowed_models)
        .bind(scopes)
        .bind(key_group)
        .bind(expires_at)
        .bind(origin)
        .fetch_one(&self.pool)
        .await?;
        Ok((row.0, material.raw.clone()))
    }

    pub async fn list_virtual_keys(&self) -> Result<Vec<VirtualKeyRecord>, sqlx::Error> {
        let query = virtual_key_select("FROM virtual_keys ORDER BY id DESC");
        sqlx::query_as::<_, VirtualKeyRecord>(&query)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn get_virtual_key(&self, id: i64) -> Result<Option<VirtualKeyRecord>, sqlx::Error> {
        let query = virtual_key_select("FROM virtual_keys WHERE id=$1");
        sqlx::query_as::<_, VirtualKeyRecord>(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn get_virtual_key_ciphertext(
        &self,
        id: i64,
    ) -> Result<Option<(String, Option<String>)>, sqlx::Error> {
        sqlx::query_as("SELECT key_prefix,key_ciphertext FROM virtual_keys WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn revoke_virtual_key(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE virtual_keys
             SET enabled=FALSE, revoked_at=COALESCE(revoked_at,NOW()), updated_at=NOW()
             WHERE id=$1 AND enabled=TRUE AND revoked_at IS NULL",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Update mutable metadata and permissions in one atomic statement. The
    /// `CASE` flags preserve the distinction between an omitted field and an
    /// explicit null used to clear expiry/group metadata.
    #[allow(dead_code)]
    pub async fn update_virtual_key(
        &self,
        id: i64,
        update: &VirtualKeyUpdate,
    ) -> Result<VirtualKeyRecord, VirtualKeyError> {
        let name_set = update.name.is_some();
        let models_set = update.allowed_models.is_some();
        let scopes_set = update.scopes.is_some();
        let expiry_set = update.expires_at.is_some();
        let group_set = update.key_group.is_some();
        let models = update
            .allowed_models
            .as_ref()
            .map(|values| serde_json::to_value(values).unwrap_or_else(|_| Value::Array(vec![])));
        let scopes = update.scopes.as_ref().map(|values| scopes_json(values));
        let expiry = update.expires_at.as_ref().and_then(|value| *value);
        let group = update.key_group.as_ref().and_then(|value| value.as_deref());
        let query = format!(
            "UPDATE virtual_keys
             SET name=CASE WHEN $2 THEN $3 ELSE name END,
                 allowed_models=CASE WHEN $4 THEN $5 ELSE allowed_models END,
                 scopes=CASE WHEN $6 THEN $7 ELSE scopes END,
                 expires_at=CASE WHEN $8 THEN $9 ELSE expires_at END,
                 key_group=CASE WHEN $10 THEN $11 ELSE key_group END,
                 updated_at=NOW()
             WHERE id=$1 AND revoked_at IS NULL
             RETURNING {}",
            VIRTUAL_KEY_COLUMNS
        );
        let record = sqlx::query_as::<_, VirtualKeyRecord>(&query)
            .bind(id)
            .bind(name_set)
            .bind(update.name.as_deref())
            .bind(models_set)
            .bind(models)
            .bind(scopes_set)
            .bind(scopes)
            .bind(expiry_set)
            .bind(expiry)
            .bind(group_set)
            .bind(group)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(VirtualKeyError::NotFound)?;
        Ok(record)
    }

    /// Rotate one key generation under a row lock. Concurrent rotations of the
    /// same generation are serialized; the second caller receives Conflict.
    #[allow(dead_code)]
    pub async fn rotate_virtual_key(
        &self,
        id: i64,
        options: &VirtualKeyRotationOptions,
    ) -> Result<VirtualKeyRotation, VirtualKeyError> {
        let material = Self::generate_virtual_key_material();
        self.rotate_virtual_key_with_material(id, options, &material, None)
            .await
    }

    pub async fn rotate_virtual_key_with_material(
        &self,
        id: i64,
        options: &VirtualKeyRotationOptions,
        material: &VirtualKeyMaterial,
        key_ciphertext: Option<&str>,
    ) -> Result<VirtualKeyRotation, VirtualKeyError> {
        if options.overlap > Duration::from_secs(86_400) {
            return Err(VirtualKeyError::Validation(
                "overlap window must be at most 86400 seconds".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        let query = virtual_key_select("FROM virtual_keys WHERE id=$1 FOR UPDATE");
        let old = sqlx::query_as::<_, VirtualKeyRecord>(&query)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(VirtualKeyError::NotFound)?;
        if !old.enabled || old.revoked_at.is_some() {
            return Err(VirtualKeyError::Conflict(
                "virtual key is disabled or revoked".into(),
            ));
        }
        if old.expires_at.is_some_and(|expires| expires <= Utc::now()) {
            return Err(VirtualKeyError::Conflict("virtual key is expired".into()));
        }
        if old.replaced_by_id.is_some() {
            return Err(VirtualKeyError::Conflict(
                "virtual key has already been rotated".into(),
            ));
        }

        let overlap_until = if options.overlap.is_zero() {
            None
        } else {
            Some(
                Utc::now()
                    + chrono::Duration::from_std(options.overlap).map_err(|_| {
                        VirtualKeyError::Validation("invalid overlap window".into())
                    })?,
            )
        };
        let models_value = options
            .allowed_models
            .as_ref()
            .map(|values| serde_json::to_value(values).unwrap_or_else(|_| Value::Array(vec![])))
            .unwrap_or_else(|| old.allowed_models.clone());
        let scopes_value = options
            .scopes
            .as_ref()
            .map(|values| scopes_json(values))
            .unwrap_or_else(|| old.scopes.clone());
        let expires_at = options.expires_at.or(old.expires_at);
        let name = options.name.as_deref().unwrap_or(&old.name);
        let key_group = options
            .key_group
            .as_ref()
            .and_then(|value| value.as_deref())
            .or(old.key_group.as_deref());
        let new_id = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO virtual_keys
             (name,key_prefix,key_hash,key_ciphertext,allowed_models,scopes,key_group,expires_at,origin)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'rotated') RETURNING id",
        )
        .bind(name)
        .bind(&material.prefix)
        .bind(&material.hash)
        .bind(key_ciphertext)
        .bind(models_value)
        .bind(scopes_value)
        .bind(key_group)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?
        .0;
        sqlx::query(
            "UPDATE virtual_keys
             SET replaced_by_id=$2, overlap_until=$3,
                 enabled=CASE WHEN $3 IS NULL THEN FALSE ELSE enabled END,
                 revoked_at=CASE WHEN $3 IS NULL THEN COALESCE(revoked_at,NOW()) ELSE revoked_at END,
                 updated_at=NOW()
             WHERE id=$1",
        )
        .bind(id)
        .bind(new_id)
        .bind(overlap_until)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(VirtualKeyRotation {
            old_id: id,
            new_id,
            key_prefix: material.prefix.clone(),
            key: material.raw.clone(),
            overlap_until,
        })
    }

    /// Import the configured legacy static key as a normal database-backed
    /// credential. This operation is idempotent and never returns the raw key.
    #[allow(dead_code)]
    pub async fn migrate_static_virtual_key(
        &self,
        raw: &str,
        name: &str,
        allowed_models: &[String],
        scopes: &[String],
        expires_at: Option<DateTime<Utc>>,
        key_group: Option<&str>,
    ) -> Result<StaticVirtualKeyMigration, VirtualKeyError> {
        let hash = hash_key(raw);
        let prefix = raw.chars().take(11).collect::<String>();
        let mut tx = self.pool.begin().await?;
        if let Some(row) = sqlx::query_as::<_, (i64, bool, Option<DateTime<Utc>>, Option<DateTime<Utc>>)>(
            "SELECT id,enabled,expires_at,revoked_at FROM virtual_keys WHERE key_hash=$1 FOR UPDATE",
        )
        .bind(&hash)
        .fetch_optional(&mut *tx)
        .await?
        {
            tx.commit().await?;
            let active = row.1
                && row.3.is_none()
                && row.2.is_none_or(|expires| expires > Utc::now());
            return Ok(StaticVirtualKeyMigration {
                id: row.0,
                key_prefix: prefix,
                created: false,
                active,
            });
        }
        let row = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO virtual_keys
             (name,key_prefix,key_hash,allowed_models,scopes,key_group,expires_at,origin)
             VALUES ($1,$2,$3,$4,$5,$6,$7,'static_migration') RETURNING id",
        )
        .bind(name)
        .bind(&prefix)
        .bind(&hash)
        .bind(serde_json::to_value(allowed_models).unwrap_or_else(|_| Value::Array(vec![])))
        .bind(scopes_json(scopes))
        .bind(key_group)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(StaticVirtualKeyMigration {
            id: row.0,
            key_prefix: prefix,
            created: true,
            active: true,
        })
    }

    pub async fn authenticate_virtual_key(
        &self,
        raw: &str,
        model: Option<&str>,
    ) -> Result<Option<i64>, sqlx::Error> {
        self.authenticate_virtual_key_with_scope(raw, model, VIRTUAL_KEY_INVOKE_SCOPE)
            .await
    }

    pub async fn authenticate_virtual_key_with_scope(
        &self,
        raw: &str,
        model: Option<&str>,
        required_scope: &str,
    ) -> Result<Option<i64>, sqlx::Error> {
        let hash = hash_key(raw);
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query_as::<_, (i64, Value, Value)>(
            "SELECT id,allowed_models,scopes FROM virtual_keys
             WHERE key_hash=$1 AND enabled=TRUE AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > NOW())
               AND (replaced_by_id IS NULL OR (overlap_until IS NOT NULL AND overlap_until > NOW()))
             FOR UPDATE",
        )
        .bind(&hash)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((id, allowed, scopes)) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let permitted_model = allowed_models_allow(&allowed, model);
        let permitted_scope = scope_allow(&scopes, required_scope);
        if !permitted_model || !permitted_scope {
            tx.commit().await?;
            return Ok(None);
        }
        let updated = sqlx::query(
            "UPDATE virtual_keys SET last_used_at=NOW() WHERE id=$1
             AND enabled=TRUE AND revoked_at IS NULL
             AND (expires_at IS NULL OR expires_at > NOW())
             AND (replaced_by_id IS NULL OR (overlap_until IS NOT NULL AND overlap_until > NOW()))",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((updated.rows_affected() == 1).then_some(id))
    }
}

fn hash_key(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

const VIRTUAL_KEY_COLUMNS: &str =
    "id,name,key_prefix,(key_ciphertext IS NOT NULL) AS key_recoverable,allowed_models,scopes,key_group,enabled,created_at,updated_at,last_used_at,expires_at,revoked_at,replaced_by_id,overlap_until,origin";

fn virtual_key_select(suffix: &str) -> String {
    format!("SELECT {VIRTUAL_KEY_COLUMNS} {suffix}")
}

fn scopes_json(scopes: &[String]) -> Value {
    if scopes.is_empty() {
        Value::Array(vec![Value::String(VIRTUAL_KEY_INVOKE_SCOPE.to_owned())])
    } else {
        serde_json::to_value(scopes).unwrap_or_else(|_| Value::Array(vec![]))
    }
}

fn allowed_models_allow(value: &Value, model: Option<&str>) -> bool {
    let allowed = value.as_array().cloned().unwrap_or_default();
    // Catalogue endpoints (`/v1/models`) pass `None` to skip the model
    // whitelist entirely; the virtual key only needs to satisfy the
    // required scope (typically `gateway:invoke`).
    allowed.is_empty()
        || model.is_none()
        || allowed
            .iter()
            .any(|item| item.as_str() == model || item.as_str() == Some("*"))
}

fn scope_allow(value: &Value, required: &str) -> bool {
    let scopes = value.as_array().cloned().unwrap_or_default();
    // `gateway:invoke` is the baseline permission and also permits the model
    // catalogue endpoint. More granular scopes can be added without changing
    // the authentication query or treating unknown values as grants.
    scopes.iter().any(|item| {
        item.as_str() == Some("*")
            || item.as_str() == Some(required)
            || (required == VIRTUAL_KEY_MODELS_SCOPE
                && item.as_str() == Some(VIRTUAL_KEY_INVOKE_SCOPE))
    })
}

pub fn validate_virtual_key_scopes(scopes: &[String]) -> Result<(), String> {
    let allowed = [VIRTUAL_KEY_INVOKE_SCOPE, VIRTUAL_KEY_MODELS_SCOPE, "*"];
    for scope in scopes {
        if !allowed.contains(&scope.as_str()) {
            return Err(format!("unsupported virtual key scope '{scope}'"));
        }
    }
    Ok(())
}
