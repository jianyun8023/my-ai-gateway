use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, Response, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Redirect},
    routing::{get, post, put},
    Router,
};
use serde_json::Value;
use tower_http::{services::ServeDir, trace::TraceLayer};

use crate::{api, auth::AdminAuth, http::response::error_response, infra::audit, state::AppState};

pub(crate) fn application(state: AppState) -> Router {
    use api::{admin, discovery, events, health_admin, keys, ops, proxy, usage};

    let discovery_router = discovery::auxiliary_router_with_health(
        state.db.clone(),
        state.http.clone(),
        state.admin_auth.clone(),
        state.health.clone(),
    );
    let admin_api = Router::new()
        .route("/admin/keys", get(keys::list_keys).post(keys::create_key))
        .route(
            "/admin/keys/{id}",
            get(keys::get_key).delete(keys::revoke_key),
        )
        .route("/admin/keys/{id}/rotate", post(keys::rotate_key))
        .route("/admin/keys/{id}/value", get(keys::reveal_key))
        .route("/admin/keys/{id}/revoke", post(keys::revoke_key))
        .route("/admin/usage/summary", get(usage::usage_summary))
        .route("/admin/usage/timeseries", get(usage::usage_timeseries))
        .route("/admin/usage/breakdown", get(usage::usage_breakdown))
        .route("/admin/usage/events", get(usage::usage_events))
        .route("/admin/usage/export", get(usage::usage_export))
        .route("/admin/usage/aggregate", get(usage::usage_aggregate))
        .route(
            "/admin/usage/events/{request_id}",
            get(usage::usage_event_detail),
        )
        .route(
            "/admin/retention/policies",
            get(ops::list_retention_policies).put(ops::update_retention_policies),
        )
        .route(
            "/admin/retention",
            get(ops::list_retention_policies).put(ops::update_retention_policies),
        )
        .route(
            "/admin/retention/cleanup",
            get(ops::list_retention_cleanups).post(ops::start_retention_cleanup),
        )
        .route(
            "/admin/retention/runs",
            get(ops::list_retention_cleanups).post(ops::start_retention_cleanup),
        )
        .route(
            "/admin/retention/cleanup/{id}",
            get(ops::get_retention_cleanup),
        )
        .route(
            "/admin/retention/runs/{id}",
            get(ops::get_retention_cleanup),
        )
        .route(
            "/admin/retention/cleanup/{id}/cancel",
            post(ops::cancel_retention_cleanup),
        )
        .route(
            "/admin/retention/cleanup/{id}/retry",
            post(ops::retry_retention_cleanup),
        )
        .route(
            "/admin/retention/runs/{id}/cancel",
            post(ops::cancel_retention_cleanup),
        )
        .route(
            "/admin/retention/runs/{id}/retry",
            post(ops::retry_retention_cleanup),
        )
        .route(
            "/admin/control-plane/export",
            get(ops::export_control_plane),
        )
        .route(
            "/admin/control-plane/import",
            post(ops::import_control_plane),
        )
        .route("/admin/backup/export", get(ops::export_control_plane))
        .route("/admin/backup/import", post(ops::import_control_plane))
        .route("/admin/audit", get(ops::list_audit_logs))
        .route("/admin/events", get(events::list_events))
        .route("/admin/backups/{id}", get(ops::get_backup_run))
        .route("/admin/backups", get(ops::list_backup_runs))
        .route("/admin/backup/{id}", get(ops::get_backup_run))
        .route("/admin/ops/schema", get(ops::ops_schema_metadata))
        .route("/admin/schema", get(ops::ops_schema_metadata))
        .route(
            "/admin/sources",
            get(admin::list_sources).post(admin::create_source),
        )
        .route(
            "/admin/sources/{id}",
            get(admin::get_source)
                .put(admin::update_source)
                .delete(admin::delete_source),
        )
        .route(
            "/admin/sources/{id}/enabled",
            put(admin::set_source_enabled),
        )
        .route(
            "/admin/sources/{source_id}/models/{upstream_model_id}/capabilities",
            get(admin::list_source_model_capabilities),
        )
        .route(
            "/admin/sources/{source_id}/models/{upstream_model_id}/capabilities/{protocol}",
            put(admin::upsert_source_model_capability),
        )
        .route(
            "/admin/accounts",
            get(admin::list_accounts).post(admin::create_account),
        )
        .route(
            "/admin/accounts/{id}",
            get(admin::get_account)
                .put(admin::update_account)
                .delete(admin::delete_account),
        )
        .route(
            "/admin/accounts/{id}/enabled",
            put(admin::set_account_enabled),
        )
        .route("/admin/credentials/encrypt", post(keys::encrypt_credential))
        .route(
            "/admin/accounts/{id}/credentials/rotate",
            post(keys::rotate_account_credential),
        )
        .route(
            "/admin/logical-models",
            get(admin::list_logical_models).post(admin::create_logical_model),
        )
        .route(
            "/admin/logical-models/{id}",
            get(admin::get_logical_model)
                .put(admin::update_logical_model)
                .delete(admin::delete_logical_model),
        )
        .route(
            "/admin/logical-models/{id}/enabled",
            put(admin::set_logical_model_enabled),
        )
        .route(
            "/admin/model-bindings",
            get(admin::list_model_bindings).post(admin::create_model_binding),
        )
        .route(
            "/admin/model-bindings/{id}",
            get(admin::get_model_binding)
                .put(admin::update_model_binding)
                .delete(admin::delete_model_binding),
        )
        .route(
            "/admin/model-bindings/{id}/enabled",
            put(admin::set_model_binding_enabled),
        )
        .route(
            "/admin/routes",
            get(admin::list_routes).post(admin::create_route),
        )
        .route(
            "/admin/routes/{id}",
            get(admin::get_route)
                .put(admin::update_route)
                .delete(admin::delete_route),
        )
        .route("/admin/routes/{id}/enabled", put(admin::set_route_enabled))
        .route("/admin/config/reload", post(admin::reload_config))
        .route("/admin/capabilities", get(admin::admin_capabilities))
        .route("/admin/health", get(health_admin::admin_health))
        .route(
            "/admin/health/probe",
            post(health_admin::admin_health_probe),
        )
        .route(
            "/admin/health/probes",
            post(health_admin::admin_health_probes),
        )
        .route(
            "/admin/health/{id}",
            get(health_admin::admin_account_health),
        )
        .route(
            "/admin/health/{id}/probe",
            post(health_admin::admin_account_probe),
        )
        .route(
            "/admin/accounts/{id}/probe",
            post(health_admin::admin_account_probe),
        )
        .route(
            "/admin/routes/{protocol}/{model}",
            get(admin::resolve_route),
        )
        .with_state(state.clone())
        .merge(discovery_router)
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            audit_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.admin_auth.clone(),
            require_admin_auth,
        ));
    Router::new()
        .route("/healthz", get(proxy::healthz))
        .route("/metrics", get(proxy::metrics_handler))
        .route("/v1/models", get(proxy::models))
        .route("/v1/chat/completions", post(proxy::chat_completions))
        .route("/v1/responses", post(proxy::responses))
        .route("/v1/messages", post(proxy::messages))
        .nest_service("/admin", ServeDir::new("web/dist"))
        .with_state(state)
        .merge(admin_api)
        // 控制台前端使用相对路径引用静态资源；直接访问 /admin 会让相对路径
        // 解析到站点根目录而 404。显式路由与 nest_service 同路径注册会冲突，
        // 因此用中间件把 /admin 统一重定向到带尾斜杠的形式。
        .layer(middleware::from_fn(redirect_admin_console_root))
        .layer(TraceLayer::new_for_http())
}

async fn redirect_admin_console_root(request: Request<Body>, next: Next) -> Response<Body> {
    if request.uri().path() == "/admin" {
        return Redirect::permanent("/admin/").into_response();
    }
    next.run(request).await
}

async fn audit_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    let method = request.method().clone();
    if !should_audit_admin_request(&method, request.uri().path()) {
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 4 * 1024 * 1024)
        .await
        .unwrap_or_default();
    let payload: Option<Value> = serde_json::from_slice(&bytes).ok();
    let context =
        audit::context_from_request(&method, parts.uri.path(), &parts.headers, payload.as_ref());
    let pool = state.db.as_ref().map(|db| db.pool().clone());
    let request = Request::from_parts(parts, Body::from(bytes));
    audit::scope(context.clone(), async move {
        let response = next.run(request).await;
        if !context.was_recorded() {
            if let Some(pool) = &pool {
                let status_str = response.status().as_u16().to_string();
                let result = if response.status().is_success() {
                    audit::append_current_success_pool(pool).await
                } else {
                    audit::append_current_failure_pool(pool, &status_str, None).await
                };
                let _ = state.events.observe("audit.write", result).await;
            }
        }
        response
    })
    .await
}

pub(crate) fn should_audit_admin_request(method: &Method, path: &str) -> bool {
    match *method {
        Method::HEAD | Method::OPTIONS => false,
        Method::GET => {
            let segments = path.trim_matches('/').split('/').collect::<Vec<_>>();
            matches!(segments.as_slice(), ["admin", "keys", _, "value"])
        }
        _ => true,
    }
}

async fn require_admin_auth(
    State(auth): State<AdminAuth>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    if !auth.authorized(request.headers()) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        );
    }
    next.run(request).await
}
