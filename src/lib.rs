mod api;
mod app;
mod auth;
mod control_plane;
mod domain;
pub(crate) mod http;
mod infra;
mod proxy;
mod runtime;
pub(crate) mod source_url;
mod state;

/// Entry point for the gateway binary.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    runtime::run().await
}

/// Minimal test-support API for integration tests.
///
/// Exposes only the types and helpers needed to construct a Gateway `Router`
/// without a PostgreSQL database.  NOT part of the stable public interface;
/// this module exists solely so `tests/` integration tests can build
/// in-process gateway instances pointed at the MockProvider.
///
/// Gated behind `cfg(test)` (unit tests) or the `test-support` cargo feature
/// (integration tests).
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use std::sync::Arc;

    pub use crate::domain::config::{
        AccountConfig, Capabilities, CapabilityMode, GatewayConfig, ProtocolCapability,
        ProtocolMode, ProviderConfig, RouteConfig,
    };
    pub use crate::domain::protocol::Protocol;

    /// Build a gateway `Router` suitable for integration testing.
    ///
    /// The returned router is a fully wired Axum application backed by
    /// in-memory configuration only (no database, no control plane).
    /// Provider `base_url` should point at a [`MockProvider`] instance.
    pub fn test_gateway_router(config: GatewayConfig) -> axum::Router {
        let config = Arc::new(config);
        let live = crate::state::LiveConfig::legacy(config);
        let state = crate::state::AppState {
            live: Arc::new(std::sync::RwLock::new(live)),
            http: crate::http::test_client().expect("test HTTP client"),
            db: None,
            control_plane: None,
            health: crate::infra::health::HealthRegistry::new(std::time::Duration::from_secs(30)),
            admin_auth: crate::auth::AdminAuth::from_key(None),
            secrets: crate::infra::secrets::SecretResolver::empty(),
            prometheus_handle: crate::infra::observability::prometheus_handle(),
        };
        crate::app::application(state)
    }
}

#[cfg(test)]
mod test_helpers;
#[cfg(test)]
mod tests;
