use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct HealthRegistry {
    cooldown: Duration,
    failures: Arc<Mutex<HashMap<String, Instant>>>,
}

impl HealthRegistry {
    pub fn new(cooldown: Duration) -> Self {
        Self {
            cooldown,
            failures: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub async fn is_available(&self, account_id: &str) -> bool {
        let mut failures = self.failures.lock().await;
        match failures.get(account_id).copied() {
            Some(until) if until > Instant::now() => false,
            Some(_) => {
                failures.remove(account_id);
                true
            }
            None => true,
        }
    }
    pub async fn mark_failure(&self, account_id: &str) {
        self.failures
            .lock()
            .await
            .insert(account_id.to_owned(), Instant::now() + self.cooldown);
    }
    pub async fn mark_success(&self, account_id: &str) {
        self.failures.lock().await.remove(account_id);
    }
}
