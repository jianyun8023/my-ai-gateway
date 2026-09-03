mod api;
mod app;
mod control_plane;
mod domain;
pub(crate) mod http;
mod infra;
mod proxy;
mod runtime;
pub(crate) mod source_url;
mod state;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    runtime::run().await
}

#[cfg(test)]
pub(crate) use app::{application, should_audit_admin_request};

#[cfg(test)]
use api::{
    health_admin::admin_health,
    proxy::{models, responses},
    usage::{csv_field, parse_usage_query, usage_events_csv},
};
#[cfg(test)]
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, Method, Request, Response, StatusCode},
    Router,
};
#[cfg(test)]
use domain::{config, protocol::Protocol};
#[cfg(test)]
use infra::{audit, db, health, observability, ops, secrets};
#[cfg(test)]
use proxy::service::proxy as proxy_fn;
#[cfg(test)]
use proxy::{transport, usage};
#[cfg(test)]
use serde_json::{json, Value};
#[cfg(test)]
use state::{AdminAuth, AppState, EnvRestore, LiveConfig, ENV_LOCK, TEST_ADMIN_KEY};
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use tower::ServiceExt;
#[cfg(test)]
use uuid::Uuid;

#[cfg(test)]
include!("main_tests.rs");
