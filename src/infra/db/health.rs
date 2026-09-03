use super::*;
use crate::domain::protocol::Protocol;

const HEALTH_SELECT_ONE: &str = "SELECT a.id AS account_id,a.enabled,s.enabled AS source_enabled,a.health_status,a.health_source,a.cooldown_until,a.consecutive_failures,a.failure_window_started_at,a.last_error,a.last_success_at,a.health_updated_at,a.last_probe_at,a.last_probe_status,a.last_probe_error,a.updated_at AS account_updated_at FROM accounts a JOIN sources s ON s.id=a.source_id WHERE a.id=$1";
const HEALTH_SELECT_ONE_FOR_UPDATE: &str = "SELECT a.id AS account_id,a.enabled,s.enabled AS source_enabled,a.health_status,a.health_source,a.cooldown_until,a.consecutive_failures,a.failure_window_started_at,a.last_error,a.last_success_at,a.health_updated_at,a.last_probe_at,a.last_probe_status,a.last_probe_error,a.updated_at AS account_updated_at FROM accounts a JOIN sources s ON s.id=a.source_id WHERE a.id=$1 FOR UPDATE OF a";
const HEALTH_SELECT_ALL: &str = "SELECT a.id AS account_id,a.enabled,s.enabled AS source_enabled,a.health_status,a.health_source,a.cooldown_until,a.consecutive_failures,a.failure_window_started_at,a.last_error,a.last_success_at,a.health_updated_at,a.last_probe_at,a.last_probe_status,a.last_probe_error,a.updated_at AS account_updated_at FROM accounts a JOIN sources s ON s.id=a.source_id ORDER BY a.id";

fn normalize_health_source(source: &str) -> &str {
    match source {
        "passive" | "probe" | "manual" | "startup" => source,
        _ => "unknown",
    }
}

fn sanitize_health_error(value: Option<&str>) -> Option<String> {
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

fn sanitize_health_code(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Some(value.to_owned())
    } else {
        Some("upstream_failure".into())
    }
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

impl Database {
    /// Read one persisted account health row. Routing uses this on every
    /// selection so a process restart cannot lose the cooldown state.
    pub async fn account_health(
        &self,
        account_id: &str,
    ) -> Result<Option<AccountHealthRow>, sqlx::Error> {
        sqlx::query_as::<_, AccountHealthRow>(HEALTH_SELECT_ONE)
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn account_health_all(&self) -> Result<Vec<AccountHealthRow>, sqlx::Error> {
        sqlx::query_as::<_, AccountHealthRow>(HEALTH_SELECT_ALL)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn account_source_id(&self, account_id: &str) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT source_id FROM accounts WHERE id=$1")
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await
    }

    /// Return one deterministic enabled protocol per account for periodic
    /// probes. The explicit Admin endpoint can choose another protocol.
    pub async fn health_probe_targets(
        &self,
    ) -> Result<Vec<(String, String, Protocol)>, sqlx::Error> {
        let rows: Vec<(String, String, Value, Value)> = sqlx::query_as(
            "SELECT a.id,a.source_id,s.endpoints,s.protocol_capabilities FROM accounts a JOIN sources s ON s.id=a.source_id WHERE a.enabled AND s.enabled AND s.provider_preset_id <> 'custom' ORDER BY a.id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut targets = Vec::with_capacity(rows.len());
        for (account_id, source_id, endpoints, capabilities) in rows {
            let endpoints =
                serde_json::from_value::<std::collections::BTreeMap<Protocol, String>>(endpoints)
                    .unwrap_or_default();
            let capabilities =
                serde_json::from_value::<std::collections::BTreeMap<Protocol, Value>>(capabilities)
                    .unwrap_or_default();
            let protocol = Protocol::ALL.into_iter().find(|protocol| {
                let has_endpoint = endpoints
                    .get(protocol)
                    .is_some_and(|endpoint| !endpoint.trim().is_empty());
                let supported = capabilities
                    .get(protocol)
                    .and_then(|value| value.get("mode").or(Some(value)))
                    .and_then(Value::as_str)
                    .is_none_or(|mode| !matches!(mode, "unknown" | "unsupported"));
                has_endpoint && supported
            });
            if let Some(protocol) = protocol {
                targets.push((account_id, source_id, protocol));
            }
        }
        Ok(targets)
    }

    pub async fn health_probe_protocol(
        &self,
        account_id: &str,
    ) -> Result<Option<Protocol>, sqlx::Error> {
        Ok(self
            .health_probe_targets()
            .await?
            .into_iter()
            .find(|target| target.0 == account_id)
            .map(|target| target.2))
    }

    /// Atomically increment a failure counter and calculate the next
    /// exponential cooldown. `SELECT ... FOR UPDATE` serializes concurrent
    /// request/probe transitions for the same account.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_account_health_failure(
        &self,
        account_id: &str,
        observed_at: DateTime<Utc>,
        base_cooldown: Duration,
        max_cooldown: Duration,
        failure_threshold: u32,
        failure_window: Duration,
        source: &str,
        error_code: Option<&str>,
        error_message: Option<&str>,
        latency_ms: Option<i64>,
        connection_test_id: Option<i64>,
    ) -> Result<(AccountHealthRow, bool), sqlx::Error> {
        let source = normalize_health_source(source);
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query_as::<_, AccountHealthRow>(HEALTH_SELECT_ONE_FOR_UPDATE)
            .bind(account_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        if !current.enabled || !current.source_enabled {
            tx.commit().await?;
            return Ok((current, false));
        }
        let active_cooldown = current
            .cooldown_until
            .is_some_and(|until| until > observed_at);
        let window_expired = current.failure_window_started_at.is_none_or(|started_at| {
            observed_at
                .signed_duration_since(started_at)
                .to_std()
                .map(|elapsed| elapsed > failure_window)
                .unwrap_or(true)
        });
        let (failures, failure_window_started_at, cooldown_until, status) = if active_cooldown {
            (
                current.consecutive_failures.max(0) as u32,
                current.failure_window_started_at,
                current.cooldown_until,
                "cooling_down",
            )
        } else {
            let failures = if window_expired {
                1
            } else {
                current.consecutive_failures.max(0) as u32 + 1
            };
            let failure_window_started_at = if window_expired {
                Some(observed_at)
            } else {
                current.failure_window_started_at
            };
            if failures >= failure_threshold {
                let breaker_failures = failures - failure_threshold + 1;
                let delay = exponential_backoff(base_cooldown, max_cooldown, breaker_failures);
                let until = observed_at
                    + chrono::Duration::from_std(delay)
                        .unwrap_or_else(|_| chrono::Duration::zero());
                (
                    failures,
                    failure_window_started_at,
                    Some(until),
                    "cooling_down",
                )
            } else {
                (failures, failure_window_started_at, None, "unhealthy")
            }
        };
        sqlx::query(
            "UPDATE accounts SET health_status=$2,cooldown_until=$3,last_error=$4,last_success_at=NULL,health_source=$5,health_updated_at=$6,consecutive_failures=$7,failure_window_started_at=$8,last_probe_at=CASE WHEN $5='probe' THEN $6 ELSE last_probe_at END,last_probe_status=CASE WHEN $5='probe' THEN 'failed' ELSE last_probe_status END,last_probe_error=CASE WHEN $5='probe' THEN $4 ELSE last_probe_error END WHERE id=$1",
        )
        .bind(account_id)
        .bind(status)
        .bind(cooldown_until)
        .bind(sanitize_health_error(error_message))
        .bind(source)
        .bind(observed_at)
        .bind(i32::try_from(failures).unwrap_or(i32::MAX))
        .bind(failure_window_started_at)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO account_health_events (account_id,status,source,observed_at,cooldown_until,consecutive_failures,error_code,error_message,connection_test_id,latency_ms) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(account_id)
        .bind(status)
        .bind(source)
        .bind(observed_at)
        .bind(cooldown_until)
        .bind(i32::try_from(failures).unwrap_or(i32::MAX))
        .bind(sanitize_health_code(error_code))
        .bind(sanitize_health_error(error_message))
        .bind(connection_test_id)
        .bind(latency_ms)
        .execute(&mut *tx)
        .await?;
        let cooldown_started = !active_cooldown && cooldown_until.is_some();
        tx.commit().await?;
        let health = self
            .account_health(account_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        Ok((health, cooldown_started))
    }

    pub async fn record_account_health_success(
        &self,
        account_id: &str,
        observed_at: DateTime<Utc>,
        source: &str,
        connection_test_id: Option<i64>,
        latency_ms: Option<i64>,
    ) -> Result<AccountHealthRow, sqlx::Error> {
        let source = normalize_health_source(source);
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query_as::<_, AccountHealthRow>(HEALTH_SELECT_ONE_FOR_UPDATE)
            .bind(account_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        if !current.enabled || !current.source_enabled {
            tx.commit().await?;
            return Ok(current);
        }
        let status = if current.enabled {
            "healthy"
        } else {
            "disabled"
        };
        sqlx::query(
            "UPDATE accounts SET health_status=$2,cooldown_until=NULL,last_error=NULL,last_success_at=$3,health_source=$4,health_updated_at=$3,consecutive_failures=0,failure_window_started_at=NULL,last_probe_at=CASE WHEN $4='probe' THEN $3 ELSE last_probe_at END,last_probe_status=CASE WHEN $4='probe' THEN 'succeeded' ELSE last_probe_status END,last_probe_error=CASE WHEN $4='probe' THEN NULL ELSE last_probe_error END WHERE id=$1",
        )
        .bind(account_id)
        .bind(status)
        .bind(observed_at)
        .bind(source)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO account_health_events (account_id,status,source,observed_at,cooldown_until,consecutive_failures,connection_test_id,latency_ms) VALUES ($1,$2,$3,$4,NULL,0,$5,$6)",
        )
        .bind(account_id)
        .bind(status)
        .bind(source)
        .bind(observed_at)
        .bind(connection_test_id)
        .bind(latency_ms)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.account_health(account_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

}
