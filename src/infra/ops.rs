//! Operational data retention, control-plane export/import, and recovery
//! bookkeeping for Issue #53.
//!
//! The gateway deliberately keeps this code separate from request accounting
//! and routing.  Retention jobs operate on immutable UTC cut-offs and commit
//! every batch independently, which makes a cancelled or interrupted job safe
//! to retry with the same operation id.

use super::db::Database;
use crate::control_plane::{ControlPlane, ControlPlaneError, RuntimeSnapshot};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};
use uuid::Uuid;

pub(crate) const CURRENT_SCHEMA_VERSION: i32 = 23;
pub(crate) const CURRENT_MIGRATION_VERSION: i32 = 23;
pub(crate) const DEFAULT_BATCH_SIZE: i32 = 500;
pub(crate) const DEFAULT_MAX_BATCHES: i32 = 1_000;
pub(crate) const MAX_BATCH_SIZE: i32 = 10_000;
pub(crate) const MAX_MAX_BATCHES: i32 = 100_000;
pub(crate) const MAX_EXPORT_ROWS: usize = 100_000;

const RETENTION_KEYS: [&str; 4] = ["usage_events", "usage_attempts", "audit", "discovery"];

#[derive(Debug)]
pub(crate) enum OpsError {
    Database(sqlx::Error),
    Json(serde_json::Error),
    Validation(String),
    NotFound(String),
    Conflict(String),
    Snapshot(String),
}

impl fmt::Display for OpsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::Validation(message)
            | Self::NotFound(message)
            | Self::Conflict(message)
            | Self::Snapshot(message) => f.write_str(message),
        }
    }
}

impl Error for OpsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for OpsError {
    fn from(value: sqlx::Error) -> Self {
        if let sqlx::Error::Database(error) = &value {
            match error.code().as_deref() {
                Some("23505") => return Self::Conflict("operation already exists".into()),
                Some("23503" | "23514" | "22P02") => {
                    return Self::Validation("operation data violates a database constraint".into())
                }
                _ => {}
            }
        }
        Self::Database(value)
    }
}

impl From<serde_json::Error> for OpsError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<ControlPlaneError> for OpsError {
    fn from(value: ControlPlaneError) -> Self {
        match value {
            ControlPlaneError::NotFound(message) => Self::NotFound(message),
            ControlPlaneError::Conflict(message) => Self::Conflict(message),
            ControlPlaneError::Validation(errors) => Self::Validation(errors.join("; ")),
            ControlPlaneError::Json(error) => Self::Json(error),
            ControlPlaneError::Database(error) => Self::Database(error),
            ControlPlaneError::Credential(error) => Self::Validation(error.public_message().into()),
            ControlPlaneError::NoCiphertext => {
                Self::Validation("account does not have an encrypted credential".into())
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct OpsRepository {
    pool: PgPool,
}

impl OpsRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn from_database(database: &Database) -> Self {
        Self::new(database.pool().clone())
    }

    pub(crate) async fn schema_metadata(&self) -> Result<SchemaMetadata, OpsError> {
        let metadata = sqlx::query_as::<_, SchemaMetadata>(
            "SELECT schema_version,migration_version,application_version,updated_at FROM gateway_schema_metadata WHERE singleton=TRUE",
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| OpsError::Validation("schema metadata is not initialized".into()))?;
        if metadata.schema_version > CURRENT_SCHEMA_VERSION
            || metadata.migration_version > CURRENT_MIGRATION_VERSION
        {
            return Err(OpsError::Validation(
                "database schema/migration is newer than this gateway".into(),
            ));
        }
        Ok(metadata)
    }

    pub(crate) async fn list_retention_policies(&self) -> Result<Vec<RetentionPolicy>, OpsError> {
        Ok(sqlx::query_as::<_, RetentionPolicy>(
            "SELECT policy_key,retention_days,enabled,updated_at FROM retention_policies ORDER BY policy_key",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub(crate) async fn update_retention_policies(
        &self,
        policies: &[RetentionPolicyWrite],
        actor: &str,
    ) -> Result<Vec<RetentionPolicy>, OpsError> {
        if policies.is_empty() {
            return Err(OpsError::Validation(
                "at least one retention policy is required".into(),
            ));
        }
        let actor = sanitize_actor(actor);
        let mut tx = self.pool.begin().await?;
        let mut seen = BTreeSet::new();
        for policy in policies {
            validate_policy_key(&policy.policy_key)?;
            if !seen.insert(policy.policy_key.clone()) {
                return Err(OpsError::Validation(format!(
                    "duplicate retention policy '{}'",
                    policy.policy_key
                )));
            }
            validate_retention_days(policy.retention_days)?;
            sqlx::query(
                "UPDATE retention_policies SET retention_days=$2,enabled=$3,updated_at=clock_timestamp() WHERE policy_key=$1",
            )
            .bind(&policy.policy_key)
            .bind(policy.retention_days)
            .bind(policy.enabled)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            .eq(&1)
            .then_some(())
            .ok_or_else(|| OpsError::NotFound(format!(
                "retention policy '{}' not found",
                policy.policy_key
            )))?;
            append_audit_tx(
                &mut tx,
                &format!("retention-policy:{}", policy.policy_key),
                "retention.policy.updated",
                "succeeded",
                &actor,
                json!({
                    "policy_key": policy.policy_key,
                    "retention_days": policy.retention_days,
                    "enabled": policy.enabled,
                }),
                None,
                None,
            )
            .await?;
        }
        tx.commit().await?;
        self.list_retention_policies().await
    }

    pub(crate) async fn start_cleanup(
        &self,
        request: &CleanupRequest,
    ) -> Result<CleanupRun, OpsError> {
        validate_cleanup_request(request)?;
        let operation_id = request
            .operation_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let actor = sanitize_actor(request.requested_by.as_deref().unwrap_or("admin_api"));
        let policies = self.load_policy_map(request.policy_keys.as_deref()).await?;
        let policy_snapshot = policy_snapshot(&policies);
        let cutoff_snapshot = cutoff_snapshot(&policies);

        if let Some(existing) = self.get_cleanup(&operation_id).await? {
            if existing.dry_run != request.dry_run
                || existing.batch_size != request.batch_size
                || existing.max_batches != request.max_batches
            {
                return Err(OpsError::Conflict(
                    "operation_id is already associated with different cleanup options".into(),
                ));
            }
            if existing.status == "completed" || existing.status == "cancelled" {
                return Ok(existing);
            }
            if existing.status == "failed" {
                self.prepare_cleanup_retry(&operation_id, &actor).await?;
            }
            return self.run_cleanup(&operation_id).await;
        }

        let inserted = sqlx::query(
            "INSERT INTO retention_cleanup_runs (id,status,dry_run,batch_size,max_batches,requested_by,policy_snapshot,cutoff_snapshot,progress) VALUES ($1,'running',$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (id) DO NOTHING",
        )
        .bind(&operation_id)
        .bind(request.dry_run)
        .bind(request.batch_size)
        .bind(request.max_batches)
        .bind(&actor)
        .bind(&policy_snapshot)
        .bind(&cutoff_snapshot)
        .bind(json!({"phase":"started","has_more":true}))
        .execute(&self.pool)
        .await?;
        if inserted.rows_affected() == 0 {
            let existing = self.get_cleanup(&operation_id).await?.ok_or_else(|| {
                OpsError::Conflict("cleanup operation was created concurrently".into())
            })?;
            if existing.dry_run != request.dry_run
                || existing.batch_size != request.batch_size
                || existing.max_batches != request.max_batches
            {
                return Err(OpsError::Conflict(
                    "operation_id is already associated with different cleanup options".into(),
                ));
            }
            if existing.status == "completed" || existing.status == "cancelled" {
                return Ok(existing);
            }
            return self.run_cleanup(&operation_id).await;
        }
        append_audit(
            &self.pool,
            &operation_id,
            "retention.cleanup.started",
            "started",
            &actor,
            json!({
                "dry_run": request.dry_run,
                "batch_size": request.batch_size,
                "max_batches": request.max_batches,
                "policy_snapshot": policy_snapshot,
                "cutoff_snapshot": cutoff_snapshot,
            }),
            None,
            None,
        )
        .await?;
        tracing::info!(
            operation_id = %operation_id,
            dry_run = request.dry_run,
            batch_size = request.batch_size,
            "retention cleanup started"
        );
        self.run_cleanup(&operation_id).await
    }

    pub(crate) async fn get_cleanup(&self, id: &str) -> Result<Option<CleanupRun>, OpsError> {
        Ok(
            sqlx::query_as::<_, CleanupRun>(cleanup_select(Some("WHERE id=$1")))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub(crate) async fn list_cleanups(&self, limit: i64) -> Result<Vec<CleanupRun>, OpsError> {
        let limit = limit.clamp(1, 500);
        Ok(sqlx::query_as::<_, CleanupRun>(cleanup_select(None))
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
    }

    pub(crate) async fn get_backup_run(&self, id: &str) -> Result<Option<BackupRun>, OpsError> {
        Ok(sqlx::query_as::<_, BackupRun>(
            "SELECT id,operation,status,requested_by,schema_version,migration_version,format,checksum,metadata,error_code,error_message,started_at,completed_at FROM backup_runs WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub(crate) async fn list_backup_runs(&self, limit: i64) -> Result<Vec<BackupRun>, OpsError> {
        let limit = limit.clamp(1, 500);
        Ok(sqlx::query_as::<_, BackupRun>(
            "SELECT id,operation,status,requested_by,schema_version,migration_version,format,checksum,metadata,error_code,error_message,started_at,completed_at FROM backup_runs ORDER BY started_at DESC,id DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    pub(crate) async fn list_audit_logs(
        &self,
        operation_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AuditLog>, OpsError> {
        let limit = limit.clamp(1, 500);
        if let Some(operation_id) = operation_id {
            Ok(sqlx::query_as::<_, AuditLog>(
                "SELECT id,operation_id,action,status,actor,details,error_code,error_message,created_at,completed_at FROM audit_logs WHERE operation_id=$1 ORDER BY created_at DESC,id DESC LIMIT $2",
            )
            .bind(operation_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
        } else {
            Ok(sqlx::query_as::<_, AuditLog>(
                "SELECT id,operation_id,action,status,actor,details,error_code,error_message,created_at,completed_at FROM audit_logs ORDER BY created_at DESC,id DESC LIMIT $1",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
        }
    }

    pub(crate) async fn cancel_cleanup(
        &self,
        id: &str,
        actor: &str,
    ) -> Result<CleanupRun, OpsError> {
        let actor = sanitize_actor(actor);
        let mut tx = self.pool.begin().await?;
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM retention_cleanup_runs WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(status) = status else {
            return Err(OpsError::NotFound(format!(
                "cleanup operation '{id}' not found"
            )));
        };
        if matches!(status.as_str(), "completed" | "cancelled" | "failed") {
            tx.commit().await?;
            return self
                .get_cleanup(id)
                .await?
                .ok_or_else(|| OpsError::NotFound(format!("cleanup operation '{id}' not found")));
        }
        sqlx::query(
            "UPDATE retention_cleanup_runs SET cancel_requested=TRUE,status='cancel_requested',updated_at=clock_timestamp() WHERE id=$1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        append_audit_tx(
            &mut tx,
            id,
            "retention.cleanup.cancel_requested",
            "cancel_requested",
            &actor,
            json!({}),
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        self.get_cleanup(id)
            .await?
            .ok_or_else(|| OpsError::NotFound(format!("cleanup operation '{id}' not found")))
    }

    pub(crate) async fn retry_cleanup(
        &self,
        id: &str,
        actor: &str,
    ) -> Result<CleanupRun, OpsError> {
        let actor = sanitize_actor(actor);
        let current = self
            .get_cleanup(id)
            .await?
            .ok_or_else(|| OpsError::NotFound(format!("cleanup operation '{id}' not found")))?;
        if current.status == "completed" {
            return Ok(current);
        }
        self.prepare_cleanup_retry(id, &actor).await?;
        self.run_cleanup(id).await
    }

    async fn prepare_cleanup_retry(&self, id: &str, actor: &str) -> Result<(), OpsError> {
        sqlx::query(
            "UPDATE retention_cleanup_runs SET status='running',cancel_requested=FALSE,last_error_code=NULL,last_error_message=NULL,finished_at=NULL,updated_at=clock_timestamp() WHERE id=$1 AND status IN ('failed','cancelled','cancel_requested')",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        append_audit(
            &self.pool,
            id,
            "retention.cleanup.retry",
            "started",
            actor,
            json!({}),
            None,
            None,
        )
        .await
    }

    async fn run_cleanup(&self, id: &str) -> Result<CleanupRun, OpsError> {
        let run = self
            .get_cleanup(id)
            .await?
            .ok_or_else(|| OpsError::NotFound(format!("cleanup operation '{id}' not found")))?;
        let policies = policy_map_from_snapshot(&run.policy_snapshot, &run.cutoff_snapshot)?;
        if run.dry_run {
            return self.finish_dry_run(&run, &policies).await;
        }

        let mut rounds = 0;
        while rounds < run.max_batches {
            let progress = self
                .cleanup_round(&run, &policies, run.batches_completed + rounds)
                .await;
            match progress {
                Ok((deleted, cancelled)) => {
                    rounds += 1;
                    if cancelled {
                        break;
                    }
                    if deleted == 0 {
                        break;
                    }
                }
                Err(error) => {
                    self.mark_cleanup_failed(id, &error).await;
                    return Err(error);
                }
            }
        }
        let final_run = self
            .get_cleanup(id)
            .await?
            .ok_or_else(|| OpsError::NotFound(format!("cleanup operation '{id}' not found")))?;
        if final_run.status == "running" {
            // A bounded invocation intentionally leaves work pending.  The
            // caller can GET progress and POST /retry (or repeat the same
            // operation id) to continue without changing cut-offs.
            return Ok(final_run);
        }
        Ok(final_run)
    }

    async fn finish_dry_run(
        &self,
        run: &CleanupRun,
        policies: &BTreeMap<String, PolicyState>,
    ) -> Result<CleanupRun, OpsError> {
        if run.status == "completed" {
            return Ok(run.clone());
        }
        let counts = self.count_candidates(policies).await?;
        let progress = json!({
            "phase":"dry_run",
            "has_more": false,
            "dry_run": true,
            "candidates": counts,
        });
        sqlx::query(
            "UPDATE retention_cleanup_runs SET status='completed',scanned_usage_events=$2,scanned_usage_attempts=$3,scanned_audit=$4,scanned_discovery=$5,progress=$6,finished_at=clock_timestamp(),updated_at=clock_timestamp() WHERE id=$1 AND status NOT IN ('completed','cancelled')",
        )
        .bind(&run.id)
        .bind(counts.usage_events)
        .bind(counts.usage_attempts)
        .bind(counts.audit)
        .bind(counts.discovery)
        .bind(&progress)
        .execute(&self.pool)
        .await?;
        append_audit(
            &self.pool,
            &run.id,
            "retention.cleanup.completed",
            "succeeded",
            &run.requested_by,
            progress,
            None,
            None,
        )
        .await?;
        self.get_cleanup(&run.id)
            .await?
            .ok_or_else(|| OpsError::NotFound(format!("cleanup operation '{}' not found", run.id)))
    }

    async fn cleanup_round(
        &self,
        run: &CleanupRun,
        policies: &BTreeMap<String, PolicyState>,
        batch_number: i32,
    ) -> Result<(i64, bool), OpsError> {
        let mut tx = self.pool.begin().await?;
        // Usage events and attempts are written concurrently by request
        // handlers.  A serializable cleanup batch either observes a complete
        // committed attempt graph or aborts and can be retried without
        // cascading a newly inserted attempt.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&run.id)
            .execute(&mut *tx)
            .await?;
        let state: (String, bool) = sqlx::query_as(
            "SELECT status,cancel_requested FROM retention_cleanup_runs WHERE id=$1 FOR UPDATE",
        )
        .bind(&run.id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| OpsError::NotFound(format!("cleanup operation '{}' not found", run.id)))?;
        if matches!(state.0.as_str(), "completed" | "failed" | "cancelled") {
            tx.commit().await?;
            return Ok((0, state.0 == "cancelled"));
        }
        if state.1 || state.0 == "cancel_requested" {
            sqlx::query(
                "UPDATE retention_cleanup_runs SET status='cancelled',finished_at=clock_timestamp(),updated_at=clock_timestamp(),progress=jsonb_build_object('phase','cancelled','has_more',true) WHERE id=$1",
            )
            .bind(&run.id)
            .execute(&mut *tx)
            .await?;
            append_audit_tx(
                &mut tx,
                &run.id,
                "retention.cleanup.cancelled",
                "cancelled",
                &run.requested_by,
                json!({}),
                None,
                None,
            )
            .await?;
            tx.commit().await?;
            return Ok((0, true));
        }

        let mut deleted = CleanupCounts::default();
        if let Some(policy) = policies.get("usage_attempts").filter(|p| p.enabled) {
            deleted.usage_attempts =
                delete_attempt_batch(&mut tx, policy.cutoff, run.batch_size).await?;
        }
        if let Some(policy) = policies.get("usage_events").filter(|p| p.enabled) {
            let attempt_guard = attempt_guard(policies);
            deleted.usage_events =
                delete_event_batch(&mut tx, policy.cutoff, attempt_guard, run.batch_size).await?;
        }
        if let Some(policy) = policies.get("audit").filter(|p| p.enabled) {
            deleted.audit = delete_audit_batch(&mut tx, policy.cutoff, run.batch_size).await?;
        }
        if let Some(policy) = policies.get("discovery").filter(|p| p.enabled) {
            deleted.discovery =
                delete_discovery_batch(&mut tx, policy.cutoff, run.batch_size).await?;
        }
        let next_progress = json!({
            "phase":"batch",
            "batch": batch_number + 1,
            "has_more": true,
            "deleted": deleted,
            "updated_at": Utc::now(),
        });
        sqlx::query(
            "UPDATE retention_cleanup_runs SET scanned_usage_events=scanned_usage_events+$2,deleted_usage_events=deleted_usage_events+$2,scanned_usage_attempts=scanned_usage_attempts+$3,deleted_usage_attempts=deleted_usage_attempts+$3,scanned_audit=scanned_audit+$4,deleted_audit=deleted_audit+$4,scanned_discovery=scanned_discovery+$5,deleted_discovery=deleted_discovery+$5,batches_completed=batches_completed+1,progress=$6,updated_at=clock_timestamp() WHERE id=$1",
        )
        .bind(&run.id)
        .bind(deleted.usage_events)
        .bind(deleted.usage_attempts)
        .bind(deleted.audit)
        .bind(deleted.discovery)
        .bind(&next_progress)
        .execute(&mut *tx)
        .await?;
        append_audit_tx(
            &mut tx,
            &run.id,
            "retention.cleanup.progress",
            "progress",
            &run.requested_by,
            next_progress.clone(),
            None,
            None,
        )
        .await?;
        tx.commit().await?;

        tracing::info!(
            operation_id = %run.id,
            batch = batch_number + 1,
            deleted_usage_events = deleted.usage_events,
            deleted_usage_attempts = deleted.usage_attempts,
            deleted_audit = deleted.audit,
            deleted_discovery = deleted.discovery,
            "retention cleanup batch completed"
        );

        let has_more = self.count_candidates(policies).await?.total() > 0;
        sqlx::query(
            "UPDATE retention_cleanup_runs SET progress=jsonb_set(progress,'{has_more}',to_jsonb($2::boolean),true),updated_at=clock_timestamp() WHERE id=$1 AND status='running'",
        )
        .bind(&run.id)
        .bind(has_more)
        .execute(&self.pool)
        .await?;
        if !has_more {
            sqlx::query(
                "UPDATE retention_cleanup_runs SET status='completed',finished_at=clock_timestamp(),updated_at=clock_timestamp(),progress=jsonb_build_object('phase','completed','has_more',false,'batch',batches_completed) WHERE id=$1 AND status='running'",
            )
            .bind(&run.id)
            .execute(&self.pool)
            .await?;
            append_audit(
                &self.pool,
                &run.id,
                "retention.cleanup.completed",
                "succeeded",
                &run.requested_by,
                json!({}),
                None,
                None,
            )
            .await?;
        }
        Ok((deleted.total(), false))
    }

    async fn mark_cleanup_failed(&self, id: &str, error: &OpsError) {
        let code = error_code(error);
        let message = public_error_message(error);
        let _ = sqlx::query(
            "UPDATE retention_cleanup_runs SET status='failed',last_error_code=$2,last_error_message=$3,finished_at=clock_timestamp(),updated_at=clock_timestamp(),progress=jsonb_build_object('phase','failed','has_more',true) WHERE id=$1 AND status NOT IN ('completed','cancelled')",
        )
        .bind(id)
        .bind(code)
        .bind(&message)
        .execute(&self.pool)
        .await;
        let _ = append_audit(
            &self.pool,
            id,
            "retention.cleanup.failed",
            "failed",
            "system",
            json!({"error_code": code}),
            Some(code),
            Some(&message),
        )
        .await;
    }

    async fn load_policy_map(
        &self,
        selected: Option<&[String]>,
    ) -> Result<BTreeMap<String, PolicyState>, OpsError> {
        let selected = selected.map(|values| values.iter().collect::<BTreeSet<_>>());
        let rows = self.list_retention_policies().await?;
        let now = Utc::now();
        let mut policies = BTreeMap::new();
        for row in rows {
            if selected
                .as_ref()
                .is_some_and(|keys| !keys.contains(&row.policy_key))
            {
                continue;
            }
            policies.insert(
                row.policy_key,
                PolicyState {
                    retention_days: row.retention_days,
                    enabled: row.enabled,
                    cutoff: now - Duration::days(i64::from(row.retention_days)),
                },
            );
        }
        if let Some(keys) = selected {
            for key in keys {
                validate_policy_key(key)?;
                if !policies.contains_key(key) {
                    return Err(OpsError::NotFound(format!(
                        "retention policy '{key}' not found"
                    )));
                }
            }
        }
        Ok(policies)
    }

    async fn count_candidates(
        &self,
        policies: &BTreeMap<String, PolicyState>,
    ) -> Result<CleanupCounts, OpsError> {
        let mut counts = CleanupCounts::default();
        if let Some(policy) = policies.get("usage_attempts").filter(|p| p.enabled) {
            counts.usage_attempts = sqlx::query_scalar(
                "SELECT COUNT(*)::BIGINT FROM usage_event_attempts WHERE created_at < $1",
            )
            .bind(policy.cutoff)
            .fetch_one(&self.pool)
            .await?;
        }
        if let Some(policy) = policies.get("usage_events").filter(|p| p.enabled) {
            counts.usage_events = match attempt_guard(policies) {
                AttemptGuard::EligibleBefore(cutoff) => sqlx::query_scalar(
                    "SELECT COUNT(*)::BIGINT FROM usage_events e WHERE e.created_at < $1 AND NOT EXISTS (SELECT 1 FROM usage_event_attempts a WHERE a.request_id=e.request_id AND a.created_at >= $2)",
                )
                .bind(policy.cutoff)
                .bind(cutoff)
                .fetch_one(&self.pool)
                .await?,
                AttemptGuard::KeepAll => sqlx::query_scalar(
                    "SELECT COUNT(*)::BIGINT FROM usage_events e WHERE e.created_at < $1 AND NOT EXISTS (SELECT 1 FROM usage_event_attempts a WHERE a.request_id=e.request_id)",
                )
                .bind(policy.cutoff)
                .fetch_one(&self.pool)
                .await?,
            };
        }
        if let Some(policy) = policies.get("audit").filter(|p| p.enabled) {
            let logs: i64 =
                sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM audit_logs WHERE created_at < $1")
                    .bind(policy.cutoff)
                    .fetch_one(&self.pool)
                    .await?;
            let tests: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)::BIGINT FROM source_connection_tests WHERE tested_at < $1",
            )
            .bind(policy.cutoff)
            .fetch_one(&self.pool)
            .await?;
            let health: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)::BIGINT FROM account_health_events WHERE created_at < $1",
            )
            .bind(policy.cutoff)
            .fetch_one(&self.pool)
            .await?;
            counts.audit = logs + tests + health;
        }
        if let Some(policy) = policies.get("discovery").filter(|p| p.enabled) {
            counts.discovery = sqlx::query_scalar(
                "SELECT COUNT(*)::BIGINT FROM source_discovery_runs WHERE completed_at < $1",
            )
            .bind(policy.cutoff)
            .fetch_one(&self.pool)
            .await?;
        }
        Ok(counts)
    }

    pub(crate) async fn export_control_plane(
        &self,
        control_plane: &ControlPlane,
        requested_by: &str,
    ) -> Result<ControlPlaneExportResult, OpsError> {
        let backup_id = Uuid::new_v4().to_string();
        let actor = sanitize_actor(requested_by);
        let metadata = self.schema_metadata().await?;
        sqlx::query("INSERT INTO backup_runs (id,operation,status,requested_by,schema_version,migration_version,format,metadata) VALUES ($1,'control_plane_export','running',$2,$3,$4,'gateway_control_plane_json','{}'::jsonb)")
            .bind(&backup_id).bind(&actor).bind(metadata.schema_version).bind(metadata.migration_version)
            .execute(&self.pool).await?;

        let result = self
            .build_control_plane_export(control_plane, &metadata)
            .await;
        match result {
            Ok(export) => {
                let checksum = sha256_json(&export)?;
                sqlx::query("UPDATE backup_runs SET status='succeeded',checksum=$2,metadata=$3,completed_at=clock_timestamp() WHERE id=$1")
                    .bind(&backup_id).bind(&checksum).bind(json!({"runtime_snapshot_fingerprint": export.runtime_snapshot.fingerprint,"row_counts": export.row_counts}))
                    .execute(&self.pool).await?;
                append_audit(&self.pool, &backup_id, "backup.control_plane_export", "succeeded", &actor, json!({"checksum":checksum,"schema_version":metadata.schema_version,"migration_version":metadata.migration_version}), None, None).await?;
                Ok(ControlPlaneExportResult {
                    backup_id,
                    checksum,
                    export,
                })
            }
            Err(error) => {
                self.mark_backup_failed(&backup_id, &error).await;
                Err(error)
            }
        }
    }

    async fn build_control_plane_export(
        &self,
        control_plane: &ControlPlane,
        metadata: &SchemaMetadata,
    ) -> Result<ControlPlaneExport, OpsError> {
        let snapshot = control_plane.load_snapshot().await?;
        let runtime = runtime_snapshot_metadata(&snapshot)?;
        let mut provider_presets = self.table_rows("provider_presets", "id,version").await?;
        let mut model_presets = self.table_rows("model_presets", "id,version").await?;
        let mut sources = self.table_rows("sources", "id").await?;
        let mut accounts = self.table_rows("accounts", "id").await?;
        let mut source_models = self
            .table_rows("source_models", "source_id,upstream_model_id")
            .await?;
        let mut source_model_capabilities = self
            .table_rows(
                "source_model_capabilities",
                "source_id,upstream_model_id,protocol",
            )
            .await?;
        let mut logical_models = self.table_rows("logical_models", "id").await?;
        let mut model_bindings = self.table_rows("model_bindings", "id").await?;
        let mut routes = self.table_rows("routes", "id").await?;
        let mut providers = self.table_rows("providers", "id").await?;
        let mut virtual_keys = self.table_rows("virtual_keys", "id").await?;

        for row in &mut provider_presets {
            *row = sanitize_json(row.clone());
        }
        for row in &mut model_presets {
            *row = sanitize_json(row.clone());
        }
        for row in &mut providers {
            *row = sanitize_json(row.clone());
        }
        for row in &mut sources {
            *row = sanitize_json(row.clone());
            sanitize_source_row(row);
        }
        for row in &mut accounts {
            let credential_env = row.get("credential_env").cloned().unwrap_or(Value::Null);
            *row = sanitize_json(row.clone());
            if let Some(object) = row.as_object_mut() {
                object.insert("credential_env".into(), credential_env);
            }
            sanitize_account_row(row);
        }
        for rows in [
            &mut source_models,
            &mut source_model_capabilities,
            &mut logical_models,
            &mut model_bindings,
            &mut routes,
        ] {
            for row in rows.iter_mut() {
                *row = sanitize_json(row.clone());
            }
        }
        for row in &mut virtual_keys {
            sanitize_virtual_key_row(row);
        }
        let row_counts = json!({
            "provider_presets": provider_presets.len(),
            "model_presets": model_presets.len(),
            "sources": sources.len(),
            "accounts": accounts.len(),
            "source_models": source_models.len(),
            "source_model_capabilities": source_model_capabilities.len(),
            "logical_models": logical_models.len(),
            "model_bindings": model_bindings.len(),
            "routes": routes.len(),
            "providers": providers.len(),
            "virtual_keys": virtual_keys.len(),
        });
        if row_counts
            .as_object()
            .into_iter()
            .flat_map(|map| map.values())
            .filter_map(Value::as_u64)
            .any(|count| count > MAX_EXPORT_ROWS as u64)
        {
            return Err(OpsError::Validation(format!(
                "control-plane export exceeds {MAX_EXPORT_ROWS} rows in one table"
            )));
        }
        Ok(ControlPlaneExport {
            format: "my-ai-gateway.control-plane".into(),
            version: 1,
            exported_at: Utc::now(),
            timezone: "UTC".into(),
            schema_version: metadata.schema_version,
            migration_version: metadata.migration_version,
            credentials: json!({
                "mode":"secret_ref_only",
                "description":"credential values and ciphertext are never exported"
            }),
            runtime_snapshot: runtime,
            row_counts,
            provider_presets,
            model_presets,
            sources,
            accounts,
            source_models,
            source_model_capabilities,
            logical_models,
            model_bindings,
            routes,
            providers,
            virtual_keys,
        })
    }

    async fn table_rows(&self, table: &str, order_by: &str) -> Result<Vec<Value>, OpsError> {
        // Table and order names are compile-time call-site constants; they are
        // never accepted from an HTTP request.
        let query =
            format!("SELECT to_jsonb(r) FROM (SELECT * FROM {table} ORDER BY {order_by}) AS r");
        Ok(sqlx::query_scalar::<_, Value>(&query)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn mark_backup_failed(&self, id: &str, error: &OpsError) {
        let code = error_code(error);
        let message = public_error_message(error);
        let _ = sqlx::query("UPDATE backup_runs SET status='failed',error_code=$2,error_message=$3,completed_at=clock_timestamp() WHERE id=$1")
            .bind(id).bind(code).bind(&message).execute(&self.pool).await;
        let _ = append_audit(
            &self.pool,
            id,
            "backup.operation.failed",
            "failed",
            "system",
            json!({"error_code":code}),
            Some(code),
            Some(&message),
        )
        .await;
    }

    pub(crate) async fn restore_control_plane(
        &self,
        control_plane: &ControlPlane,
        export: &ControlPlaneExport,
        replace: bool,
        requested_by: &str,
    ) -> Result<RestoreResult, OpsError> {
        validate_export_shape(export)?;
        let metadata = self.schema_metadata().await?;
        if export.schema_version > metadata.schema_version
            || export.migration_version > metadata.migration_version
        {
            return Err(OpsError::Validation(
                "export was created by a newer schema/migration than this database".into(),
            ));
        }
        let backup_id = Uuid::new_v4().to_string();
        let actor = sanitize_actor(requested_by);
        sqlx::query("INSERT INTO backup_runs (id,operation,status,requested_by,schema_version,migration_version,format,metadata) VALUES ($1,'restore','running',$2,$3,$4,'gateway_control_plane_json','{}'::jsonb)")
            .bind(&backup_id).bind(&actor).bind(metadata.schema_version).bind(metadata.migration_version)
            .execute(&self.pool).await?;

        let restore_result = self
            .restore_rows(control_plane, export, replace, &backup_id, &actor)
            .await;
        let (snapshot, skipped_virtual_keys) = match restore_result {
            Ok(value) => value,
            Err(error) => {
                self.mark_backup_failed(&backup_id, &error).await;
                return Err(error);
            }
        };
        let actual_fingerprint = runtime_snapshot_fingerprint(&snapshot)?;
        if actual_fingerprint != export.runtime_snapshot.fingerprint {
            let error = OpsError::Snapshot(
                "restored control plane does not produce the exported runtime snapshot".into(),
            );
            self.mark_backup_failed(&backup_id, &error).await;
            return Err(error);
        }
        let checksum = sha256_json(export)?;
        sqlx::query("UPDATE backup_runs SET status='succeeded',checksum=$2,metadata=$3,completed_at=clock_timestamp() WHERE id=$1")
            .bind(&backup_id)
            .bind(&checksum)
            .bind(json!({"verified":true,"runtime_snapshot_fingerprint":actual_fingerprint,"skipped_virtual_keys":skipped_virtual_keys,"source_schema_version":export.schema_version,"source_migration_version":export.migration_version}))
            .execute(&self.pool).await?;
        append_audit(&self.pool, &backup_id, "backup.restore", "succeeded", &actor, json!({"verified":true,"schema_version":export.schema_version,"migration_version":export.migration_version}), None, None).await?;
        Ok(RestoreResult {
            backup_id,
            verified: true,
            snapshot,
            skipped_virtual_keys,
        })
    }

    async fn restore_rows(
        &self,
        control_plane: &ControlPlane,
        export: &ControlPlaneExport,
        replace: bool,
        operation_id: &str,
        actor: &str,
    ) -> Result<(RuntimeSnapshot, i64), OpsError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await?;
        let non_empty: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM sources)+(SELECT COUNT(*) FROM accounts)+(SELECT COUNT(*) FROM logical_models)+(SELECT COUNT(*) FROM model_bindings)+(SELECT COUNT(*) FROM routes)",
        )
        .fetch_one(&mut *tx)
        .await?;
        if non_empty > 0 && !replace {
            return Err(OpsError::Conflict(
                "destination control plane is not empty; pass replace=true explicitly".into(),
            ));
        }
        if replace {
            clear_control_plane_tx(&mut tx).await?;
        }
        restore_provider_presets(&mut tx, &export.provider_presets).await?;
        restore_model_presets(&mut tx, &export.model_presets).await?;
        restore_sources(&mut tx, &export.sources).await?;
        restore_legacy_providers(&mut tx, &export.providers).await?;
        restore_accounts(&mut tx, &export.accounts).await?;
        restore_source_models(&mut tx, &export.source_models).await?;
        restore_capabilities(&mut tx, &export.source_model_capabilities).await?;
        restore_logical_models(&mut tx, &export.logical_models).await?;
        restore_bindings(&mut tx, &export.model_bindings).await?;
        restore_routes(&mut tx, &export.routes).await?;
        let skipped_virtual_keys = restore_virtual_keys(&mut tx, &export.virtual_keys).await?;
        restore_snapshot_state(&mut tx, &export.runtime_snapshot).await?;
        reset_sequences(&mut tx).await?;
        let snapshot = control_plane.load_snapshot_in_transaction(&mut tx).await?;
        let fingerprint = runtime_snapshot_fingerprint(&snapshot)?;
        if fingerprint != export.runtime_snapshot.fingerprint {
            return Err(OpsError::Snapshot(
                "restored control plane does not produce the exported runtime snapshot".into(),
            ));
        }
        append_audit_tx(
            &mut tx,
            operation_id,
            "backup.restore.started",
            "progress",
            actor,
            json!({"replace":replace}),
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok((snapshot, skipped_virtual_keys))
    }
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub(crate) struct SchemaMetadata {
    pub(crate) schema_version: i32,
    pub(crate) migration_version: i32,
    pub(crate) application_version: String,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow, PartialEq)]
pub(crate) struct RetentionPolicy {
    pub(crate) policy_key: String,
    pub(crate) retention_days: i32,
    pub(crate) enabled: bool,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RetentionPolicyWrite {
    pub(crate) policy_key: String,
    pub(crate) retention_days: i32,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CleanupRequest {
    #[serde(default)]
    pub(crate) dry_run: bool,
    #[serde(default = "default_batch_size")]
    pub(crate) batch_size: i32,
    #[serde(default = "default_max_batches")]
    pub(crate) max_batches: i32,
    #[serde(default)]
    pub(crate) operation_id: Option<String>,
    #[serde(default)]
    pub(crate) requested_by: Option<String>,
    #[serde(default)]
    pub(crate) policy_keys: Option<Vec<String>>,
}

impl Default for CleanupRequest {
    fn default() -> Self {
        Self {
            dry_run: false,
            batch_size: DEFAULT_BATCH_SIZE,
            max_batches: DEFAULT_MAX_BATCHES,
            operation_id: None,
            requested_by: None,
            policy_keys: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub(crate) struct CleanupRun {
    pub(crate) id: String,
    pub(crate) status: String,
    pub(crate) dry_run: bool,
    pub(crate) batch_size: i32,
    pub(crate) max_batches: i32,
    pub(crate) requested_by: String,
    pub(crate) policy_snapshot: Value,
    pub(crate) cutoff_snapshot: Value,
    pub(crate) scanned_usage_events: i64,
    pub(crate) deleted_usage_events: i64,
    pub(crate) scanned_usage_attempts: i64,
    pub(crate) deleted_usage_attempts: i64,
    pub(crate) scanned_audit: i64,
    pub(crate) deleted_audit: i64,
    pub(crate) scanned_discovery: i64,
    pub(crate) deleted_discovery: i64,
    pub(crate) batches_completed: i32,
    pub(crate) progress: Value,
    pub(crate) last_error_code: Option<String>,
    pub(crate) last_error_message: Option<String>,
    pub(crate) cancel_requested: bool,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) finished_at: Option<DateTime<Utc>>,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub(crate) struct AuditLog {
    pub(crate) id: i64,
    pub(crate) operation_id: String,
    pub(crate) action: String,
    pub(crate) status: String,
    pub(crate) actor: String,
    pub(crate) details: Value,
    pub(crate) error_code: Option<String>,
    pub(crate) error_message: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub(crate) struct BackupRun {
    pub(crate) id: String,
    pub(crate) operation: String,
    pub(crate) status: String,
    pub(crate) requested_by: String,
    pub(crate) schema_version: i32,
    pub(crate) migration_version: i32,
    pub(crate) format: String,
    pub(crate) checksum: Option<String>,
    pub(crate) metadata: Value,
    pub(crate) error_code: Option<String>,
    pub(crate) error_message: Option<String>,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RuntimeSnapshotMetadata {
    pub(crate) revision: i64,
    pub(crate) generated_at: DateTime<Utc>,
    pub(crate) fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ControlPlaneExport {
    pub(crate) format: String,
    pub(crate) version: i32,
    pub(crate) exported_at: DateTime<Utc>,
    pub(crate) timezone: String,
    pub(crate) schema_version: i32,
    pub(crate) migration_version: i32,
    pub(crate) credentials: Value,
    pub(crate) runtime_snapshot: RuntimeSnapshotMetadata,
    pub(crate) row_counts: Value,
    #[serde(default)]
    pub(crate) provider_presets: Vec<Value>,
    #[serde(default)]
    pub(crate) model_presets: Vec<Value>,
    #[serde(default)]
    pub(crate) sources: Vec<Value>,
    #[serde(default)]
    pub(crate) accounts: Vec<Value>,
    #[serde(default)]
    pub(crate) source_models: Vec<Value>,
    #[serde(default)]
    pub(crate) source_model_capabilities: Vec<Value>,
    #[serde(default)]
    pub(crate) logical_models: Vec<Value>,
    #[serde(default)]
    pub(crate) model_bindings: Vec<Value>,
    #[serde(default)]
    pub(crate) routes: Vec<Value>,
    #[serde(default)]
    pub(crate) providers: Vec<Value>,
    #[serde(default)]
    pub(crate) virtual_keys: Vec<Value>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ControlPlaneExportResult {
    pub(crate) backup_id: String,
    pub(crate) checksum: String,
    pub(crate) export: ControlPlaneExport,
}

pub(crate) fn control_plane_export_checksum(
    export: &ControlPlaneExport,
) -> Result<String, OpsError> {
    sha256_json(export)
}

#[derive(Clone)]
pub(crate) struct RestoreResult {
    pub(crate) backup_id: String,
    pub(crate) verified: bool,
    pub(crate) snapshot: RuntimeSnapshot,
    pub(crate) skipped_virtual_keys: i64,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct CleanupCounts {
    usage_events: i64,
    usage_attempts: i64,
    audit: i64,
    discovery: i64,
}

impl CleanupCounts {
    fn total(self) -> i64 {
        self.usage_events + self.usage_attempts + self.audit + self.discovery
    }
}

#[derive(Clone, Copy, Debug)]
struct PolicyState {
    retention_days: i32,
    enabled: bool,
    cutoff: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug)]
enum AttemptGuard {
    EligibleBefore(DateTime<Utc>),
    KeepAll,
}

fn attempt_guard(policies: &BTreeMap<String, PolicyState>) -> AttemptGuard {
    policies
        .get("usage_attempts")
        .filter(|policy| policy.enabled)
        .map(|policy| AttemptGuard::EligibleBefore(policy.cutoff))
        .unwrap_or(AttemptGuard::KeepAll)
}

fn cleanup_select(filter: Option<&str>) -> &'static str {
    match filter {
        Some(_) => "SELECT id,status,dry_run,batch_size,max_batches,requested_by,policy_snapshot,cutoff_snapshot,scanned_usage_events,deleted_usage_events,scanned_usage_attempts,deleted_usage_attempts,scanned_audit,deleted_audit,scanned_discovery,deleted_discovery,batches_completed,progress,last_error_code,last_error_message,cancel_requested,created_at,started_at,finished_at,updated_at FROM retention_cleanup_runs WHERE id=$1",
        None => "SELECT id,status,dry_run,batch_size,max_batches,requested_by,policy_snapshot,cutoff_snapshot,scanned_usage_events,deleted_usage_events,scanned_usage_attempts,deleted_usage_attempts,scanned_audit,deleted_audit,scanned_discovery,deleted_discovery,batches_completed,progress,last_error_code,last_error_message,cancel_requested,created_at,started_at,finished_at,updated_at FROM retention_cleanup_runs ORDER BY created_at DESC,id DESC LIMIT $1",
    }
}

fn validate_cleanup_request(request: &CleanupRequest) -> Result<(), OpsError> {
    if !(1..=MAX_BATCH_SIZE).contains(&request.batch_size) {
        return Err(OpsError::Validation(format!(
            "batch_size must be between 1 and {MAX_BATCH_SIZE}"
        )));
    }
    if !(1..=MAX_MAX_BATCHES).contains(&request.max_batches) {
        return Err(OpsError::Validation(format!(
            "max_batches must be between 1 and {MAX_MAX_BATCHES}"
        )));
    }
    if let Some(id) = &request.operation_id {
        if id.trim().is_empty() || id.len() > 128 {
            return Err(OpsError::Validation(
                "operation_id must be 1..128 characters".into(),
            ));
        }
    }
    if let Some(keys) = &request.policy_keys {
        let mut seen = BTreeSet::new();
        for key in keys {
            validate_policy_key(key)?;
            if !seen.insert(key) {
                return Err(OpsError::Validation(format!(
                    "duplicate policy key '{key}'"
                )));
            }
        }
    }
    Ok(())
}

fn validate_policy_key(key: &str) -> Result<(), OpsError> {
    if RETENTION_KEYS.contains(&key) {
        Ok(())
    } else {
        Err(OpsError::Validation(format!(
            "unknown retention policy '{key}'"
        )))
    }
}

fn validate_retention_days(days: i32) -> Result<(), OpsError> {
    if (0..=36_500).contains(&days) {
        Ok(())
    } else {
        Err(OpsError::Validation(
            "retention_days must be between 0 and 36500".into(),
        ))
    }
}

fn policy_snapshot(policies: &BTreeMap<String, PolicyState>) -> Value {
    let mut map = Map::new();
    for (key, state) in policies {
        map.insert(
            key.clone(),
            json!({"retention_days":state.retention_days,"enabled":state.enabled}),
        );
    }
    Value::Object(map)
}

fn cutoff_snapshot(policies: &BTreeMap<String, PolicyState>) -> Value {
    let mut map = Map::new();
    for (key, state) in policies {
        map.insert(
            key.clone(),
            state
                .enabled
                .then(|| state.cutoff.to_rfc3339())
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
    }
    Value::Object(map)
}

fn policy_map_from_snapshot(
    value: &Value,
    cutoffs: &Value,
) -> Result<BTreeMap<String, PolicyState>, OpsError> {
    let object = value
        .as_object()
        .ok_or_else(|| OpsError::Validation("cleanup policy_snapshot must be an object".into()))?;
    let cutoff_object = cutoffs
        .as_object()
        .ok_or_else(|| OpsError::Validation("cleanup cutoff_snapshot must be an object".into()))?;
    let mut policies = BTreeMap::new();
    for (key, state) in object {
        validate_policy_key(key)?;
        let state = state
            .as_object()
            .ok_or_else(|| OpsError::Validation("cleanup policy value must be an object".into()))?;
        let days = state
            .get("retention_days")
            .and_then(Value::as_i64)
            .ok_or_else(|| OpsError::Validation("cleanup retention_days is invalid".into()))?;
        let days = i32::try_from(days)
            .map_err(|_| OpsError::Validation("cleanup retention_days is invalid".into()))?;
        validate_retention_days(days)?;
        let cutoff = match cutoff_object.get(key) {
            Some(Value::String(value)) => DateTime::parse_from_rfc3339(value)
                .map(|date| date.with_timezone(&Utc))
                .map_err(|_| OpsError::Validation("cleanup cutoff is invalid".into()))?,
            Some(Value::Null) | None => Utc::now() - Duration::days(i64::from(days)),
            _ => return Err(OpsError::Validation("cleanup cutoff is invalid".into())),
        };
        policies.insert(
            key.clone(),
            PolicyState {
                retention_days: days,
                enabled: state
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                cutoff,
            },
        );
    }
    Ok(policies)
}

fn default_true() -> bool {
    true
}
fn default_batch_size() -> i32 {
    DEFAULT_BATCH_SIZE
}
fn default_max_batches() -> i32 {
    DEFAULT_MAX_BATCHES
}

fn error_code(error: &OpsError) -> &'static str {
    match error {
        OpsError::Database(_) => "database_error",
        OpsError::Json(_) | OpsError::Validation(_) => "invalid_operation",
        OpsError::NotFound(_) => "not_found",
        OpsError::Conflict(_) => "operation_conflict",
        OpsError::Snapshot(_) => "snapshot_verification_failed",
    }
}

fn public_error_message(error: &OpsError) -> String {
    match error {
        OpsError::Database(_) => "database operation failed".into(),
        OpsError::Json(_) => "operation payload is invalid".into(),
        OpsError::Validation(message)
        | OpsError::NotFound(message)
        | OpsError::Conflict(message)
        | OpsError::Snapshot(message) => message.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn append_audit(
    pool: &PgPool,
    operation_id: &str,
    action: &str,
    status: &str,
    actor: &str,
    details: Value,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> Result<(), OpsError> {
    sqlx::query(
        "INSERT INTO audit_logs (operation_id,action,status,actor,details,error_code,error_message,completed_at) VALUES ($1,$2,$3,$4,$5,$6,$7,CASE WHEN $3 IN ('succeeded','failed','cancelled') THEN clock_timestamp() ELSE NULL END)",
    )
    .bind(operation_id)
    .bind(action)
    .bind(status)
    .bind(sanitize_actor(actor))
    .bind(sanitize_json(details))
    .bind(error_code)
    .bind(error_message.map(redact_text))
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn append_audit_tx(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: &str,
    action: &str,
    status: &str,
    actor: &str,
    details: Value,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> Result<(), OpsError> {
    sqlx::query(
        "INSERT INTO audit_logs (operation_id,action,status,actor,details,error_code,error_message,completed_at) VALUES ($1,$2,$3,$4,$5,$6,$7,CASE WHEN $3 IN ('succeeded','failed','cancelled') THEN clock_timestamp() ELSE NULL END)",
    )
    .bind(operation_id)
    .bind(action)
    .bind(status)
    .bind(sanitize_actor(actor))
    .bind(sanitize_json(details))
    .bind(error_code)
    .bind(error_message.map(redact_text))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn delete_attempt_batch(
    tx: &mut Transaction<'_, Postgres>,
    cutoff: DateTime<Utc>,
    batch_size: i32,
) -> Result<i64, OpsError> {
    Ok(sqlx::query(
        "DELETE FROM usage_event_attempts WHERE id IN (SELECT id FROM usage_event_attempts WHERE created_at < $1 ORDER BY id LIMIT $2)",
    )
    .bind(cutoff)
    .bind(batch_size)
    .execute(&mut **tx)
    .await?
    .rows_affected() as i64)
}

async fn delete_event_batch(
    tx: &mut Transaction<'_, Postgres>,
    cutoff: DateTime<Utc>,
    attempt_guard: AttemptGuard,
    batch_size: i32,
) -> Result<i64, OpsError> {
    let result = match attempt_guard {
        AttemptGuard::EligibleBefore(attempt_cutoff) => sqlx::query(
            "DELETE FROM usage_events WHERE id IN (SELECT e.id FROM usage_events e WHERE e.created_at < $1 AND NOT EXISTS (SELECT 1 FROM usage_event_attempts a WHERE a.request_id=e.request_id AND a.created_at >= $2) ORDER BY e.id LIMIT $3)",
        )
        .bind(cutoff)
        .bind(attempt_cutoff)
        .bind(batch_size)
        .execute(&mut **tx)
        .await?,
        AttemptGuard::KeepAll => sqlx::query(
            "DELETE FROM usage_events WHERE id IN (SELECT e.id FROM usage_events e WHERE e.created_at < $1 AND NOT EXISTS (SELECT 1 FROM usage_event_attempts a WHERE a.request_id=e.request_id) ORDER BY e.id LIMIT $2)",
        )
        .bind(cutoff)
        .bind(batch_size)
        .execute(&mut **tx)
        .await?,
    };
    Ok(result.rows_affected() as i64)
}

async fn delete_audit_batch(
    tx: &mut Transaction<'_, Postgres>,
    cutoff: DateTime<Utc>,
    batch_size: i32,
) -> Result<i64, OpsError> {
    let logs = sqlx::query(
        "DELETE FROM audit_logs WHERE id IN (SELECT id FROM audit_logs WHERE created_at < $1 ORDER BY id LIMIT $2)",
    )
    .bind(cutoff)
    .bind(batch_size)
    .execute(&mut **tx)
    .await?
    .rows_affected() as i64;
    let tests = sqlx::query(
        "DELETE FROM source_connection_tests WHERE id IN (SELECT id FROM source_connection_tests WHERE tested_at < $1 ORDER BY id LIMIT $2)",
    )
    .bind(cutoff)
    .bind(batch_size)
    .execute(&mut **tx)
    .await?
    .rows_affected() as i64;
    let health = sqlx::query(
        "DELETE FROM account_health_events WHERE id IN (SELECT id FROM account_health_events WHERE created_at < $1 ORDER BY id LIMIT $2)",
    )
    .bind(cutoff)
    .bind(batch_size)
    .execute(&mut **tx)
    .await?
    .rows_affected() as i64;
    Ok(logs + tests + health)
}

async fn delete_discovery_batch(
    tx: &mut Transaction<'_, Postgres>,
    cutoff: DateTime<Utc>,
    batch_size: i32,
) -> Result<i64, OpsError> {
    Ok(sqlx::query(
        "DELETE FROM source_discovery_runs WHERE id IN (SELECT id FROM source_discovery_runs WHERE completed_at < $1 ORDER BY id LIMIT $2)",
    )
    .bind(cutoff)
    .bind(batch_size)
    .execute(&mut **tx)
    .await?
    .rows_affected() as i64)
}

fn redact_text(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.contains("authorization")
        || lower.contains("bearer ")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("secret")
        || lower.contains("password")
    {
        "redacted".into()
    } else {
        value.chars().take(512).collect()
    }
}

fn sanitize_actor(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "system".into();
    }
    let sanitized = redact_text(trimmed);
    sanitized
        .chars()
        .filter(|character| !character.is_control())
        .take(128)
        .collect()
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    matches!(
        key.as_str(),
        "authorization"
            | "api_key"
            | "apikey"
            | "access_token"
            | "refresh_token"
            | "secret"
            | "password"
            | "private_key"
            | "credential"
            | "credential_ciphertext"
            | "key_hash"
    ) || key.ends_with("_secret")
        || key.ends_with("_password")
        || key.ends_with("-secret")
        || key.ends_with("-password")
        || key.ends_with("_key")
        || key.ends_with("-key")
        || key.starts_with("x-api-")
        || key.contains("api_key")
        || key.contains("api-key")
        || key.contains("apikey")
        || matches!(key.as_str(), "auth_token" | "session_token")
}

fn looks_like_secret(value: &str) -> bool {
    let value = value.trim();
    (value.starts_with("sk-") && value.len() >= 16)
        || (value.starts_with("sk_") && value.len() >= 16)
        || (value.starts_with("ghp_") && value.len() >= 20)
        || (value.starts_with("Bearer ") && value.len() >= 20)
}

fn sanitize_string(value: String) -> String {
    if looks_like_secret(&value) {
        return "[REDACTED]".into();
    }
    if let Ok(mut url) = reqwest::Url::parse(&value) {
        let query_is_sensitive = url.query().is_some_and(|query| {
            let query = query.to_ascii_lowercase();
            query.contains("api_key")
                || query.contains("apikey")
                || query.contains("token")
                || query.contains("secret")
                || query.contains("password")
        });
        if !url.username().is_empty()
            || url.password().is_some()
            || query_is_sensitive
            || url.fragment().is_some()
        {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            if query_is_sensitive {
                url.set_query(None);
            }
            url.set_fragment(None);
            return url.to_string();
        }
    }
    value
}

fn sanitize_json(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    if is_sensitive_key(&key) {
                        (key, Value::String("[REDACTED]".into()))
                    } else {
                        let value = sanitize_json(value);
                        let value = match value {
                            Value::String(text) if looks_like_secret(&text) => {
                                Value::String("[REDACTED]".into())
                            }
                            other => other,
                        };
                        (key, value)
                    }
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(sanitize_json).collect()),
        Value::String(value) => Value::String(sanitize_string(value)),
        other => other,
    }
}

fn sanitize_source_row(row: &mut Value) {
    if let Some(object) = row.as_object_mut() {
        for key in ["auth_config", "provider_preset_snapshot"] {
            if let Some(value) = object.remove(key) {
                object.insert(key.into(), sanitize_json(value));
            }
        }
    }
}

fn sanitize_account_row(row: &mut Value) {
    let Some(object) = row.as_object_mut() else {
        return;
    };
    let env = object
        .get("credential_env")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let had_ciphertext = object
        .get("credential_ciphertext")
        .is_some_and(|value| !value.is_null());
    object.remove("credential_ciphertext");
    object.insert(
        "credential".into(),
        match env {
            Some(name) if !name.trim().is_empty() => {
                json!({"kind":"secret_ref","name":name})
            }
            None if had_ciphertext => json!({"kind":"redacted"}),
            _ => json!({"kind":"none"}),
        },
    );
    if let Some(value) = object.remove("auth_config") {
        object.insert("auth_config".into(), sanitize_json(value));
    }
}

fn sanitize_virtual_key_row(row: &mut Value) {
    if let Some(object) = row.as_object_mut() {
        object.remove("key_hash");
        object.remove("key_ciphertext");
    }
}

fn runtime_snapshot_metadata(
    snapshot: &RuntimeSnapshot,
) -> Result<RuntimeSnapshotMetadata, OpsError> {
    Ok(RuntimeSnapshotMetadata {
        revision: snapshot.revision,
        generated_at: snapshot.generated_at,
        fingerprint: runtime_snapshot_fingerprint(snapshot)?,
    })
}

fn runtime_snapshot_fingerprint(snapshot: &RuntimeSnapshot) -> Result<String, OpsError> {
    let mut config = serde_json::to_value(&*snapshot.config)?;
    if let Some(object) = config.as_object_mut() {
        // The listener is deployment-local and may legitimately differ on a
        // restore target; routing and model state remain part of the digest.
        object.remove("listen_addr");
    }
    let routes = snapshot
        .resolver
        .runtime_routes()
        .map(|routes| {
            routes
                .iter()
                .map(|route| {
                    json!({
                        "route_id": route.route_id,
                        "model": route.model,
                        "protocol": route.protocol,
                        "allow_lossy_conversion": route.allow_lossy_conversion,
                        "bindings": route.bindings.iter().map(|binding| json!({
                            "binding_id": binding.binding_id,
                            "source_id": binding.source_id,
                            "provider_id": binding.provider_id,
                            "account_id": binding.account_id,
                            "upstream_model_id": binding.upstream_model_id,
                            "protocol_upstream": binding.protocol_upstream,
                            "upstream_endpoint": binding.upstream_endpoint,
                            "mode": binding.mode,
                            "adapter": binding.adapter,
                            "effective_capabilities": binding.effective_capabilities,
                            "degraded_features": binding.degraded_features,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    sha256_json(&json!({
        "config": config,
        "models": &*snapshot.models,
        "routes": routes,
    }))
}

fn sha256_json<T: Serialize>(value: &T) -> Result<String, OpsError> {
    let bytes = serde_json::to_vec(value)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn validate_export_shape(export: &ControlPlaneExport) -> Result<(), OpsError> {
    if export.format != "my-ai-gateway.control-plane" || export.version != 1 {
        return Err(OpsError::Validation(
            "unsupported control-plane export format or version".into(),
        ));
    }
    if export.timezone != "UTC" {
        return Err(OpsError::Validation(
            "control-plane exports must use UTC".into(),
        ));
    }
    if export.schema_version <= 0 || export.migration_version <= 0 {
        return Err(OpsError::Validation(
            "control-plane export schema metadata is invalid".into(),
        ));
    }
    if export.runtime_snapshot.fingerprint.len() != 64
        || !export
            .runtime_snapshot
            .fingerprint
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(OpsError::Validation(
            "control-plane runtime snapshot fingerprint is invalid".into(),
        ));
    }
    for rows in [
        &export.provider_presets,
        &export.model_presets,
        &export.sources,
        &export.accounts,
        &export.source_models,
        &export.source_model_capabilities,
        &export.logical_models,
        &export.model_bindings,
        &export.routes,
        &export.providers,
        &export.virtual_keys,
    ] {
        if rows.len() > MAX_EXPORT_ROWS {
            return Err(OpsError::Validation(format!(
                "control-plane export exceeds {MAX_EXPORT_ROWS} rows"
            )));
        }
        for row in rows {
            if !row.is_object() {
                return Err(OpsError::Validation(
                    "control-plane export rows must be JSON objects".into(),
                ));
            }
            reject_plaintext_secret_fields(row)?;
        }
    }
    Ok(())
}

fn reject_plaintext_secret_fields(value: &Value) -> Result<(), OpsError> {
    let Some(object) = value.as_object() else {
        if let Value::Array(values) = value {
            for value in values {
                reject_plaintext_secret_fields(value)?;
            }
        }
        return Ok(());
    };
    for (key, child) in object {
        let lower = key.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "credential_ciphertext"
                | "authorization"
                | "api_key"
                | "apikey"
                | "password"
                | "private_key"
                | "access_token"
                | "refresh_token"
        ) && !child.is_null()
            && child
                .as_str()
                .is_some_and(|text| !text.is_empty() && text != "[REDACTED]" && text != "redacted")
        {
            return Err(OpsError::Validation(
                "control-plane export contains a plaintext credential field".into(),
            ));
        }
        if lower == "credential"
            && child
                .as_str()
                .is_some_and(|text| !text.is_empty() && text != "[REDACTED]" && text != "redacted")
        {
            return Err(OpsError::Validation(
                "control-plane export contains a plaintext credential field".into(),
            ));
        }
        reject_plaintext_secret_fields(child)?;
    }
    Ok(())
}

async fn clear_control_plane_tx(tx: &mut Transaction<'_, Postgres>) -> Result<(), OpsError> {
    // Delete children explicitly so this remains correct if a future schema
    // changes one of the cascade actions. Historical usage and unexportable
    // Virtual Keys are intentionally untouched by a control-plane restore.
    for statement in [
        "DELETE FROM routes",
        "DELETE FROM model_bindings",
        "DELETE FROM logical_models",
        "DELETE FROM source_model_capabilities",
        "DELETE FROM source_models",
        "DELETE FROM accounts",
        "DELETE FROM sources",
        "DELETE FROM providers",
    ] {
        sqlx::query(statement).execute(&mut **tx).await?;
    }
    Ok(())
}

async fn restore_provider_presets(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<(), OpsError> {
    for row in rows {
        sqlx::query(
            "INSERT INTO provider_presets SELECT (jsonb_populate_record(NULL::provider_presets,$1)).* ON CONFLICT (id,version) DO NOTHING",
        )
        .bind(sanitize_json(row.clone()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn restore_model_presets(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<(), OpsError> {
    for row in rows {
        sqlx::query(
            "INSERT INTO model_presets SELECT (jsonb_populate_record(NULL::model_presets,$1)).* ON CONFLICT (id,version) DO NOTHING",
        )
        .bind(sanitize_json(row.clone()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn restore_sources(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<(), OpsError> {
    for row in rows {
        sqlx::query(
            "INSERT INTO sources SELECT (jsonb_populate_record(NULL::sources,$1)).* ON CONFLICT (id) DO NOTHING",
        )
        .bind(sanitize_json(row.clone()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn restore_legacy_providers(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<(), OpsError> {
    for row in rows {
        sqlx::query(
            "INSERT INTO providers SELECT (jsonb_populate_record(NULL::providers,$1)).* ON CONFLICT (id) DO NOTHING",
        )
        .bind(sanitize_json(row.clone()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn restore_accounts(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<(), OpsError> {
    for row in rows {
        let mut row = sanitize_json(row.clone());
        if let Some(object) = row.as_object_mut() {
            object.remove("credential");
            // Never accept ciphertext from an imported payload, even if a
            // caller hand-edits the JSON after export.
            object.insert("credential_ciphertext".into(), Value::Null);
            if object.get("credential_env").is_some_and(Value::is_null) {
                object.remove("credential_env");
            }
        }
        sqlx::query(
            "INSERT INTO accounts SELECT (jsonb_populate_record(NULL::accounts,$1)).* ON CONFLICT (id) DO NOTHING",
        )
        .bind(row)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn restore_source_models(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<(), OpsError> {
    for row in rows {
        sqlx::query(
            "INSERT INTO source_models SELECT (jsonb_populate_record(NULL::source_models,$1)).* ON CONFLICT (source_id,upstream_model_id) DO NOTHING",
        )
        .bind(sanitize_json(row.clone()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn restore_capabilities(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<(), OpsError> {
    for row in rows {
        sqlx::query(
            "INSERT INTO source_model_capabilities SELECT (jsonb_populate_record(NULL::source_model_capabilities,$1)).* ON CONFLICT (source_id,upstream_model_id,protocol) DO NOTHING",
        )
        .bind(sanitize_json(row.clone()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn restore_logical_models(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<(), OpsError> {
    for row in rows {
        sqlx::query(
            "INSERT INTO logical_models SELECT (jsonb_populate_record(NULL::logical_models,$1)).* ON CONFLICT (id) DO NOTHING",
        )
        .bind(sanitize_json(row.clone()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn restore_bindings(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<(), OpsError> {
    for row in rows {
        sqlx::query(
            "INSERT INTO model_bindings SELECT (jsonb_populate_record(NULL::model_bindings,$1)).* ON CONFLICT (id) DO NOTHING",
        )
        .bind(sanitize_json(row.clone()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn restore_routes(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<(), OpsError> {
    for row in rows {
        sqlx::query(
            "INSERT INTO routes SELECT (jsonb_populate_record(NULL::routes,$1)).* ON CONFLICT (id) DO NOTHING",
        )
        .bind(sanitize_json(row.clone()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn restore_virtual_keys(
    _tx: &mut Transaction<'_, Postgres>,
    rows: &[Value],
) -> Result<i64, OpsError> {
    let mut skipped = 0;
    for row in rows {
        // Exports intentionally omit key_hash.  Keeping metadata in the file
        // is useful for inventory, but importing a hand-edited hash would make
        // the file an authentication credential, so Virtual Keys are always
        // skipped and must be re-issued on the target database.
        let _ = row;
        skipped += 1;
    }
    Ok(skipped)
}

async fn restore_snapshot_state(
    tx: &mut Transaction<'_, Postgres>,
    metadata: &RuntimeSnapshotMetadata,
) -> Result<(), OpsError> {
    if metadata.revision < 0 {
        return Err(OpsError::Validation(
            "runtime snapshot revision must not be negative".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO runtime_snapshot_state (singleton,revision,updated_at) VALUES (TRUE,$1,$2) ON CONFLICT (singleton) DO UPDATE SET revision=EXCLUDED.revision,updated_at=EXCLUDED.updated_at",
    )
    .bind(metadata.revision)
    .bind(metadata.generated_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn reset_sequences(tx: &mut Transaction<'_, Postgres>) -> Result<(), OpsError> {
    for (table, column) in [("model_bindings", "id"), ("virtual_keys", "id")] {
        let query = format!(
            "SELECT setval(pg_get_serial_sequence('{table}','{column}'), COALESCE((SELECT MAX({column}) FROM {table}),1), TRUE)"
        );
        sqlx::query(&query).execute(&mut **tx).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{control_plane::ControlPlane, domain::config::GatewayConfig};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn account_export_uses_a_secret_reference_and_never_ciphertext() {
        let mut row = json!({
            "id": "account-a",
            "credential_env": "UPSTREAM_KEY",
            "credential_ciphertext": "ciphertext-that-must-not-escape"
        });
        sanitize_account_row(&mut row);
        let serialized = row.to_string();
        assert!(!serialized.contains("ciphertext-that-must-not-escape"));
        assert_eq!(row["credential"]["kind"], "secret_ref");
        assert_eq!(row["credential"]["name"], "UPSTREAM_KEY");
        assert!(row.get("credential_ciphertext").is_none());
    }

    #[test]
    fn virtual_key_export_never_contains_authentication_or_recovery_material() {
        let mut row = json!({
            "id": 7,
            "name": "personal",
            "key_prefix": "mgk_public",
            "key_hash": "hash-that-must-not-escape",
            "key_ciphertext": "ciphertext-that-must-not-escape"
        });
        sanitize_virtual_key_row(&mut row);

        assert_eq!(row["key_prefix"], "mgk_public");
        assert!(row.get("key_hash").is_none());
        assert!(row.get("key_ciphertext").is_none());
    }

    #[test]
    fn sensitive_nested_source_values_are_redacted_but_model_limits_survive() {
        let sanitized = sanitize_json(json!({
            "auth": {"api_key": "secret", "authorization": "Bearer secret"},
            "headers": {"X-Key": "sk-1234567890123456"},
            "metadata": {"max_output_tokens": 4096, "context_window": 128000}
        }));
        assert_eq!(sanitized["auth"]["api_key"], "[REDACTED]");
        assert_eq!(sanitized["auth"]["authorization"], "[REDACTED]");
        assert_eq!(sanitized["headers"]["X-Key"], "[REDACTED]");
        assert_eq!(sanitized["metadata"]["max_output_tokens"], 4096);
        assert_eq!(
            sanitize_json(json!(
                "https://user:password@example.test/path?api_key=secret"
            )),
            json!("https://example.test/path")
        );
    }

    #[test]
    fn cleanup_defaults_are_operational_and_invalid_batches_are_rejected() {
        let request = CleanupRequest::default();
        assert_eq!(request.batch_size, DEFAULT_BATCH_SIZE);
        assert_eq!(request.max_batches, DEFAULT_MAX_BATCHES);
        assert!(validate_cleanup_request(&request).is_ok());
        let invalid = CleanupRequest {
            batch_size: 0,
            ..request
        };
        assert!(validate_cleanup_request(&invalid).is_err());
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL; run with the PostgreSQL regression suite"]
    async fn postgres_retention_is_independent_resumable_and_restore_verified() {
        let Some(url) = std::env::var("TEST_DATABASE_URL").ok() else {
            eprintln!("TEST_DATABASE_URL is not set; skipping ops PostgreSQL regression");
            return;
        };
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect PostgreSQL test admin database");
        let schema = format!("ops_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create ops test schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse TEST_DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect ops isolated schema");
        let database = Database::from_test_pool(pool.clone())
            .await
            .expect("migrate ops isolated schema");
        let repository = OpsRepository::from_database(&database);
        let control_plane = ControlPlane::new(database.pool().clone(), "127.0.0.1:0");
        crate::control_plane::model_catalog::install_builtin_presets(
            &crate::control_plane::model_catalog::ModelCatalogRepository::new(
                database.pool().clone(),
            ),
        )
        .await
        .expect("install built-in presets for export fixture");

        let policies = RETENTION_KEYS
            .iter()
            .map(|key| RetentionPolicyWrite {
                policy_key: (*key).into(),
                retention_days: 1,
                enabled: true,
            })
            .collect::<Vec<_>>();
        repository
            .update_retention_policies(&policies, "ops-test")
            .await
            .expect("set zero-day test policies");

        let old_request = format!("old-{}", Uuid::new_v4());
        let fresh_request = format!("fresh-{}", Uuid::new_v4());
        for (request_id, age) in [(&old_request, "2 days"), (&fresh_request, "0 seconds")] {
            sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success,created_at) VALUES ($1,'provider','account','model','model','source','test','openai_chat_completions','openai_chat_completions','native',200,TRUE,NOW()-$2::interval)")
                .bind(request_id)
                .bind(age)
                .execute(&pool)
                .await
                .expect("insert usage event fixture");
        }
        sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,source_id,account_id,status_code,success,created_at) VALUES ($1,0,'provider','source','account',200,TRUE,NOW()-INTERVAL '2 days')")
            .bind(&old_request)
            .execute(&pool)
            .await
            .expect("insert old attempt fixture");

        // Both existing audit domains are independently covered by the audit
        // policy; their rows are deliberately older than the cut-off.
        sqlx::query("INSERT INTO audit_logs (operation_id,action,status,actor,details,created_at) VALUES ('old-audit','test','progress','ops-test','{}',NOW()-INTERVAL '2 days')")
            .execute(&pool)
            .await
            .expect("insert old audit fixture");

        let dry = repository
            .start_cleanup(&CleanupRequest {
                dry_run: true,
                batch_size: 1,
                max_batches: 1,
                operation_id: Some(format!("dry-{}", Uuid::new_v4())),
                requested_by: Some("ops-test".into()),
                policy_keys: None,
            })
            .await
            .expect("dry-run cleanup");
        assert_eq!(dry.status, "completed");
        assert!(dry.progress["candidates"]["usage_events"]
            .as_i64()
            .is_some_and(|count| count >= 1));
        let old_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM usage_events WHERE request_id=$1")
                .bind(&old_request)
                .fetch_one(&pool)
                .await
                .expect("check dry-run fixture");
        assert_eq!(old_count, 1);

        let operation_id = format!("cleanup-{}", Uuid::new_v4());
        let request = CleanupRequest {
            dry_run: false,
            batch_size: 1,
            max_batches: 1,
            operation_id: Some(operation_id.clone()),
            requested_by: Some("ops-test".into()),
            policy_keys: None,
        };
        let mut run = repository
            .start_cleanup(&request)
            .await
            .expect("first bounded cleanup batch");
        assert!(matches!(run.status.as_str(), "running" | "completed"));
        for _ in 0..8 {
            if run.status != "running" {
                break;
            }
            run = repository
                .start_cleanup(&request)
                .await
                .expect("resume cleanup with same operation id");
        }
        assert_eq!(run.status, "completed");
        let old_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM usage_events WHERE request_id=$1")
                .bind(&old_request)
                .fetch_one(&pool)
                .await
                .expect("check cleaned event");
        let fresh_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM usage_events WHERE request_id=$1")
                .bind(&fresh_request)
                .fetch_one(&pool)
                .await
                .expect("check fresh event");
        assert_eq!(old_count, 0);
        assert_eq!(fresh_count, 1);
        let orphan_attempts: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM usage_event_attempts WHERE request_id=$1")
                .bind(&old_request)
                .fetch_one(&pool)
                .await
                .expect("check cleaned attempt");
        assert_eq!(orphan_attempts, 0);

        let config: GatewayConfig = serde_json::from_value(json!({
            "listen_addr":"127.0.0.1:0",
            "providers":[{"id":"restore-source","name":"Restore Source","base_url":"https://example.com","models":["restore-model"],"native_protocols":["openai_chat_completions"],"endpoints":{"openai_chat_completions":"/v1/chat/completions"},"capabilities":{"streaming":true,"usage":true}}],
            "accounts":[{"id":"restore-account","provider_id":"restore-source","display_name":"Restore Account","credential_env":"OPS_RESTORE_KEY"}],
            "routes":[{"id":"restore-route","model":"restore-model","provider_id":"restore-source","protocols":["openai_chat_completions"],"primary_account_id":"restore-account","mode":"native"}]
        }))
        .expect("build restore config");
        control_plane
            .initialize_from_config(&config, true)
            .await
            .expect("initialize restore fixture");
        sqlx::query("INSERT INTO source_connection_tests (source_id,account_id,protocol,upstream_protocol,mode,status,http_status,latency_ms,error_code,error_message,requested_by,tested_at) VALUES ('restore-source','restore-account','openai_chat_completions','openai_chat_completions','native','failed',503,10,'upstream_unavailable','fixed','ops-test',NOW()-INTERVAL '2 days')")
            .execute(&pool)
            .await
            .expect("insert old connection-test audit fixture");
        sqlx::query("INSERT INTO source_discovery_runs (source_id,account_id,provider_preset_id,provider_preset_version,status,raw_snapshot,diff,discovered_model_count,http_status,latency_ms,error_code,error_message,requested_by,started_at,completed_at) VALUES ('restore-source','restore-account','custom',1,'failed',NULL,'{\"added\":[],\"changed\":[],\"missing\":[]}',0,503,10,'discovery_failed','fixed','ops-test',NOW()-INTERVAL '2 days',NOW()-INTERVAL '2 days')")
            .execute(&pool)
            .await
            .expect("insert old discovery fixture");
        let history_cleanup = repository
            .start_cleanup(&CleanupRequest {
                batch_size: 10,
                max_batches: 10,
                operation_id: Some(format!("history-{}", Uuid::new_v4())),
                requested_by: Some("ops-test".into()),
                ..CleanupRequest::default()
            })
            .await
            .expect("clean old audit and discovery history");
        assert_eq!(history_cleanup.status, "completed");
        let old_connection_tests: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM source_connection_tests WHERE source_id='restore-source'",
        )
        .fetch_one(&pool)
        .await
        .expect("check old connection-test history");
        let old_discoveries: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM source_discovery_runs WHERE source_id='restore-source'",
        )
        .fetch_one(&pool)
        .await
        .expect("check old discovery history");
        assert_eq!(old_connection_tests, 0);
        assert_eq!(old_discoveries, 0);
        let exported = repository
            .export_control_plane(&control_plane, "ops-test")
            .await
            .expect("export control plane")
            .export;
        let exported_json = serde_json::to_string(&exported).expect("serialize export");
        assert!(exported_json.contains("OPS_RESTORE_KEY"));
        assert!(!exported_json.contains("credential_ciphertext"));
        assert!(!exported_json.contains("Authorization"));
        let mut invalid_export = exported.clone();
        invalid_export.runtime_snapshot.fingerprint = "0".repeat(64);
        assert!(matches!(
            repository
                .restore_control_plane(&control_plane, &invalid_export, true, "ops-test")
                .await,
            Err(OpsError::Snapshot(_))
        ));
        let source_after_failed_restore: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sources WHERE id='restore-source'")
                .fetch_one(&pool)
                .await
                .expect("check rollback after failed restore");
        assert_eq!(source_after_failed_restore, 1);
        let restored = repository
            .restore_control_plane(&control_plane, &exported, true, "ops-test")
            .await
            .expect("restore and verify control plane");
        assert!(restored.verified);
        assert_eq!(
            restored.snapshot.revision,
            exported.runtime_snapshot.revision
        );
        let credential_ciphertext: Option<String> = sqlx::query_scalar(
            "SELECT credential_ciphertext FROM accounts WHERE id='restore-account'",
        )
        .fetch_one(&pool)
        .await
        .expect("check restored credential ciphertext");
        assert!(credential_ciphertext.is_none());

        drop(database);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop ops test schema");
        admin.close().await;
    }
}
