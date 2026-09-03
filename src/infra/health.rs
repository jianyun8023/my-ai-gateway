use super::db::{AccountHealthRow, Database};
use crate::{
    control_plane::{
        model_catalog::{ConnectionTestRecord, ModelCatalogRepository},
        model_discovery::{DiscoveryServiceError, ModelDiscoveryService},
    },
    domain::protocol::Protocol,
    http::SourceHttpClient,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::sync::{Mutex, OwnedMutexGuard};

pub const DEFAULT_COOLDOWN: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_COOLDOWN: Duration = Duration::from_secs(30 * 60);
pub const DEFAULT_STALE_AFTER: Duration = Duration::from_secs(10 * 60);
pub const DEFAULT_PROBE_INTERVAL: Duration = Duration::from_secs(60);
pub const DEFAULT_FAILURE_THRESHOLD: u32 = 3;
pub const DEFAULT_FAILURE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthConfig {
    pub cooldown: Duration,
    pub max_cooldown: Duration,
    pub stale_after: Duration,
    pub probe_interval: Duration,
    pub failure_threshold: u32,
    pub failure_window: Duration,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            cooldown: DEFAULT_COOLDOWN,
            max_cooldown: DEFAULT_MAX_COOLDOWN,
            stale_after: DEFAULT_STALE_AFTER,
            probe_interval: DEFAULT_PROBE_INTERVAL,
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            failure_window: DEFAULT_FAILURE_WINDOW,
        }
    }
}

impl HealthConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            cooldown: env_duration_ms("GATEWAY_HEALTH_COOLDOWN_MS", defaults.cooldown),
            max_cooldown: env_duration_ms("GATEWAY_HEALTH_MAX_COOLDOWN_MS", defaults.max_cooldown),
            stale_after: env_duration_secs("GATEWAY_HEALTH_STALE_AFTER_SECS", defaults.stale_after),
            probe_interval: env_duration_secs(
                "GATEWAY_HEALTH_PROBE_INTERVAL_SECS",
                defaults.probe_interval,
            ),
            failure_threshold: env_u32(
                "GATEWAY_HEALTH_FAILURE_THRESHOLD",
                defaults.failure_threshold,
            ),
            failure_window: env_duration_secs(
                "GATEWAY_HEALTH_FAILURE_WINDOW_SECS",
                defaults.failure_window,
            ),
        }
    }

    fn normalized(self) -> Self {
        let cooldown = self.cooldown.max(Duration::from_millis(1));
        Self {
            cooldown,
            max_cooldown: self.max_cooldown.max(cooldown),
            stale_after: self.stale_after.max(Duration::from_millis(1)),
            probe_interval: self.probe_interval.max(Duration::from_millis(1)),
            failure_threshold: self.failure_threshold.max(1),
            failure_window: self.failure_window.max(Duration::from_millis(1)),
        }
    }
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}

fn env_duration_ms(name: &str, default: Duration) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(default)
}

fn env_duration_secs(name: &str, default: Duration) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(default)
}

pub trait HealthClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemHealthClock;

impl HealthClock for SystemHealthClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone)]
pub struct ManualHealthClock {
    now: Arc<RwLock<DateTime<Utc>>>,
}

#[allow(dead_code)]
impl ManualHealthClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: Arc::new(RwLock::new(now)),
        }
    }

    pub fn set(&self, now: DateTime<Utc>) {
        *self.now.write().expect("manual health clock lock") = now;
    }

    pub fn advance(&self, duration: Duration) {
        let duration =
            ChronoDuration::from_std(duration).unwrap_or_else(|_| ChronoDuration::zero());
        let mut now = self.now.write().expect("manual health clock lock");
        *now += duration;
    }
}

impl HealthClock for ManualHealthClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.read().expect("manual health clock lock")
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AccountHealth {
    pub available: bool,
    pub source_enabled: bool,
    pub consecutive_failures: u32,
    pub cooldown_remaining_ms: u64,
    pub status: String,
    pub source: String,
    pub stale: bool,
    pub updated_at: Option<DateTime<Utc>>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_probe_at: Option<DateTime<Utc>>,
    pub last_probe_status: Option<String>,
    pub last_probe_error: Option<String>,
}

#[derive(Clone, Debug)]
struct MemoryAccountState {
    status: String,
    source: String,
    cooldown_until: Option<DateTime<Utc>>,
    consecutive_failures: u32,
    failure_window_started_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
    last_success_at: Option<DateTime<Utc>>,
    last_probe_at: Option<DateTime<Utc>>,
    last_probe_status: Option<String>,
    last_probe_error: Option<String>,
    enabled: bool,
}

#[derive(Clone)]
pub struct HealthRegistry {
    config: HealthConfig,
    database: Option<Database>,
    state: Arc<Mutex<HashMap<String, MemoryAccountState>>>,
    probe_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    clock: Arc<dyn HealthClock>,
}

#[allow(dead_code)]
impl HealthRegistry {
    pub fn new(cooldown: Duration) -> Self {
        let mut config = HealthConfig::default();
        config.cooldown = cooldown;
        config.max_cooldown = config.max_cooldown.max(cooldown);
        Self::with_config_and_clock(config, Arc::new(SystemHealthClock))
    }

    pub fn with_config(config: HealthConfig) -> Self {
        Self::with_config_and_clock(config, Arc::new(SystemHealthClock))
    }

    pub fn with_database(database: Database, cooldown: Duration) -> Self {
        let mut config = HealthConfig::default();
        config.cooldown = cooldown;
        config.max_cooldown = config.max_cooldown.max(cooldown);
        Self::with_database_config(database, config)
    }

    pub fn with_database_config(database: Database, config: HealthConfig) -> Self {
        Self::with_database_config_and_clock(database, config, Arc::new(SystemHealthClock))
    }

    pub fn from_database(database: Database, config: HealthConfig) -> Self {
        Self::with_database_config(database, config)
    }

    pub fn with_clock<C>(cooldown: Duration, clock: C) -> Self
    where
        C: HealthClock + 'static,
    {
        let mut config = HealthConfig::default();
        config.cooldown = cooldown;
        config.max_cooldown = config.max_cooldown.max(cooldown);
        Self::with_config_and_clock(config, Arc::new(clock))
    }

    pub fn with_config_and_clock(config: HealthConfig, clock: Arc<dyn HealthClock>) -> Self {
        Self {
            config: config.normalized(),
            database: None,
            state: Arc::new(Mutex::new(HashMap::new())),
            probe_locks: Arc::new(Mutex::new(HashMap::new())),
            clock,
        }
    }

    pub fn with_database_config_and_clock(
        database: Database,
        config: HealthConfig,
        clock: Arc<dyn HealthClock>,
    ) -> Self {
        Self {
            config: config.normalized(),
            database: Some(database),
            state: Arc::new(Mutex::new(HashMap::new())),
            probe_locks: Arc::new(Mutex::new(HashMap::new())),
            clock,
        }
    }

    pub fn config(&self) -> HealthConfig {
        self.config
    }

    pub fn database(&self) -> Option<Database> {
        self.database.clone()
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    async fn acquire_probe_lock(&self, account_id: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.probe_locks.lock().await;
            locks
                .entry(account_id.to_owned())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }

    pub async fn restore(&self) -> Result<(), sqlx::Error> {
        if let Some(database) = &self.database {
            database.account_health_all().await.map(|_| ())
        } else {
            Ok(())
        }
    }

    pub async fn is_available(&self, account_id: &str) -> bool {
        if let Some(database) = &self.database {
            return match database.account_health(account_id).await {
                Ok(Some(row)) => {
                    health_from_row(&row, self.now(), self.config.stale_after).available
                }
                Ok(None) => false,
                Err(error) => {
                    tracing::warn!(account_id, %error, "failed to read persisted account health");
                    false
                }
            };
        }
        let state = self.state.lock().await;
        state
            .get(account_id)
            .map(|value| memory_health(value, self.now(), self.config.stale_after).available)
            .unwrap_or(true)
    }

    pub async fn get_health(&self, account_id: &str) -> AccountHealth {
        if let Some(database) = &self.database {
            return match database.account_health(account_id).await {
                Ok(Some(row)) => health_from_row(&row, self.now(), self.config.stale_after),
                Ok(None) => unknown_health(false, "unknown"),
                Err(error) => {
                    tracing::warn!(account_id, %error, "failed to read persisted account health");
                    unknown_health(false, "database")
                }
            };
        }
        let state = self.state.lock().await;
        state
            .get(account_id)
            .map(|value| memory_health(value, self.now(), self.config.stale_after))
            .unwrap_or_else(|| unknown_health(true, "unknown"))
    }

    pub async fn all_health(&self) -> HashMap<String, AccountHealth> {
        if let Some(database) = &self.database {
            return match database.account_health_all().await {
                Ok(rows) => rows
                    .iter()
                    .map(|row| {
                        (
                            row.account_id.clone(),
                            health_from_row(row, self.now(), self.config.stale_after),
                        )
                    })
                    .collect(),
                Err(error) => {
                    tracing::warn!(%error, "failed to read persisted account health rows");
                    HashMap::new()
                }
            };
        }
        let state = self.state.lock().await;
        state
            .iter()
            .map(|(id, value)| {
                (
                    id.clone(),
                    memory_health(value, self.now(), self.config.stale_after),
                )
            })
            .collect()
    }

    pub async fn mark_failure(&self, account_id: &str) {
        self.mark_failure_with_details(account_id, "passive", Some("upstream_failure"), None)
            .await;
    }

    pub async fn mark_failure_with_details(
        &self,
        account_id: &str,
        source: &str,
        error_code: Option<&str>,
        error_message: Option<&str>,
    ) -> bool {
        if let Some(database) = &self.database {
            return match database
                .record_account_health_failure(
                    account_id,
                    self.now(),
                    self.config.cooldown,
                    self.config.max_cooldown,
                    self.config.failure_threshold,
                    self.config.failure_window,
                    source,
                    error_code,
                    error_message,
                    None,
                    None,
                )
                .await
            {
                Ok((_, cooldown_started)) => cooldown_started,
                Err(error) => {
                    tracing::warn!(account_id, %error, "failed to persist account health failure");
                    false
                }
            };
        }
        self.mark_memory_failure(account_id, source, error_message)
            .await
    }

    async fn mark_probe_failure(
        &self,
        account_id: &str,
        connection_test_id: Option<i64>,
        error_code: Option<&str>,
        error_message: Option<&str>,
        latency_ms: Option<i64>,
    ) {
        if let Some(database) = &self.database {
            if let Err(error) = database
                .record_account_health_failure(
                    account_id,
                    self.now(),
                    self.config.cooldown,
                    self.config.max_cooldown,
                    self.config.failure_threshold,
                    self.config.failure_window,
                    "probe",
                    error_code,
                    error_message,
                    latency_ms,
                    connection_test_id,
                )
                .await
            {
                tracing::warn!(account_id, %error, "failed to persist probe health failure");
            }
            return;
        }
        self.mark_memory_failure(account_id, "probe", error_message)
            .await;
    }

    async fn mark_memory_failure(
        &self,
        account_id: &str,
        source: &str,
        error_message: Option<&str>,
    ) -> bool {
        let now = self.now();
        let mut state = self.state.lock().await;
        let entry = state
            .entry(account_id.to_owned())
            .or_insert_with(|| MemoryAccountState {
                status: "unknown".into(),
                source: "unknown".into(),
                cooldown_until: None,
                consecutive_failures: 0,
                failure_window_started_at: None,
                updated_at: None,
                last_error: None,
                last_success_at: None,
                last_probe_at: None,
                last_probe_status: None,
                last_probe_error: None,
                enabled: true,
            });
        let active_cooldown = entry.cooldown_until.is_some_and(|until| until > now);
        let mut cooldown_started = false;
        if !active_cooldown {
            let window_expired = entry.failure_window_started_at.is_none_or(|started_at| {
                now.signed_duration_since(started_at)
                    .to_std()
                    .map(|elapsed| elapsed > self.config.failure_window)
                    .unwrap_or(true)
            });
            if window_expired {
                entry.consecutive_failures = 1;
                entry.failure_window_started_at = Some(now);
            } else {
                entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
            }
            if entry.consecutive_failures >= self.config.failure_threshold {
                let breaker_failures =
                    entry.consecutive_failures - self.config.failure_threshold + 1;
                let delay = exponential_backoff(
                    self.config.cooldown,
                    self.config.max_cooldown,
                    breaker_failures,
                );
                entry.cooldown_until =
                    Some(now + ChronoDuration::from_std(delay).unwrap_or_default());
                cooldown_started = true;
            } else {
                entry.cooldown_until = None;
            }
        }
        entry.status = if entry.enabled && entry.cooldown_until.is_some_and(|until| until > now) {
            "cooling_down"
        } else if entry.enabled {
            "unhealthy"
        } else {
            "disabled"
        }
        .into();
        entry.source = normalize_source(source).into();
        entry.updated_at = Some(now);
        entry.last_error = sanitize_error(error_message);
        entry.last_success_at = None;
        if source == "probe" {
            entry.last_probe_at = Some(now);
            entry.last_probe_status = Some("failed".into());
            entry.last_probe_error = sanitize_error(error_message);
        }
        cooldown_started
    }

    pub async fn mark_success(&self, account_id: &str) {
        self.mark_success_with_source(account_id, "passive", None, None)
            .await;
    }

    pub async fn mark_success_with_source(
        &self,
        account_id: &str,
        source: &str,
        connection_test_id: Option<i64>,
        latency_ms: Option<i64>,
    ) {
        if let Some(database) = &self.database {
            if let Err(error) = database
                .record_account_health_success(
                    account_id,
                    self.now(),
                    source,
                    connection_test_id,
                    latency_ms,
                )
                .await
            {
                tracing::warn!(account_id, %error, "failed to persist account health success");
            }
            return;
        }
        let now = self.now();
        let mut state = self.state.lock().await;
        let entry = state
            .entry(account_id.to_owned())
            .or_insert_with(|| MemoryAccountState {
                status: "unknown".into(),
                source: "unknown".into(),
                cooldown_until: None,
                consecutive_failures: 0,
                failure_window_started_at: None,
                updated_at: None,
                last_error: None,
                last_success_at: None,
                last_probe_at: None,
                last_probe_status: None,
                last_probe_error: None,
                enabled: true,
            });
        entry.status = if entry.enabled { "healthy" } else { "disabled" }.into();
        entry.source = normalize_source(source).into();
        entry.cooldown_until = None;
        entry.consecutive_failures = 0;
        entry.failure_window_started_at = None;
        entry.updated_at = Some(now);
        entry.last_error = None;
        entry.last_success_at = Some(now);
        if source == "probe" {
            entry.last_probe_at = Some(now);
            entry.last_probe_status = Some("succeeded".into());
            entry.last_probe_error = None;
        }
    }

    /// Apply the result of the shared ProviderPreset connection-test service.
    /// Model discovery never calls this method.
    pub async fn apply_connection_test(&self, record: &ConnectionTestRecord) {
        let Some(account_id) = record.account_id.as_deref() else {
            return;
        };
        if record.status == "succeeded" {
            self.mark_success_with_source(
                account_id,
                "probe",
                Some(record.id),
                Some(record.latency_ms.max(0)),
            )
            .await;
        } else {
            self.mark_probe_failure(
                account_id,
                Some(record.id),
                record.error_code.as_deref(),
                record.error_message.as_deref(),
                Some(record.latency_ms.max(0)),
            )
            .await;
        }
    }

    /// Execute a ProviderPreset connection test for an enabled account. This
    /// deliberately does not invoke model discovery.
    pub async fn probe_account(
        &self,
        http: &SourceHttpClient,
        account_id: &str,
        protocol: Protocol,
        model: Option<&str>,
        requested_by: &str,
    ) -> Result<ProbeOutcome, ProbeError> {
        let _probe_guard = self.acquire_probe_lock(account_id).await;
        let database = self.database.clone().ok_or(ProbeError::DatabaseRequired)?;
        let source_id = database
            .account_source_id(account_id)
            .await
            .map_err(ProbeError::Database)?
            .ok_or_else(|| ProbeError::AccountNotFound(account_id.to_owned()))?;
        let service = ModelDiscoveryService::new(
            ModelCatalogRepository::new(database.pool().clone()),
            http.clone(),
        );
        let record = match service
            .test_connection(&source_id, account_id, protocol, model, requested_by)
            .await
        {
            Ok(record) => record,
            Err(error) => return Err(ProbeError::Discovery(error)),
        };
        self.apply_connection_test(&record).await;
        let health = self.get_health(account_id).await;
        Ok(ProbeOutcome {
            account_id: account_id.to_owned(),
            protocol,
            connection_test: record,
            health,
        })
    }
}

#[derive(Debug)]
pub enum ProbeError {
    DatabaseRequired,
    Database(sqlx::Error),
    Discovery(DiscoveryServiceError),
    AccountNotFound(String),
}

impl ProbeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::DatabaseRequired => "database_unavailable",
            Self::Database(_) => "database_error",
            Self::Discovery(error) => error.code(),
            Self::AccountNotFound(_) => "not_found",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::DatabaseRequired => "health probes require PostgreSQL".into(),
            Self::Database(error) => {
                let _ = error;
                "health probe database operation failed".into()
            }
            Self::Discovery(error) => error.public_message().into(),
            Self::AccountNotFound(account_id) => format!("account '{account_id}' not found"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeOutcome {
    pub account_id: String,
    pub protocol: Protocol,
    pub connection_test: ConnectionTestRecord,
    pub health: AccountHealth,
}

fn normalize_source(source: &str) -> &str {
    match source {
        "passive" | "probe" | "manual" | "startup" => source,
        _ => "unknown",
    }
}

fn sanitize_error(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let safe = matches!(
        value,
        "upstream request failed"
            | "retryable upstream response"
            | "upstream stream failed"
            | "upstream connection failed"
            | "upstream request timed out"
            | "account credential unavailable"
    );
    Some(if safe {
        value.to_owned()
    } else {
        "upstream request failed".into()
    })
}

fn exponential_backoff(base: Duration, maximum: Duration, failures: u32) -> Duration {
    if failures == 0 {
        return Duration::ZERO;
    }
    let base_ms = base.as_millis();
    let max_ms = maximum.as_millis().max(base_ms);
    let shift = failures.saturating_sub(1).min(63);
    let multiplier = 1u128.checked_shl(shift).unwrap_or(u128::MAX);
    let delay_ms = base_ms.saturating_mul(multiplier).min(max_ms);
    Duration::from_millis(u64::try_from(delay_ms).unwrap_or(u64::MAX))
}

fn stale(now: DateTime<Utc>, updated_at: Option<DateTime<Utc>>, stale_after: Duration) -> bool {
    let Some(updated_at) = updated_at else {
        return true;
    };
    let Ok(stale_after) = ChronoDuration::from_std(stale_after) else {
        return true;
    };
    now.signed_duration_since(updated_at) >= stale_after
}

fn cooldown_remaining(now: DateTime<Utc>, until: Option<DateTime<Utc>>) -> u64 {
    until
        .and_then(|until| until.signed_duration_since(now).to_std().ok())
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn health_from_row(
    row: &AccountHealthRow,
    now: DateTime<Utc>,
    stale_after: Duration,
) -> AccountHealth {
    let updated_at = row.health_updated_at.or(Some(row.account_updated_at));
    let is_stale = stale(now, updated_at, stale_after);
    let cooldown_active = row.cooldown_until.is_some_and(|until| until > now);
    let status = if !row.enabled || !row.source_enabled {
        "disabled"
    } else if is_stale {
        "stale"
    } else if cooldown_active {
        "cooling_down"
    } else {
        match row.health_status.as_str() {
            "healthy" => "healthy",
            "unhealthy" | "cooling_down" => "unhealthy",
            _ if row.consecutive_failures > 0 => "unhealthy",
            _ => "unknown",
        }
    };
    AccountHealth {
        // A stale persisted observation is advisory only. It cannot keep an
        // account out of routing forever, even if an old cooldown timestamp
        // lies in the future due to a clock change or interrupted shutdown.
        available: row.enabled
            && row.source_enabled
            && (!cooldown_active || is_stale)
            && status != "disabled",
        source_enabled: row.source_enabled,
        consecutive_failures: row.consecutive_failures.max(0) as u32,
        cooldown_remaining_ms: cooldown_remaining(now, row.cooldown_until),
        status: status.into(),
        source: normalize_source(&row.health_source).into(),
        stale: is_stale,
        updated_at,
        cooldown_until: row.cooldown_until,
        last_error: sanitize_error(row.last_error.as_deref()),
        last_success_at: row.last_success_at,
        last_probe_at: row.last_probe_at,
        last_probe_status: row.last_probe_status.clone(),
        last_probe_error: sanitize_error(row.last_probe_error.as_deref()),
    }
}

fn memory_health(
    state: &MemoryAccountState,
    now: DateTime<Utc>,
    stale_after: Duration,
) -> AccountHealth {
    let is_stale = stale(now, state.updated_at, stale_after);
    let cooldown_active = state.cooldown_until.is_some_and(|until| until > now);
    let status = if !state.enabled {
        "disabled"
    } else if is_stale {
        "stale"
    } else if cooldown_active {
        "cooling_down"
    } else {
        match state.status.as_str() {
            "healthy" => "healthy",
            "unhealthy" | "cooling_down" => "unhealthy",
            _ if state.consecutive_failures > 0 => "unhealthy",
            _ => "unknown",
        }
    };
    AccountHealth {
        available: state.enabled && (!cooldown_active || is_stale) && status != "disabled",
        source_enabled: true,
        consecutive_failures: state.consecutive_failures,
        cooldown_remaining_ms: cooldown_remaining(now, state.cooldown_until),
        status: status.into(),
        source: normalize_source(&state.source).into(),
        stale: is_stale,
        updated_at: state.updated_at,
        cooldown_until: state.cooldown_until,
        last_error: state.last_error.clone(),
        last_success_at: state.last_success_at,
        last_probe_at: state.last_probe_at,
        last_probe_status: state.last_probe_status.clone(),
        last_probe_error: state.last_probe_error.clone(),
    }
}

fn unknown_health(available: bool, source: &str) -> AccountHealth {
    AccountHealth {
        available,
        source_enabled: true,
        consecutive_failures: 0,
        cooldown_remaining_ms: 0,
        status: "unknown".into(),
        source: source.into(),
        stale: true,
        updated_at: None,
        cooldown_until: None,
        last_error: None,
        last_success_at: None,
        last_probe_at: None,
        last_probe_status: None,
        last_probe_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control_plane::model_catalog::{install_builtin_presets, ModelCatalogRepository},
        domain::{
            catalog::SourceInput,
            provider_preset::{builtin_provider_presets, ProviderPresetDefinition},
        },
        http,
    };
    use axum::{
        body::{to_bytes, Body},
        extract::Request,
        http::{header, HeaderMap, Response, StatusCode},
        Router,
    };
    use chrono::TimeZone;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::{
        str::FromStr,
        sync::{Arc, Mutex as StdMutex},
    };
    use tower::ServiceExt;

    fn clock() -> ManualHealthClock {
        ManualHealthClock::new(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap())
    }

    #[tokio::test]
    async fn passive_failures_use_exponential_backoff_and_recover() {
        let clock = clock();
        let registry = HealthRegistry::with_config_and_clock(
            HealthConfig {
                cooldown: Duration::from_secs(10),
                failure_threshold: 1,
                ..HealthConfig::default()
            },
            Arc::new(clock.clone()),
        );
        assert!(registry.is_available("a").await);
        registry.mark_failure("a").await;
        let first = registry.get_health("a").await;
        assert_eq!(first.status, "cooling_down");
        assert_eq!(first.cooldown_remaining_ms, 10_000);
        clock.advance(Duration::from_secs(10));
        assert!(registry.is_available("a").await);
        registry.mark_failure("a").await;
        assert_eq!(registry.get_health("a").await.cooldown_remaining_ms, 20_000);
        registry.mark_success("a").await;
        let recovered = registry.get_health("a").await;
        assert!(recovered.available);
        assert_eq!(recovered.consecutive_failures, 0);
        assert_eq!(recovered.status, "healthy");
    }

    #[tokio::test]
    async fn transient_failures_require_threshold_and_reset_outside_window() {
        let clock = clock();
        let registry = HealthRegistry::with_config_and_clock(
            HealthConfig {
                cooldown: Duration::from_secs(10),
                max_cooldown: Duration::from_secs(40),
                failure_threshold: 3,
                failure_window: Duration::from_secs(60),
                ..HealthConfig::default()
            },
            Arc::new(clock.clone()),
        );

        assert!(
            !registry
                .mark_failure_with_details("a", "passive", Some("upstream_http_429"), None)
                .await
        );
        assert!(
            !registry
                .mark_failure_with_details("a", "passive", Some("upstream_http_429"), None)
                .await
        );
        let suspect = registry.get_health("a").await;
        assert!(suspect.available);
        assert_eq!(suspect.status, "unhealthy");
        assert_eq!(suspect.consecutive_failures, 2);
        assert_eq!(suspect.cooldown_remaining_ms, 0);

        clock.advance(Duration::from_secs(61));
        assert!(
            !registry
                .mark_failure_with_details("a", "passive", Some("upstream_http_429"), None)
                .await
        );
        let reset = registry.get_health("a").await;
        assert!(reset.available);
        assert_eq!(reset.consecutive_failures, 1);
        assert_eq!(reset.cooldown_remaining_ms, 0);

        assert!(
            !registry
                .mark_failure_with_details("a", "passive", Some("upstream_http_429"), None)
                .await
        );
        assert!(
            registry
                .mark_failure_with_details("a", "passive", Some("upstream_http_429"), None)
                .await
        );
        let cooling = registry.get_health("a").await;
        assert!(!cooling.available);
        assert_eq!(cooling.consecutive_failures, 3);
        assert_eq!(cooling.cooldown_remaining_ms, 10_000);

        registry.mark_success("a").await;
        let recovered = registry.get_health("a").await;
        assert!(recovered.available);
        assert_eq!(recovered.consecutive_failures, 0);
    }

    #[tokio::test]
    async fn stale_is_orthogonal_and_does_not_permanently_block_after_expiry() {
        let clock = clock();
        let config = HealthConfig {
            cooldown: Duration::from_secs(5),
            max_cooldown: Duration::from_secs(20),
            stale_after: Duration::from_secs(3),
            failure_threshold: 1,
            ..HealthConfig::default()
        };
        let registry = HealthRegistry::with_config_and_clock(config, Arc::new(clock.clone()));
        registry.mark_failure("a").await;
        registry.mark_failure("a").await;
        clock.advance(Duration::from_secs(6));
        let health = registry.get_health("a").await;
        assert!(health.stale);
        assert_eq!(health.status, "stale");
        assert!(health.available);
        assert_eq!(health.cooldown_remaining_ms, 0);
    }

    #[test]
    fn exponential_backoff_is_capped() {
        assert_eq!(
            exponential_backoff(Duration::from_secs(1), Duration::from_secs(5), 1),
            Duration::from_secs(1)
        );
        assert_eq!(
            exponential_backoff(Duration::from_secs(1), Duration::from_secs(5), 3),
            Duration::from_secs(4)
        );
        assert_eq!(
            exponential_backoff(Duration::from_secs(1), Duration::from_secs(5), 4),
            Duration::from_secs(5)
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL and runs against an isolated PostgreSQL schema"]
    async fn postgres_health_survives_restart_concurrency_expiry_and_manual_toggle() {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for PostgreSQL health regression");
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect PostgreSQL health admin pool");
        let schema = format!("health_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create isolated health schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse TEST_DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .expect("connect isolated health schema");
        let database = Database::from_test_pool(pool.clone())
            .await
            .expect("migrate isolated health schema");
        let versions: (i32, i32) = sqlx::query_as(
            "SELECT schema_version,migration_version FROM gateway_schema_metadata WHERE singleton=TRUE",
        )
        .fetch_one(&pool)
        .await
        .expect("read schema metadata");
        assert_eq!(
            versions,
            (
                crate::infra::ops::CURRENT_SCHEMA_VERSION,
                crate::infra::ops::CURRENT_MIGRATION_VERSION,
            )
        );
        let migration_name: String =
            sqlx::query_scalar("SELECT name FROM gateway_schema_migrations WHERE version=12")
                .fetch_one(&pool)
                .await
                .expect("read health migration metadata");
        assert_eq!(migration_name, "health_persistence");
        let failure_window_migration: String =
            sqlx::query_scalar("SELECT name FROM gateway_schema_migrations WHERE version=22")
                .fetch_one(&pool)
                .await
                .expect("read failure-window migration metadata");
        assert_eq!(failure_window_migration, "health_failure_window");
        sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities) VALUES ('health-source','Health Source','custom',1,'{}'::jsonb,'https://health.example','{\"openai_chat_completions\":\"/chat/completions\"}'::jsonb,'{}'::jsonb,'{}'::jsonb)")
            .execute(&pool)
            .await
            .expect("seed health source");
        sqlx::query("INSERT INTO accounts (id,source_id,display_name,enabled,weight) VALUES ('health-account','health-source','Health Account',TRUE,100)")
            .execute(&pool)
            .await
            .expect("seed health account");

        // Simulate an upgrade from v21: legacy counters are reset exactly once,
        // while ordinary startup replays must preserve current runtime state.
        sqlx::query("DELETE FROM gateway_schema_migrations WHERE version=22")
            .execute(&pool)
            .await
            .expect("rewind failure-window migration marker");
        sqlx::query("UPDATE gateway_schema_metadata SET schema_version=21,migration_version=21 WHERE singleton=TRUE")
            .execute(&pool)
            .await
            .expect("rewind schema metadata");
        sqlx::query("UPDATE accounts SET health_status='cooling_down',consecutive_failures=9,cooldown_until=NOW()+INTERVAL '30 minutes' WHERE id='health-account'")
            .execute(&pool)
            .await
            .expect("seed legacy cooldown state");
        sqlx::raw_sql(include_str!(
            "../../migrations/0022_health_failure_window.sql"
        ))
        .execute(&pool)
        .await
        .expect("apply failure-window migration upgrade");
        let upgraded: (i32, Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT consecutive_failures,cooldown_until,failure_window_started_at FROM accounts WHERE id='health-account'",
        )
        .fetch_one(&pool)
        .await
        .expect("read upgraded health state");
        assert_eq!(upgraded, (0, None, None));

        sqlx::query("UPDATE accounts SET health_status='cooling_down',consecutive_failures=7,cooldown_until=NOW()+INTERVAL '30 minutes' WHERE id='health-account'")
            .execute(&pool)
            .await
            .expect("seed post-migration cooldown state");
        sqlx::raw_sql(include_str!(
            "../../migrations/0022_health_failure_window.sql"
        ))
        .execute(&pool)
        .await
        .expect("replay failure-window migration");
        let replayed: (i32, bool) = sqlx::query_as(
            "SELECT consecutive_failures,cooldown_until IS NOT NULL FROM accounts WHERE id='health-account'",
        )
        .fetch_one(&pool)
        .await
        .expect("read replayed health state");
        assert_eq!(replayed, (7, true));
        sqlx::query("UPDATE accounts SET health_status='unknown',consecutive_failures=0,cooldown_until=NULL WHERE id='health-account'")
            .execute(&pool)
            .await
            .expect("reset health state for transition assertions");

        let clock = clock();
        let config = HealthConfig {
            cooldown: Duration::from_secs(1),
            max_cooldown: Duration::from_secs(8),
            stale_after: Duration::from_secs(3),
            probe_interval: Duration::from_secs(60),
            failure_threshold: 3,
            failure_window: Duration::from_secs(60),
        };
        let registry = HealthRegistry::with_database_config_and_clock(
            database.clone(),
            config,
            Arc::new(clock.clone()),
        );
        let mut tasks = Vec::new();
        for _ in 0..5 {
            let registry = registry.clone();
            tasks.push(tokio::spawn(async move {
                registry
                    .mark_failure_with_details(
                        "health-account",
                        "passive",
                        Some("upstream_http_503"),
                        Some("retryable upstream response"),
                    )
                    .await;
            }));
        }
        for task in tasks {
            task.await.expect("concurrent health transition");
        }
        let persisted: (i32, String, DateTime<Utc>) = sqlx::query_as(
            "SELECT consecutive_failures,health_source,cooldown_until FROM accounts WHERE id='health-account'",
        )
        .fetch_one(&pool)
        .await
        .expect("read persisted health state");
        assert_eq!(persisted.0, 3);
        assert_eq!(persisted.1, "passive");
        assert_eq!(
            persisted.2,
            clock.now() + ChronoDuration::from_std(Duration::from_secs(1)).unwrap()
        );

        // A new registry instance reads the same row, proving restart recovery.
        let restarted = HealthRegistry::with_database_config_and_clock(
            database.clone(),
            config,
            Arc::new(clock.clone()),
        );
        let cooling = restarted.get_health("health-account").await;
        assert!(!cooling.available);
        assert_eq!(cooling.status, "cooling_down");
        assert_eq!(cooling.source, "passive");

        clock.advance(Duration::from_secs(3));
        let stale = restarted.get_health("health-account").await;
        assert!(stale.available);
        assert!(stale.stale);
        assert_eq!(stale.cooldown_remaining_ms, 0);
        clock.advance(Duration::from_secs(2));
        let expired = restarted.get_health("health-account").await;
        assert!(expired.available);
        assert!(expired.stale);
        restarted.mark_success("health-account").await;
        let recovered = restarted.get_health("health-account").await;
        assert!(recovered.available);
        assert_eq!(recovered.status, "healthy");
        assert_eq!(recovered.consecutive_failures, 0);

        let control_plane =
            crate::control_plane::ControlPlane::new(database.pool().clone(), "127.0.0.1:0");
        control_plane
            .set_account_enabled("health-account", false)
            .await
            .expect("manual disable account");
        let disabled = restarted.get_health("health-account").await;
        assert!(!disabled.available);
        assert_eq!(disabled.status, "disabled");
        assert_eq!(disabled.source, "manual");
        control_plane
            .set_account_enabled("health-account", true)
            .await
            .expect("manual enable account");
        let enabled = restarted.get_health("health-account").await;
        assert!(enabled.available);
        assert_eq!(enabled.status, "unknown");
        assert_eq!(enabled.source, "manual");

        control_plane
            .set_source_enabled("health-source", false)
            .await
            .expect("manual disable source");
        let source_disabled = restarted.get_health("health-account").await;
        assert!(!source_disabled.available);
        assert_eq!(source_disabled.status, "disabled");
        assert!(!source_disabled.source_enabled);
        assert_eq!(source_disabled.source, "manual");
        control_plane
            .set_source_enabled("health-source", true)
            .await
            .expect("manual enable source");
        let source_enabled = restarted.get_health("health-account").await;
        assert!(source_enabled.source_enabled);
        assert!(source_enabled.available);

        let events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM account_health_events WHERE account_id='health-account'",
        )
        .fetch_one(&pool)
        .await
        .expect("count health events");
        assert!(events >= 5);

        drop(database);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop isolated health schema");
        admin.close().await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL and runs against an isolated PostgreSQL schema"]
    async fn postgres_probe_reuses_provider_preset_connection_test_without_discovery() {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for PostgreSQL probe regression");
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect PostgreSQL probe admin pool");
        let schema = format!("probe_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create isolated probe schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse TEST_DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .expect("connect isolated probe schema");
        let database = Database::from_test_pool(pool.clone())
            .await
            .expect("migrate isolated probe schema");
        install_builtin_presets(&ModelCatalogRepository::new(database.pool().clone()))
            .await
            .expect("install provider presets");

        let requests = Arc::new(StdMutex::new(Vec::<(String, HeaderMap, Vec<u8>)>::new()));
        let requests_for_server = requests.clone();
        let app = Router::new().fallback(move |request: Request| {
            let requests = requests_for_server.clone();
            async move {
                let (parts, body) = request.into_parts();
                let body = to_bytes(body, 1024 * 1024).await.expect("read probe body");
                requests.lock().expect("probe request lock").push((
                    parts.uri.path().to_owned(),
                    parts.headers,
                    body.to_vec(),
                ));
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{}"#))
                    .expect("probe response")
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind probe upstream");
        let address = listener.local_addr().expect("probe upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve probe upstream");
        });
        let base_url = format!("http://{address}");
        let preset = builtin_provider_presets()
            .expect("built-in provider presets")
            .into_iter()
            .find(|preset| preset.id == "deepseek")
            .expect("DeepSeek preset");
        let definition: ProviderPresetDefinition =
            serde_json::from_value(preset.definition.clone()).expect("preset definition");
        let endpoints = definition
            .protocols
            .iter()
            .map(|(protocol, value)| (*protocol, value.endpoint.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        ModelCatalogRepository::new(database.pool().clone())
            .create_source(&SourceInput {
                id: "probe-source".into(),
                display_name: "Probe Source".into(),
                provider_preset_id: preset.id,
                provider_preset_version: preset.version,
                base_url,
                endpoints: serde_json::to_value(endpoints).expect("probe endpoints"),
                auth_config: definition.auth_snapshot(),
                protocol_capabilities: definition.protocol_capabilities_snapshot(),
            })
            .await
            .expect("create probe source");
        sqlx::query("INSERT INTO accounts (id,source_id,display_name,credential_env,enabled,weight) VALUES ('probe-account','probe-source','Probe Account','HEALTH_PROBE_TEST_KEY',TRUE,100)")
            .execute(&pool)
            .await
            .expect("create probe account");

        let _environment_lock = crate::state::ENV_LOCK.lock().await;
        let previous = std::env::var_os("HEALTH_PROBE_TEST_KEY");
        std::env::set_var("HEALTH_PROBE_TEST_KEY", "probe-secret");
        let registry = HealthRegistry::with_database_config(
            database.clone(),
            HealthConfig {
                cooldown: Duration::from_secs(1),
                ..HealthConfig::default()
            },
        );
        let empty_config = crate::domain::config::GatewayConfig {
            listen_addr: "127.0.0.1:0".into(),
            providers: Vec::new(),
            accounts: Vec::new(),
            routes: Vec::new(),
        };
        let app_state = crate::state::AppState {
            live: Arc::new(std::sync::RwLock::new(crate::state::LiveConfig::legacy(
                Arc::new(empty_config),
            ))),
            http: http::test_client().expect("probe HTTP client"),
            db: Some(database.clone()),
            control_plane: None,
            health: registry.clone(),
            admin_auth: crate::state::AdminAuth::test(),
            secrets: crate::infra::secrets::SecretResolver::empty(),
            prometheus_handle: crate::infra::observability::prometheus_handle(),
        };
        let response = crate::application(app_state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/accounts/probe-account/probe")
                    .header(header::AUTHORIZATION, "Bearer test-admin-key")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"protocol":"openai_chat_completions","requested_by":"probe-test"}"#,
                    ))
                    .expect("probe HTTP request"),
            )
            .await
            .expect("successful provider preset probe HTTP response");
        assert_eq!(response.status(), StatusCode::OK);
        let response_body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read probe HTTP response");
        let response_body: serde_json::Value =
            serde_json::from_slice(&response_body).expect("probe HTTP JSON");
        let outcome = &response_body["data"];
        if let Some(value) = previous {
            std::env::set_var("HEALTH_PROBE_TEST_KEY", value);
        } else {
            std::env::remove_var("HEALTH_PROBE_TEST_KEY");
        }
        assert_eq!(outcome["connection_test"]["status"], "succeeded");
        assert_eq!(outcome["health"]["source"], "probe");
        assert_eq!(outcome["health"]["status"], "healthy");
        {
            let requests = requests.lock().expect("probe requests lock");
            assert_eq!(requests.len(), 1);
            assert!(requests[0].0.ends_with("/chat/completions"));
            assert_eq!(
                requests[0]
                    .1
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok()),
                Some("Bearer probe-secret")
            );
            let body = String::from_utf8_lossy(&requests[0].2);
            assert!(!body.contains("probe-secret"));
        }
        let discovery_runs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM source_discovery_runs WHERE source_id='probe-source'",
        )
        .fetch_one(&pool)
        .await
        .expect("count discovery runs");
        assert_eq!(discovery_runs, 0);

        drop(_environment_lock);
        drop(database);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop isolated probe schema");
        admin.close().await;
    }
}
