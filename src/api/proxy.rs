use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header::CONTENT_TYPE, HeaderMap, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::domain::protocol::Protocol;
use crate::proxy::service as proxy_service;
use crate::state::{authorized_with_db, data_plane_error_response, AppState};

pub(crate) async fn healthz(State(state): State<AppState>) -> Json<Value> {
    let live = state.snapshot();
    Json(
        json!({"status":"ok", "sources":live.config.providers.len(), "accounts":live.config.accounts.len(), "snapshot_revision":live.revision, "snapshot_generated_at":live.generated_at}),
    )
}

pub(crate) async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        state.prometheus_handle.render(),
    )
}

pub(crate) async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    let request_id = Uuid::new_v4().to_string();
    if authorized_with_db(&state, &headers, None).await.is_none() {
        // Mirror the OpenAI Chat Completions data-plane envelope so clients
        // see a uniform 401 shape across all `/v1/*` endpoints.  `None` for
        // model skips the virtual-key `allowed_models` whitelist because the
        // model catalogue endpoint does not address a single model.
        return data_plane_error_response(
            Protocol::OpenAiChatCompletions,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "invalid or revoked virtual key",
            &request_id,
        );
    }
    let live = state.snapshot();
    let mut data = Vec::new();
    for model in live.models.iter() {
        let mut healthy = false;
        for account_id in &model.account_ids {
            let enabled = live
                .config
                .account(account_id)
                .is_some_and(|account| account.enabled);
            if enabled && state.health.is_available(account_id).await {
                healthy = true;
                break;
            }
        }
        if healthy {
            data.push(json!({"id":model.id,"object":"model","owned_by":"gateway"}));
        }
    }
    Json(json!({"object":"list","data":data})).into_response()
}

pub(crate) async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    proxy_service::proxy(state, headers, body, Protocol::OpenAiChatCompletions).await
}

pub(crate) async fn responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    proxy_service::proxy(state, headers, body, Protocol::OpenAiResponses).await
}

pub(crate) async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    proxy_service::proxy(state, headers, body, Protocol::AnthropicMessages).await
}
