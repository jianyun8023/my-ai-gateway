use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone, Debug, serde::Serialize)]
pub struct AccountHealth {
    pub available: bool,
    pub consecutive_failures: u32,
    pub cooldown_remaining_ms: u64,
}

#[derive(Clone)]
struct AccountState {
    cooldown_until: Instant,
    consecutive_failures: u32,
}

#[derive(Clone)]
pub struct HealthRegistry {
    cooldown: Duration,
    state: Arc<Mutex<HashMap<String, AccountState>>>,
}

impl HealthRegistry {
    pub fn new(cooldown: Duration) -> Self {
        Self {
            cooldown,
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn is_available(&self, account_id: &str) -> bool {
        let mut state = self.state.lock().await;
        match state.get(account_id) {
            Some(s) if s.cooldown_until > Instant::now() => false,
            Some(s) if s.consecutive_failures == 0 => true,
            Some(_) => {
                if let Some(s) = state.get_mut(account_id) {
                    s.cooldown_until = Instant::now();
                }
                true
            }
            None => true,
        }
    }

    pub async fn mark_failure(&self, account_id: &str) {
        let mut state = self.state.lock().await;
        let entry = state
            .entry(account_id.to_owned())
            .or_insert_with(|| AccountState {
                cooldown_until: Instant::now(),
                consecutive_failures: 0,
            });
        entry.consecutive_failures += 1;
        let backoff = self.cooldown * entry.consecutive_failures.min(5);
        entry.cooldown_until = Instant::now() + backoff;
    }

    pub async fn mark_success(&self, account_id: &str) {
        self.state.lock().await.remove(account_id);
    }

    #[allow(dead_code)]
    pub async fn get_health(&self, account_id: &str) -> AccountHealth {
        let state = self.state.lock().await;
        match state.get(account_id) {
            Some(s) => {
                let now = Instant::now();
                let remaining = if s.cooldown_until > now {
                    (s.cooldown_until - now).as_millis() as u64
                } else {
                    0
                };
                AccountHealth {
                    available: s.cooldown_until <= now,
                    consecutive_failures: s.consecutive_failures,
                    cooldown_remaining_ms: remaining,
                }
            }
            None => AccountHealth {
                available: true,
                consecutive_failures: 0,
                cooldown_remaining_ms: 0,
            },
        }
    }

    pub async fn all_health(&self) -> HashMap<String, AccountHealth> {
        let state = self.state.lock().await;
        let now = Instant::now();
        state
            .iter()
            .map(|(id, s)| {
                let remaining = if s.cooldown_until > now {
                    (s.cooldown_until - now).as_millis() as u64
                } else {
                    0
                };
                (
                    id.clone(),
                    AccountHealth {
                        available: s.cooldown_until <= now,
                        consecutive_failures: s.consecutive_failures,
                        cooldown_remaining_ms: remaining,
                    },
                )
            })
            .collect()
    }
}
