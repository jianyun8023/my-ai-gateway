//! Shared application state and atomic publication of validated snapshots.
use std::sync::Arc;

use metrics_exporter_prometheus::PrometheusHandle;

use crate::{
    auth::AdminAuth,
    control_plane,
    domain::{catalog::PublishedModel, config::GatewayConfig, routing::RouteResolver},
    http,
    infra::{audit, db, events, health, observability, secrets},
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
    pub(crate) events: events::EventRepository,
    pub(crate) health: health::HealthRegistry,
    pub(crate) admin_auth: AdminAuth,
    pub(crate) secrets: secrets::SecretResolver,
    pub(crate) prometheus_handle: PrometheusHandle,
}

impl AppState {
    pub(crate) fn snapshot(&self) -> LiveConfig {
        self.live.read().unwrap().clone()
    }

    pub(crate) async fn reload_snapshot(&self, snapshot: control_plane::RuntimeSnapshot) -> bool {
        let candidate = LiveConfig::from_snapshot(snapshot);
        let candidate_revision = candidate.revision;
        let candidate_generated_at = candidate.generated_at;
        let current_revision = {
            let mut current = self.live.write().unwrap();
            let current_revision = current.revision;
            if candidate_revision >= current_revision {
                *current = candidate;
                observability::set_snapshot_revision(candidate_revision);
            }
            current_revision
        };
        let accepted = candidate_revision >= current_revision;
        self.events.database_recovered("runtime.snapshot").await;
        let correlation_id = audit::current_context().map(|context| context.request_id);
        let mut built_event = events::SystemEvent::new(
            "configuration",
            "runtime.snapshot_built",
            "info",
            "runtime_snapshot",
            "Runtime snapshot built",
        )
        .subject_id(candidate_revision.to_string())
        .details(serde_json::json!({
            "snapshot_revision": candidate_revision,
            "snapshot_generated_at": candidate_generated_at,
        }));
        if let Some(correlation_id) = &correlation_id {
            built_event = built_event.correlation_id(correlation_id.clone());
        }
        self.events.record(built_event).await;

        let mut switch_event = events::SystemEvent::new(
            "configuration",
            if accepted {
                "runtime.snapshot_switched"
            } else {
                "runtime.snapshot_switch_rejected"
            },
            if accepted { "info" } else { "warning" },
            "runtime_snapshot",
            if accepted {
                "Runtime snapshot switched"
            } else {
                "Older runtime snapshot was rejected"
            },
        )
        .subject_id(candidate_revision.to_string())
        .details(serde_json::json!({
            "previous_revision": current_revision,
            "candidate_revision": candidate_revision,
        }));
        if let Some(correlation_id) = correlation_id {
            switch_event = switch_event.correlation_id(correlation_id);
        }
        self.events.record(switch_event).await;
        accepted
    }
}
