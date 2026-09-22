use crate::{control_plane::quota::QuotaService, http::response::error_response, state::AppState};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{Response, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::json;

pub(crate) async fn list_upstream_quotas(State(state): State<AppState>) -> Response<Body> {
    let Some(database) = &state.db else {
        return database_unavailable();
    };
    match QuotaService::new(database.pool(), &state.http, &state.secrets)
        .list()
        .await
    {
        Ok(snapshots) => (StatusCode::OK, Json(json!({"data": snapshots}))).into_response(),
        Err(error) => database_error(error),
    }
}

pub(crate) async fn refresh_upstream_quotas(state: State<AppState>) -> Response<Body> {
    list_upstream_quotas(state).await
}

pub(crate) async fn get_upstream_quota(
    Path(account_id): Path<String>,
    State(state): State<AppState>,
) -> Response<Body> {
    let Some(database) = &state.db else {
        return database_unavailable();
    };
    match QuotaService::new(database.pool(), &state.http, &state.secrets)
        .get(&account_id)
        .await
    {
        Ok(Some(snapshot)) => (StatusCode::OK, Json(json!({"data": snapshot}))).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "quota_account_not_found",
            "upstream account not found",
        ),
        Err(error) => database_error(error),
    }
}

pub(crate) async fn refresh_upstream_quota(
    account: Path<String>,
    state: State<AppState>,
) -> Response<Body> {
    get_upstream_quota(account, state).await
}

fn database_unavailable() -> Response<Body> {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
        "DATABASE_URL is not configured",
    )
}

fn database_error(error: sqlx::Error) -> Response<Body> {
    tracing::error!(%error, "upstream quota database query failed");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "quota_database_error",
        "upstream quota database query failed",
    )
}
