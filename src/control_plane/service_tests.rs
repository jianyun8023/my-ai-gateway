#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::db::Database;
    use crate::state::{AppState, EnvRestore, TEST_ADMIN_KEY};
    use axum::{
        body::{to_bytes, Body},
        extract::State,
        http::{HeaderMap, Request, StatusCode},
    };
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::{str::FromStr, time::Duration};
    use tower::ServiceExt;

    /// Drive `/v1/models` through the production handler with the fail-open
    /// env state cleared so the unit tests stay hermetic.
    async fn invoke_models_for_test(state: &AppState) -> Value {
        let _environment_lock = crate::state::ENV_LOCK.lock().await;
        // PR #86 (/v1/models now requires data-plane auth) makes a bare
        // HeaderMap fail with a 401 envelope whose body has no `data` field.
        // Seed the static admin key the handler accepts; EnvRestore cleans up
        // when the call returns so subsequent tests still see an empty env.
        let _admin = EnvRestore::set("GATEWAY_API_KEY", TEST_ADMIN_KEY);
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test-admin-key"),
        );
        let response = crate::models(State(state.clone()), headers).await;
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("models response body");
        serde_json::from_slice(&body).expect("models response JSON")
    }

    #[test]
    fn source_validation_uses_server_policy_and_does_not_echo_blocked_targets() {
        let input = SourceWrite {
            id: "private-source".into(),
            display_name: "Private Source".into(),
            provider_preset_id: "custom".into(),
            provider_preset_version: 1,
            base_url: "http://10.20.30.40:8080".into(),
            endpoints: HashMap::new(),
            auth_config: json!({}),
            protocol_capabilities: json!({}),
            enabled: true,
        };
        let error = validate_source_input(&input, &SourceUrlPolicy::default()).unwrap_err();
        let message = error.message();
        assert!(message.contains("server Source URL policy"));
        assert!(!message.contains("10.20.30.40"));

        let allowlisted =
            SourceUrlPolicy::from_allowlist("10.20.0.0/16").expect("private test CIDR");
        assert!(validate_source_input(&input, &allowlisted).is_ok());
    }

    async fn isolated_database() -> (Database, PgPool, String) {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for the PostgreSQL control-plane test");
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect PostgreSQL test admin database");
        let schema = format!("control_plane_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create isolated test schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse TEST_DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect isolated test schema");
        let database = Database::from_test_pool(pool.clone())
            .await
            .expect("migrate isolated test schema");
        (database, admin, schema)
    }

    fn bootstrap_config(base_url: &str) -> GatewayConfig {
        serde_json::from_value(json!({
            "listen_addr": "127.0.0.1:0",
            "providers": [{
                "id": "source-a",
                "name": "Source A",
                "base_url": base_url,
                "models": ["logical-a"],
                "endpoints": {"openai_chat_completions": "/v1/chat/completions"},
                "protocol_capabilities": {
                    "openai_chat_completions": {"mode": "unsupported"}
                },
                "capabilities": {"streaming":"native","tools":"native","usage":"native"},
                "model_overrides": {
                    "logical-a": {
                        "protocol_capabilities": {
                            "openai_chat_completions": {"mode": "native"}
                        },
                        "capabilities": {
                            "streaming": "translated",
                            "tools": "translated",
                            "usage": "translated"
                        }
                    }
                }
            }],
            "accounts": [{
                "id": "account-a",
                "provider_id": "source-a",
                "display_name": "Account A",
                "credential_env": "SOURCE_A_API_KEY",
                "enabled": true,
                "weight": 100,
                "protocol_capabilities": {
                    "openai_chat_completions": {"mode": "unsupported"}
                },
                "capabilities": {
                    "streaming": "unsupported",
                    "tools": "unsupported",
                    "usage": "unsupported"
                }
            }],
            "routes": [{
                "id": "route-a",
                "model": "logical-a",
                "provider_id": "source-a",
                "protocols": ["openai_chat_completions"],
                "primary_account_id": "account-a",
                "mode": "native"
            }]
        }))
        .expect("bootstrap config")
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL and runs against an isolated PostgreSQL schema"]
    async fn postgres_db_first_crud_rollback_snapshot_and_models_contract() {
        let (database, admin, schema) = isolated_database().await;
        let control_plane = ControlPlane::new(database.pool().clone(), "127.0.0.1:0");

        let first = control_plane
            .initialize_from_config(&bootstrap_config("https://source-a.example"), false)
            .await
            .expect("initial DB import")
            .expect("empty control plane imports once");
        assert_eq!(first.models.len(), 1);
        let resolved = first
            .resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-a")
            .expect("imported binding is routable");
        assert_eq!(resolved.upstream_model_id, "logical-a");
        assert_eq!(resolved.protocol_upstream, Protocol::OpenAiChatCompletions);
        let (imported_mode, imported_features): (String, Value) = sqlx::query_as(
            "SELECT mode::text,feature_capabilities FROM source_model_capabilities WHERE source_id='source-a' AND upstream_model_id='logical-a' AND protocol='openai_chat_completions'",
        )
        .fetch_one(database.pool())
        .await
        .expect("load imported model capability precedence fixture");
        assert_eq!(imported_mode, "native");
        assert_eq!(
            imported_features,
            json!({
                "streaming": "translated",
                "tools": "translated",
                "tool_streaming": "unsupported",
                "thinking": "unsupported",
                "web_search": "unsupported",
                "file_search": "unsupported",
                "vision": "unsupported",
                "usage": "translated"
            })
        );

        assert!(control_plane
            .initialize_from_config(
                &bootstrap_config("https://must-not-overwrite.example"),
                false
            )
            .await
            .expect("repeat startup import check")
            .is_none());
        assert_eq!(
            control_plane
                .get_source("source-a")
                .await
                .expect("load source after repeat startup")
                .base_url,
            "https://source-a.example"
        );

        let invalid_adapter_source = SourceWrite {
            id: "invalid-adapter-source".into(),
            display_name: "Invalid Adapter".into(),
            provider_preset_id: "custom".into(),
            provider_preset_version: 1,
            base_url: "https://invalid.example".into(),
            endpoints: HashMap::from([(Protocol::AnthropicMessages, "/v1/messages".into())]),
            auth_config: json!({}),
            protocol_capabilities: serde_json::to_value(HashMap::from([(
                Protocol::OpenAiResponses,
                ProtocolCapability::adapter(Protocol::AnthropicMessages, "missing_adapter"),
            )]))
            .unwrap(),
            enabled: true,
        };
        assert!(matches!(
            control_plane.create_source(&invalid_adapter_source).await,
            Err(ControlPlaneError::Validation(_))
        ));
        assert!(matches!(
            control_plane.get_source("invalid-adapter-source").await,
            Err(ControlPlaneError::NotFound(_))
        ));

        let source_input = SourceWrite {
            id: "source-b".into(),
            display_name: "Source B".into(),
            provider_preset_id: "custom".into(),
            provider_preset_version: 1,
            base_url: "https://source-b.example".into(),
            endpoints: HashMap::from([
                (
                    Protocol::OpenAiChatCompletions,
                    "/v1/chat/completions".into(),
                ),
                (Protocol::AnthropicMessages, "/v1/messages".into()),
            ]),
            auth_config: json!({}),
            protocol_capabilities: json!({}),
            enabled: true,
        };
        control_plane
            .create_source(&source_input)
            .await
            .expect("create source");
        let mut updated_source = source_input.clone();
        updated_source.display_name = "Source B Updated".into();
        control_plane
            .update_source("source-b", &updated_source)
            .await
            .expect("update source");
        assert_eq!(control_plane.list_sources().await.unwrap().len(), 2);

        let account_input = AccountWrite {
            id: "account-b".into(),
            source_id: "source-b".into(),
            display_name: "Account B".into(),
            credential_env: Some("SOURCE_B_API_KEY".into()),
            credential_ciphertext: None,
            enabled: true,
            weight: 50,
        };
        let account = control_plane
            .create_account(&account_input)
            .await
            .expect("create account")
            .record;
        let serialized_account = serde_json::to_value(&account).unwrap();
        assert!(serialized_account.get("credential_ciphertext").is_none());
        assert_eq!(serialized_account["credential_configured"], true);
        let mut updated_account = account_input.clone();
        updated_account.display_name = "Account B Updated".into();
        updated_account.weight = 75;
        assert_eq!(
            control_plane
                .update_account("account-b", &updated_account)
                .await
                .expect("update account")
                .record
                .weight,
            75
        );
        assert_eq!(control_plane.list_accounts().await.unwrap().len(), 2);

        let logical_input = LogicalModelWrite {
            id: "logical-b-id".into(),
            public_name: "logical-b".into(),
            display_name: "Logical B".into(),
            status: CatalogStatus::Confirmed,
            metadata: json!({}),
            field_sources: json!({}),
            enabled: true,
        };
        control_plane
            .create_logical_model(&logical_input)
            .await
            .expect("create logical model");
        let mut updated_logical = logical_input.clone();
        updated_logical.display_name = "Logical B Updated".into();
        control_plane
            .update_logical_model("logical-b-id", &updated_logical)
            .await
            .expect("update logical model");
        assert_eq!(
            control_plane
                .get_logical_model("logical-b-id")
                .await
                .unwrap()
                .display_name,
            "Logical B Updated"
        );
        let pool = database.pool().clone();
        sqlx::query("INSERT INTO source_models (source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,confirmed_at) VALUES ('source-b','upstream-b','confirmed','available','{}'::jsonb,'{}'::jsonb,'{}'::jsonb,NOW())")
            .execute(&pool)
            .await
            .expect("insert confirmed source model fixture");
        sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,feature_capabilities,field_source,confirmed_at) VALUES ('source-b','upstream-b','openai_chat_completions','confirmed','native','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW())")
            .execute(&pool)
            .await
            .expect("insert confirmed capability fixture");
        sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,feature_capabilities,field_source,confirmed_at) VALUES ('source-b','upstream-b','anthropic_messages','confirmed','native','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW()),('source-b','upstream-b','openai_responses','confirmed','adapter','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW())")
            .execute(&pool)
            .await
            .expect_err("adapter fixture without source_protocol/adapter must be rejected");
        sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,feature_capabilities,field_source,confirmed_at) VALUES ('source-b','upstream-b','anthropic_messages','confirmed','native','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW())")
            .execute(&pool)
            .await
            .expect("insert adapter source protocol fixture");
        sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,source_protocol,adapter,feature_capabilities,field_source,confirmed_at) VALUES ('source-b','upstream-b','openai_responses','confirmed','adapter','anthropic_messages','kimi_responses_adapter','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW())")
            .execute(&pool)
            .await
            .expect("insert confirmed adapter capability fixture");
        let binding_input = ModelBindingWrite {
            logical_model_id: "logical-b-id".into(),
            source_id: "source-b".into(),
            account_id: "account-b".into(),
            upstream_model_id: "upstream-b".into(),
            protocol: Protocol::OpenAiChatCompletions,
            status: CatalogStatus::Confirmed,
            enabled: true,
            priority: 100,
        };
        let binding = control_plane
            .create_model_binding(&binding_input)
            .await
            .expect("create confirmed binding")
            .record;
        let adapter_binding = control_plane
            .create_model_binding(&ModelBindingWrite {
                logical_model_id: "logical-b-id".into(),
                source_id: "source-b".into(),
                account_id: "account-b".into(),
                upstream_model_id: "upstream-b".into(),
                protocol: Protocol::OpenAiResponses,
                status: CatalogStatus::Confirmed,
                enabled: true,
                priority: 100,
            })
            .await
            .expect("create confirmed adapter binding")
            .record;
        let mut updated_binding = binding_input.clone();
        updated_binding.priority = 200;
        assert_eq!(
            control_plane
                .update_model_binding(binding.id, &updated_binding)
                .await
                .expect("update binding")
                .record
                .priority,
            200
        );
        let route_input = RouteWrite {
            id: "route-b".into(),
            logical_model_id: "logical-b-id".into(),
            protocols: vec![Protocol::OpenAiChatCompletions, Protocol::OpenAiResponses],
            strategy: "primary_then_weighted_fallback".into(),
            allow_lossy_conversion: false,
            enabled: true,
        };
        control_plane
            .create_route(&route_input)
            .await
            .expect("create route");
        let mut updated_route = route_input.clone();
        updated_route.strategy = "priority_then_fallback".into();
        let route = control_plane
            .update_route("route-b", &updated_route)
            .await
            .expect("update route");
        assert_eq!(control_plane.list_model_bindings().await.unwrap().len(), 3);
        assert_eq!(control_plane.list_routes().await.unwrap().len(), 2);
        let stable_snapshot = route.snapshot;
        let resolved = stable_snapshot
            .resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b")
            .expect("DB binding route resolves");
        assert_eq!(resolved.source_id, "source-b");
        assert_eq!(resolved.provider_id, "custom");
        assert_eq!(resolved.upstream_model_id, "upstream-b");
        assert_eq!(resolved.binding_id, Some(binding.id));
        let adapter_route = stable_snapshot
            .resolver
            .resolve_detailed(Protocol::OpenAiResponses, "logical-b")
            .expect("adapter binding route resolves");
        assert_eq!(adapter_route.source_id, "source-b");
        assert_eq!(adapter_route.provider_id, "custom");
        assert_eq!(adapter_route.protocol_upstream, Protocol::AnthropicMessages);
        assert_eq!(
            adapter_route.adapter.as_deref(),
            Some("kimi_responses_adapter")
        );

        let mut invalid_source = updated_source.clone();
        invalid_source.endpoints.clear();
        assert!(matches!(
            control_plane
                .update_source("source-b", &invalid_source)
                .await,
            Err(ControlPlaneError::Validation(_))
        ));
        let persisted_endpoints = control_plane
            .get_source("source-b")
            .await
            .expect("failed source update rolled back")
            .endpoints;
        assert_eq!(
            persisted_endpoints["openai_chat_completions"],
            "/v1/chat/completions"
        );
        let binding_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM model_bindings WHERE logical_model_id='logical-b-id'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(control_plane
            .create_model_binding(&ModelBindingWrite {
                logical_model_id: "logical-b-id".into(),
                source_id: "source-b".into(),
                account_id: "account-b".into(),
                upstream_model_id: "missing-model".into(),
                protocol: Protocol::OpenAiChatCompletions,
                status: CatalogStatus::Confirmed,
                enabled: true,
                priority: 1,
            })
            .await
            .is_err());
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM model_bindings WHERE logical_model_id='logical-b-id'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            binding_count
        );

        let disabled_route = control_plane
            .set_route_enabled("route-b", false)
            .await
            .expect("disable route");
        assert!(disabled_route
            .snapshot
            .resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b")
            .is_err());
        assert!(stable_snapshot
            .resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b")
            .is_ok());
        let live_resolver = Arc::new(std::sync::RwLock::new(stable_snapshot.resolver.clone()));
        let mut readers = Vec::new();
        for _ in 0..8 {
            let live_resolver = live_resolver.clone();
            readers.push(tokio::spawn(async move {
                for _ in 0..100 {
                    let result = live_resolver
                        .read()
                        .unwrap()
                        .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b");
                    if let Err(error) = result {
                        assert_eq!(error.code, "route_not_found");
                    }
                    tokio::task::yield_now().await;
                }
            }));
        }
        *live_resolver.write().unwrap() = disabled_route.snapshot.resolver.clone();
        for reader in readers {
            reader.await.expect("snapshot reader task");
        }
        control_plane
            .set_route_enabled("route-b", true)
            .await
            .expect("reenable route");

        for snapshot in [
            control_plane
                .set_model_binding_enabled(binding.id, false)
                .await
                .expect("disable binding")
                .snapshot,
            control_plane
                .set_account_enabled("account-b", false)
                .await
                .expect("disable account")
                .snapshot,
            control_plane
                .set_source_enabled("source-b", false)
                .await
                .expect("disable source")
                .snapshot,
            control_plane
                .set_logical_model_enabled("logical-b-id", false)
                .await
                .expect("disable logical model")
                .snapshot,
        ] {
            assert!(snapshot
                .resolver
                .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b")
                .is_err());
        }
        control_plane
            .set_logical_model_enabled("logical-b-id", true)
            .await
            .expect("reenable logical model");
        control_plane
            .set_source_enabled("source-b", true)
            .await
            .expect("reenable source");
        control_plane
            .set_account_enabled("account-b", true)
            .await
            .expect("reenable account");
        let active_snapshot = control_plane
            .set_model_binding_enabled(binding.id, true)
            .await
            .expect("reenable binding")
            .snapshot;

        sqlx::query("UPDATE sources SET endpoints='{}'::jsonb WHERE id='source-b'")
            .execute(&pool)
            .await
            .expect("corrupt candidate endpoint for failed reload test");
        assert!(control_plane.load_snapshot().await.is_err());
        assert!(active_snapshot
            .resolver
            .resolve_detailed(Protocol::OpenAiChatCompletions, "logical-b")
            .is_ok());
        sqlx::query("UPDATE sources SET endpoints='{\"openai_chat_completions\":\"/v1/chat/completions\",\"anthropic_messages\":\"/v1/messages\"}'::jsonb WHERE id='source-b'")
            .execute(&pool)
            .await
            .expect("restore valid endpoint");
        sqlx::query("UPDATE sources SET base_url='http://127.0.0.1:8787' WHERE id='source-b'")
            .execute(&pool)
            .await
            .expect("corrupt Source URL for failed reload test");
        let blocked = match control_plane.load_snapshot().await {
            Ok(_) => panic!("unsafe persisted Source URL must block snapshot publication"),
            Err(error) => error,
        };
        assert!(blocked.message().contains("server Source URL policy"));
        assert!(!blocked.message().contains("127.0.0.1"));
        sqlx::query("UPDATE sources SET base_url='https://source-b.example' WHERE id='source-b'")
            .execute(&pool)
            .await
            .expect("restore valid Source URL");
        let active_snapshot = control_plane
            .load_snapshot()
            .await
            .expect("reload restored snapshot");
        assert!(active_snapshot.revision > stable_snapshot.revision);

        let health = crate::infra::health::HealthRegistry::new(Duration::from_secs(60));
        let active_revision = active_snapshot.revision;
        let state = crate::state::AppState {
            live: Arc::new(std::sync::RwLock::new(
                crate::state::LiveConfig::from_snapshot(active_snapshot),
            )),
            http: crate::http::test_client().expect("HTTP client"),
            db: Some(database.clone()),
            control_plane: Some(control_plane.clone()),
            health: health.clone(),
            admin_auth: crate::state::AdminAuth::test(),
            secrets: crate::infra::secrets::SecretResolver::empty(),
            prometheus_handle: crate::infra::observability::prometheus_handle(),
        };
        state.reload_snapshot(stable_snapshot.clone());
        assert_eq!(state.snapshot().revision, active_revision);

        let capability_response = crate::admin_capabilities_response(true, &state.snapshot());
        assert_eq!(capability_response.status(), StatusCode::OK);
        let capability_body = to_bytes(capability_response.into_body(), 1024 * 1024)
            .await
            .expect("read DB-backed capability matrix response");
        let capability_text = String::from_utf8_lossy(&capability_body);
        assert!(!capability_text.contains("SOURCE_B_API_KEY"));
        assert!(!capability_text.contains("credential_env"));
        let capability_body: Value =
            serde_json::from_slice(&capability_body).expect("parse capability matrix JSON");
        assert_eq!(capability_body["version"], "v1");
        assert_eq!(capability_body["fact_source"], "runtime_snapshot");
        assert_eq!(capability_body["snapshot_revision"], active_revision);
        let capability_row = capability_body["data"]
            .as_array()
            .expect("capability matrix data")
            .iter()
            .find(|row| {
                row["route_id"] == "route-b"
                    && row["model"] == "logical-b"
                    && row["source"]["source_id"] == "source-b"
                    && row["account"]["account_id"] == "account-b"
            })
            .expect("DB runtime route capability row");
        let protocol_cell = |protocol: &str| {
            capability_row["protocols"]
                .as_array()
                .expect("three protocol cells")
                .iter()
                .find(|cell| cell["protocol_in"] == protocol)
                .unwrap_or_else(|| panic!("missing capability cell for {protocol}"))
        };
        let chat_cell = protocol_cell("openai_chat_completions");
        assert_eq!(chat_cell["status"], "routable");
        assert_eq!(chat_cell["mode"], "native");
        assert_eq!(chat_cell["binding_id"], binding.id);
        let responses_cell = protocol_cell("openai_responses");
        assert_eq!(responses_cell["status"], "routable");
        assert_eq!(responses_cell["mode"], "adapter");
        assert_eq!(responses_cell["binding_id"], adapter_binding.id);
        assert_eq!(responses_cell["adapter"], "kimi_responses_adapter");
        assert_eq!(responses_cell["protocol_upstream"], "anthropic_messages");
        assert_eq!(responses_cell["degraded"], true);
        assert_eq!(
            responses_cell["conversion_chain"][0]["protocol_from"],
            "openai_responses"
        );
        assert_eq!(
            responses_cell["conversion_chain"][0]["protocol_to"],
            "anthropic_messages"
        );
        let messages_cell = protocol_cell("anthropic_messages");
        assert_eq!(messages_cell["status"], "unroutable");
        assert!(messages_cell["mode"].is_null());
        assert_eq!(messages_cell["error"]["code"], "route_not_found");

        let model_payload = invoke_models_for_test(&state).await;
        assert!(model_payload["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "logical-b"));
        for _ in 0..health.config().failure_threshold {
            health.mark_failure("account-b").await;
        }
        let model_payload = invoke_models_for_test(&state).await;
        assert!(!model_payload["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "logical-b"));

        let app = crate::application(state);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/accounts")
                    .header(
                        "authorization",
                        format!("Bearer {}", crate::state::TEST_ADMIN_KEY),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("list accounts API response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(!body.contains("credential_ciphertext"));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/sources")
                    .header("content-type", "application/json")
                    .header(
                        "authorization",
                        format!("Bearer {}", crate::state::TEST_ADMIN_KEY),
                    )
                    .body(Body::from(
                        json!({
                            "id":"source-api",
                            "display_name":"Source API",
                            "provider_preset_id":"custom",
                            "base_url":"https://source-api.example",
                            "endpoints":{"openai_chat_completions":"/v1/chat/completions"}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .expect("create Source through merged control-plane API");
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body["snapshot_revision"].as_i64().unwrap() > active_revision);
        control_plane
            .delete_source("source-api")
            .await
            .expect("delete Source created through API");

        control_plane
            .delete_route("route-b")
            .await
            .expect("delete route");
        control_plane
            .delete_model_binding(binding.id)
            .await
            .expect("delete binding");
        control_plane
            .delete_model_binding(adapter_binding.id)
            .await
            .expect("delete adapter binding");
        control_plane
            .delete_logical_model("logical-b-id")
            .await
            .expect("delete logical model");
        control_plane
            .delete_account("account-b")
            .await
            .expect("delete account");
        control_plane
            .delete_source("source-b")
            .await
            .expect("delete source");
        assert!(matches!(
            control_plane.get_source("source-b").await,
            Err(ControlPlaneError::NotFound(_))
        ));

        drop(control_plane);
        drop(database);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop isolated test schema");
        admin.close().await;
    }
}
