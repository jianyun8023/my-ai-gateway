use crate::{
    control_plane::model_catalog::{
        CatalogAvailability, CatalogError, CatalogStatus, MetadataValues, ModelCatalogRepository,
        SourceModelConfirmation,
    },
    control_plane::model_discovery::{DiscoveryServiceError, ModelDiscoveryService},
    domain::{
        protocol::Protocol,
        provider_preset::{provider_preset_diff, ProviderPresetDefinition},
    },
    infra::{db::Database, source_url::SourceUrlPolicyError},
    proxy::transport::SourceHttpClient,
    state::AdminAuth,
};
use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const MAX_ADMIN_JSON_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
struct DiscoveryApiState {
    database: Option<Database>,
    http: SourceHttpClient,
    admin_auth: AdminAuth,
    health: Option<crate::infra::health::HealthRegistry>,
}

#[cfg(test)]
pub fn router(database: Option<Database>, http: SourceHttpClient) -> Router {
    let health = database.clone().map(|database| {
        crate::infra::health::HealthRegistry::with_database_config(
            database,
            crate::infra::health::HealthConfig::default(),
        )
    });
    router_inner(database, http, AdminAuth::test(), true, health)
}

/// Discovery endpoints mounted by the gateway application. The Source
/// collection itself is owned by the DB-first control plane so creation can
/// publish a validated runtime snapshot in the same operation.
#[allow(dead_code)]
pub fn auxiliary_router(
    database: Option<Database>,
    http: SourceHttpClient,
    admin_auth: AdminAuth,
) -> Router {
    router_inner(database, http, admin_auth, false, None)
}

pub fn auxiliary_router_with_health(
    database: Option<Database>,
    http: SourceHttpClient,
    admin_auth: AdminAuth,
    health: crate::infra::health::HealthRegistry,
) -> Router {
    router_inner(database, http, admin_auth, false, Some(health))
}

fn router_inner(
    database: Option<Database>,
    http: SourceHttpClient,
    admin_auth: AdminAuth,
    include_source_collection: bool,
    health: Option<crate::infra::health::HealthRegistry>,
) -> Router {
    let state = DiscoveryApiState {
        database,
        http,
        admin_auth,
        health,
    };
    let router = Router::new()
        .route("/admin/provider-presets", get(list_provider_presets))
        .route(
            "/admin/sources/{source_id}/preset-diff",
            get(source_preset_diff),
        )
        .route(
            "/admin/sources/{source_id}/connection-tests",
            post(test_connection),
        )
        .route(
            "/admin/sources/{source_id}/discoveries",
            post(discover_models),
        )
        .route(
            "/admin/sources/{source_id}/discoveries/latest",
            get(latest_discovery),
        )
        .route(
            "/admin/sources/{source_id}/models",
            get(list_source_models).patch(edit_source_model),
        )
        .route(
            "/admin/sources/{source_id}/models/confirm",
            post(confirm_source_models),
        );
    let router = if include_source_collection {
        router.route("/admin/sources", get(list_sources).post(create_source))
    } else {
        router
    };
    router.with_state(state)
}

async fn list_provider_presets(
    State(state): State<DiscoveryApiState>,
    headers: HeaderMap,
) -> Response<Body> {
    let Some(repository) = authorized_repository(&state, &headers) else {
        return authorization_or_database_error(&state, &headers);
    };
    match repository.list_provider_presets().await {
        Ok(presets) => ok(json!({"data":presets})),
        Err(error) => catalog_error_response(error),
    }
}

#[derive(Deserialize)]
struct CreateSourceRequest {
    id: String,
    display_name: String,
    provider_preset_id: String,
    provider_preset_version: Option<i32>,
    base_url: Option<String>,
    #[serde(default)]
    endpoint_overrides: BTreeMap<Protocol, String>,
}

async fn create_source(
    State(state): State<DiscoveryApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let Some(repository) = authorized_repository(&state, &headers) else {
        return authorization_or_database_error(&state, &headers);
    };
    let request: CreateSourceRequest = match parse_json(&body) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    if request.id.trim().is_empty()
        || request.display_name.trim().is_empty()
        || request.provider_preset_id.trim().is_empty()
    {
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_source",
            "source id, display_name, and provider_preset_id are required",
        );
    }
    let preset = match request.provider_preset_version {
        Some(version) => {
            repository
                .get_provider_preset(&request.provider_preset_id, version)
                .await
        }
        None => {
            repository
                .latest_provider_preset(&request.provider_preset_id)
                .await
        }
    };
    let preset = match preset {
        Ok(preset) => preset,
        Err(error) => return catalog_error_response(error),
    };
    let definition: ProviderPresetDefinition =
        match serde_json::from_value(preset.definition.clone()) {
            Ok(definition) => definition,
            Err(_) => {
                return api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_provider_preset",
                    "provider preset cannot create a managed source",
                )
            }
        };
    if definition.validate().is_err() {
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_provider_preset",
            "provider preset cannot create a managed source",
        );
    }
    let base_url = request
        .base_url
        .unwrap_or_else(|| definition.default_base_url.clone());
    if let Err(error) = state.http.validate_base_url(&base_url) {
        return source_url_error_response(error);
    }
    let mut endpoints = definition
        .protocols
        .iter()
        .map(|(protocol, protocol_preset)| (*protocol, protocol_preset.endpoint.clone()))
        .collect::<BTreeMap<_, _>>();
    for (protocol, endpoint) in request.endpoint_overrides {
        if !endpoint.starts_with('/') || endpoint.starts_with("//") {
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_endpoint",
                "source endpoint overrides must be absolute paths",
            );
        }
        endpoints.insert(protocol, endpoint);
    }
    let input = crate::domain::catalog::SourceInput {
        id: request.id,
        display_name: request.display_name,
        provider_preset_id: preset.id,
        provider_preset_version: preset.version,
        base_url,
        endpoints: serde_json::to_value(endpoints).expect("source endpoints serialize"),
        auth_config: definition.auth_snapshot(),
        protocol_capabilities: definition.protocol_capabilities_snapshot(),
    };
    match repository.create_source(&input).await {
        Ok(source) => (StatusCode::CREATED, Json(json!({"data":source}))).into_response(),
        Err(error) => catalog_error_response(error),
    }
}

async fn list_sources(
    State(state): State<DiscoveryApiState>,
    headers: HeaderMap,
) -> Response<Body> {
    let Some(repository) = authorized_repository(&state, &headers) else {
        return authorization_or_database_error(&state, &headers);
    };
    match repository.list_sources().await {
        Ok(sources) => ok(json!({"data":sources})),
        Err(error) => catalog_error_response(error),
    }
}

async fn source_preset_diff(
    State(state): State<DiscoveryApiState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
) -> Response<Body> {
    let Some(repository) = authorized_repository(&state, &headers) else {
        return authorization_or_database_error(&state, &headers);
    };
    let source = match repository.get_source(&source_id).await {
        Ok(source) => source,
        Err(error) => return catalog_error_response(error),
    };
    let latest = match repository
        .latest_provider_preset(&source.provider_preset_id)
        .await
    {
        Ok(preset) => preset,
        Err(error) => return catalog_error_response(error),
    };
    ok(json!({"data":provider_preset_diff(&source,&latest)}))
}

#[derive(Deserialize)]
struct ConnectionTestRequest {
    account_id: String,
    protocol: Protocol,
    model: Option<String>,
    #[serde(default = "default_actor")]
    requested_by: String,
}

async fn test_connection(
    State(state): State<DiscoveryApiState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
    body: Bytes,
) -> Response<Body> {
    let Some(database) = authorized_database(&state, &headers) else {
        return authorization_or_database_error(&state, &headers);
    };
    let request: ConnectionTestRequest = match parse_json(&body) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let service = ModelDiscoveryService::new(
        ModelCatalogRepository::from_database(&database),
        state.http.clone(),
    );
    match service
        .test_connection(
            &source_id,
            &request.account_id,
            request.protocol,
            request.model.as_deref(),
            &request.requested_by,
        )
        .await
    {
        Ok(result) => {
            if let Some(health) = &state.health {
                health.apply_connection_test(&result).await;
            }
            ok(json!({"data":result}))
        }
        Err(error) => service_error_response(error),
    }
}

#[derive(Deserialize)]
struct DiscoveryRequest {
    account_id: String,
    #[serde(default = "default_actor")]
    requested_by: String,
}

async fn discover_models(
    State(state): State<DiscoveryApiState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
    body: Bytes,
) -> Response<Body> {
    let Some(database) = authorized_database(&state, &headers) else {
        return authorization_or_database_error(&state, &headers);
    };
    let request: DiscoveryRequest = match parse_json(&body) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let service = ModelDiscoveryService::new(
        ModelCatalogRepository::from_database(&database),
        state.http.clone(),
    );
    match service
        .discover(&source_id, &request.account_id, &request.requested_by)
        .await
    {
        Ok(result) => ok(json!({"data":result})),
        Err(error) => service_error_response(error),
    }
}

async fn latest_discovery(
    State(state): State<DiscoveryApiState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
) -> Response<Body> {
    let Some(repository) = authorized_repository(&state, &headers) else {
        return authorization_or_database_error(&state, &headers);
    };
    match repository.latest_discovery_run(&source_id).await {
        Ok(Some(run)) => match run.discovery_diff() {
            Ok(diff) => ok(json!({
                "data": run,
                "diff": diff,
                "last_discovered_at": run.completed_at,
            })),
            Err(error) => catalog_error_response(error),
        },
        Ok(None) => api_error(
            StatusCode::NOT_FOUND,
            "discovery_not_found",
            "source has no discovery history",
        ),
        Err(error) => catalog_error_response(error),
    }
}

#[derive(Default, Deserialize)]
struct SourceModelsQuery {
    confirmation_status: Option<CatalogStatus>,
    availability_status: Option<CatalogAvailability>,
}

async fn list_source_models(
    State(state): State<DiscoveryApiState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
    Query(query): Query<SourceModelsQuery>,
) -> Response<Body> {
    let Some(repository) = authorized_repository(&state, &headers) else {
        return authorization_or_database_error(&state, &headers);
    };
    match repository
        .list_source_models(
            &source_id,
            query.confirmation_status,
            query.availability_status,
        )
        .await
    {
        Ok(models) => ok(json!({"data":models})),
        Err(error) => catalog_error_response(error),
    }
}

#[derive(Deserialize)]
struct EditSourceModelRequest {
    upstream_model_id: String,
    #[serde(default)]
    metadata: MetadataValues,
}

async fn edit_source_model(
    State(state): State<DiscoveryApiState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
    body: Bytes,
) -> Response<Body> {
    let Some(repository) = authorized_repository(&state, &headers) else {
        return authorization_or_database_error(&state, &headers);
    };
    let request: EditSourceModelRequest = match parse_json(&body) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    match repository
        .update_source_model_user_overrides(
            &source_id,
            &request.upstream_model_id,
            &request.metadata,
        )
        .await
    {
        Ok(model) => ok(json!({"data":model})),
        Err(error) => catalog_error_response(error),
    }
}

#[derive(Deserialize)]
struct ConfirmSourceModelRequest {
    upstream_model_id: String,
    #[serde(default)]
    metadata: MetadataValues,
}

#[derive(Deserialize)]
struct BulkConfirmRequest {
    models: Vec<ConfirmSourceModelRequest>,
}

async fn confirm_source_models(
    State(state): State<DiscoveryApiState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
    body: Bytes,
) -> Response<Body> {
    let Some(repository) = authorized_repository(&state, &headers) else {
        return authorization_or_database_error(&state, &headers);
    };
    let request: BulkConfirmRequest = match parse_json(&body) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let confirmations = request
        .models
        .into_iter()
        .map(|model| SourceModelConfirmation {
            upstream_model_id: model.upstream_model_id,
            user_overrides: model.metadata,
        })
        .collect::<Vec<_>>();
    match repository
        .confirm_source_models(&source_id, &confirmations)
        .await
    {
        Ok(models) => ok(json!({"data":models})),
        Err(error) => catalog_error_response(error),
    }
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, Box<Response<Body>>> {
    if body.len() > MAX_ADMIN_JSON_BYTES {
        return Err(Box::new(api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "request_too_large",
            "admin request body exceeds the size limit",
        )));
    }
    serde_json::from_slice(body).map_err(|_| {
        Box::new(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "request body is not valid for this operation",
        ))
    })
}

fn authorized_database(state: &DiscoveryApiState, headers: &HeaderMap) -> Option<Database> {
    state
        .admin_auth
        .authorized(headers)
        .then(|| state.database.clone())
        .flatten()
}

fn authorized_repository(
    state: &DiscoveryApiState,
    headers: &HeaderMap,
) -> Option<ModelCatalogRepository> {
    authorized_database(state, headers)
        .map(|database| ModelCatalogRepository::from_database(&database))
}

fn authorization_or_database_error(
    state: &DiscoveryApiState,
    headers: &HeaderMap,
) -> Response<Body> {
    if !state.admin_auth.authorized(headers) {
        api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "admin key required",
        )
    } else if state.database.is_none() {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DATABASE_URL is not configured",
        )
    } else {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "catalog_unavailable",
            "catalog service is unavailable",
        )
    }
}

fn service_error_response(error: DiscoveryServiceError) -> Response<Body> {
    let status = match error.code() {
        "not_found" => StatusCode::NOT_FOUND,
        "database_error" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    api_error(status, error.code(), error.public_message())
}

fn catalog_error_response(error: CatalogError) -> Response<Body> {
    match error {
        CatalogError::NotFound(message) => api_error(StatusCode::NOT_FOUND, "not_found", &message),
        CatalogError::InvalidMetadata(message) | CatalogError::InvalidState(message) => api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_catalog_state",
            &message,
        ),
        CatalogError::ImmutableVersionConflict(message) => {
            api_error(StatusCode::CONFLICT, "immutable_version_conflict", &message)
        }
        CatalogError::Database(error)
            if error
                .as_database_error()
                .is_some_and(|error| matches!(error.code().as_deref(), Some("23505"))) =>
        {
            api_error(
                StatusCode::CONFLICT,
                "catalog_conflict",
                "catalog record already exists",
            )
        }
        CatalogError::Database(_) | CatalogError::Json(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_error",
            "catalog database operation failed",
        ),
    }
}

fn source_url_error_response(error: SourceUrlPolicyError) -> Response<Body> {
    if error == SourceUrlPolicyError::InvalidUrl {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_source_url",
            "source base_url must be an http(s) URL without credentials, query, or fragment",
        )
    } else {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "source_url_blocked",
            "source base_url is blocked by server policy",
        )
    }
}

fn default_actor() -> String {
    "admin_api".into()
}

fn ok(value: Value) -> Response<Body> {
    (StatusCode::OK, Json(value)).into_response()
}

fn api_error(status: StatusCode, kind: &str, message: &str) -> Response<Body> {
    (
        status,
        Json(json!({"error":{"code":kind,"type":kind,"message":message}})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control_plane::model_catalog::install_builtin_presets,
        control_plane::model_catalog::{MetadataField, SourceModelRefresh},
        domain::provider_preset::BUILTIN_PROVIDER_PRESET_VERSION,
        proxy::transport,
        state::TEST_ADMIN_KEY,
    };
    use axum::{body::to_bytes, http::Request};
    use chrono::Utc;
    use tower::ServiceExt;

    #[test]
    fn source_creation_rejects_credential_bearing_urls() {
        let client = transport::test_client().unwrap();
        assert!(client
            .validate_base_url("https://api.example.com/base")
            .is_ok());
        assert!(client
            .validate_base_url("https://secret@api.example.com")
            .is_err());
        assert!(client
            .validate_base_url("https://api.example.com?key=secret")
            .is_err());
    }

    fn admin_request(method: &str, uri: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", TEST_ADMIN_KEY))
            .body(Body::from(body.to_string()))
            .expect("admin API request")
    }

    async fn response_json(response: Response<Body>) -> Value {
        serde_json::from_slice(
            &to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("read API response"),
        )
        .expect("API JSON response")
    }

    #[tokio::test]
    async fn blocked_source_url_error_does_not_echo_the_target() {
        let response = source_url_error_response(SourceUrlPolicyError::DisallowedTarget);
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = response_json(response).await;
        assert_eq!(body["error"]["code"], "source_url_blocked");
        let serialized = body.to_string();
        assert!(!serialized.contains("169.254.169.254"));
        assert!(!serialized.contains("metadata.google.internal"));
    }

    #[tokio::test]
    async fn postgres_source_pending_edit_and_bulk_confirm_api_contract() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("skipping discovery API test: TEST_DATABASE_URL is not set");
            return;
        };
        let database = Database::connect(&url)
            .await
            .expect("connect discovery API PostgreSQL database");
        let repository = ModelCatalogRepository::from_database(&database);
        install_builtin_presets(&repository)
            .await
            .expect("install built-in presets");
        let app = router(
            Some(database.clone()),
            transport::test_client().expect("API HTTP client"),
        );
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let source_id = format!("discovery-api-source-{suffix}");

        let response = app
            .clone()
            .oneshot(admin_request(
                "POST",
                "/admin/sources",
                json!({
                    "id":source_id,
                    "display_name":"Discovery API Source",
                    "provider_preset_id":"deepseek",
                    "base_url":"https://source.example"
                }),
            ))
            .await
            .expect("create source response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let created = response_json(response).await;
        assert_eq!(created["data"]["provider_preset_id"], "deepseek");
        assert_eq!(
            created["data"]["provider_preset_version"],
            BUILTIN_PROVIDER_PRESET_VERSION
        );
        assert_eq!(
            created["data"]["provider_preset_snapshot"]["default_base_url"],
            "https://api.deepseek.com"
        );

        for model in ["api-model-a", "api-model-b"] {
            repository
                .refresh_source_model(&SourceModelRefresh {
                    source_id: source_id.clone(),
                    upstream_model_id: model.into(),
                    raw_snapshot: json!({"id":model}),
                    upstream_metadata: MetadataValues::from_fields([
                        (MetadataField::LogicalModelName, json!(model)),
                        (MetadataField::DisplayName, json!(model)),
                    ])
                    .unwrap(),
                    matched_preset: None,
                    preset_metadata: None,
                    discovered_at: Utc::now(),
                })
                .await
                .expect("seed pending source model");
        }

        let response = app
            .clone()
            .oneshot(admin_request(
                "GET",
                &format!(
                    "/admin/sources/{source_id}/models?confirmation_status=pending&availability_status=available"
                ),
                json!({}),
            ))
            .await
            .expect("pending models response");
        assert_eq!(response.status(), StatusCode::OK);
        let pending = response_json(response).await;
        assert_eq!(pending["data"].as_array().unwrap().len(), 2);

        let response = app
            .clone()
            .oneshot(admin_request(
                "PATCH",
                &format!("/admin/sources/{source_id}/models"),
                json!({
                    "upstream_model_id":"api-model-a",
                    "metadata":{"context_window":777}
                }),
            ))
            .await
            .expect("edit model response");
        assert_eq!(response.status(), StatusCode::OK);
        let edited = response_json(response).await;
        assert_eq!(edited["data"]["metadata"]["context_window"], 777);
        assert_eq!(edited["data"]["field_sources"]["context_window"], "user");

        let response = app
            .clone()
            .oneshot(admin_request(
                "POST",
                &format!("/admin/sources/{source_id}/models/confirm"),
                json!({"models":[
                    {"upstream_model_id":"api-model-a","metadata":{}},
                    {"upstream_model_id":"api-model-b","metadata":{"logical_model_name":"public-b"}}
                ]}),
            ))
            .await
            .expect("bulk confirmation response");
        assert_eq!(response.status(), StatusCode::OK);
        let confirmed = response_json(response).await;
        assert_eq!(confirmed["data"].as_array().unwrap().len(), 2);
        assert!(confirmed["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|model| model["confirmation_status"] == "confirmed"));

        let response = app
            .clone()
            .oneshot(admin_request(
                "GET",
                &format!("/admin/sources/{source_id}/models?confirmation_status=pending"),
                json!({}),
            ))
            .await
            .expect("empty pending models response");
        assert_eq!(
            response_json(response).await["data"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let response = app
            .oneshot(admin_request(
                "GET",
                &format!("/admin/sources/{source_id}/preset-diff"),
                json!({}),
            ))
            .await
            .expect("preset diff response");
        assert_eq!(response.status(), StatusCode::OK);
        let diff = response_json(response).await;
        assert_eq!(
            diff["data"]["source_version"],
            BUILTIN_PROVIDER_PRESET_VERSION
        );
        assert_eq!(
            diff["data"]["latest_version"],
            BUILTIN_PROVIDER_PRESET_VERSION
        );
        assert!(diff["data"]["changes"].as_array().unwrap().is_empty());

        let binding_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM model_bindings WHERE source_id=$1")
                .bind(&source_id)
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(binding_count, 0);
        sqlx::query("DELETE FROM sources WHERE id=$1")
            .bind(&source_id)
            .execute(database.pool())
            .await
            .expect("clean discovery API source");
    }
}
