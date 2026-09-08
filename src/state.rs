//! Shared application state and atomic publication of validated snapshots.
use std::sync::Arc;

use metrics_exporter_prometheus::PrometheusHandle;

use crate::{
    auth::AdminAuth,
    control_plane,
    domain::{catalog::PublishedModel, config::GatewayConfig, routing::RouteResolver},
    http,
    infra::{db, health, observability, secrets},
};

#[derive(Clone)]
pub(crate) struct LiveConfig {
    pub(crate) config: Arc<GatewayConfig>,
    pub(crate) resolver: RouteResolver,
    pub(crate) models: Arc<Vec<PublishedModel>>,
    pub(crate) revision: i64,
    pub(crate) generated_at: chrono::DateTime<chrono::Utc>,
}

impl LiveConfig {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn legacy(config: Arc<GatewayConfig>) -> Self {
        let account_ids = config
            .accounts
            .iter()
            .filter(|account| account.enabled)
            .map(|account| account.id.clone())
            .collect::<Vec<_>>();
        let models = config
            .models()
            .into_iter()
            .map(|id| PublishedModel {
                display_name: id.clone(),
                id,
                account_ids: account_ids.clone(),
            })
            .collect();
        Self {
            resolver: RouteResolver::new(config.clone()),
            config,
            models: Arc::new(models),
            revision: 0,
            generated_at: chrono::Utc::now(),
        }
    }

    pub(crate) fn from_snapshot(snapshot: control_plane::RuntimeSnapshot) -> Self {
        Self {
            config: snapshot.config,
            resolver: snapshot.resolver,
            models: snapshot.models,
            revision: snapshot.revision,
            generated_at: snapshot.generated_at,
        }
    }
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) live: Arc<std::sync::RwLock<LiveConfig>>,
    pub(crate) http: http::SourceHttpClient,
    pub(crate) db: Option<db::Database>,
    pub(crate) control_plane: Option<control_plane::ControlPlane>,
    pub(crate) health: health::HealthRegistry,
    pub(crate) admin_auth: AdminAuth,
    pub(crate) secrets: secrets::SecretResolver,
    pub(crate) prometheus_handle: PrometheusHandle,
}

impl AppState {
    pub(crate) fn snapshot(&self) -> LiveConfig {
        self.live.read().unwrap().clone()
    }

    pub(crate) fn reload_snapshot(&self, snapshot: control_plane::RuntimeSnapshot) {
        let candidate = LiveConfig::from_snapshot(snapshot);
        let mut current = self.live.write().unwrap();
        if candidate.revision >= current.revision {
            let revision = candidate.revision;
            *current = candidate;
            observability::set_snapshot_revision(revision);
        }
    }
}
