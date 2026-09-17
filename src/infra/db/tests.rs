use super::usage::filter_sql;
use super::*;
use crate::{
    control_plane::model_catalog::{install_builtin_presets, ModelCatalogRepository},
    domain::{
        catalog::{
            CapabilitySupport, CatalogMetadata, CatalogStatus, LogicalModelInput, MetadataField,
            MetadataSource, MetadataValues, ModelBindingInput, ModelPresetInput, ModelPresetRef,
            ProviderPresetInput, SourceInput, SourceModelCapabilityInput, SourceModelRefresh,
            SourceProtocolMode,
        },
        protocol::Protocol,
        provider_preset::SourceAuthConfig,
    },
};
use serde_json::json;
use sqlx::postgres::PgConnectOptions;
use std::{collections::BTreeMap, str::FromStr};

#[test]
fn filters_support_combined_dimensions_and_utc_bounds() {
    let filter = UsageFilter {
        from: Some("2026-01-01T00:00:00Z".parse().unwrap()),
        to: Some("2026-01-02T00:00:00Z".parse().unwrap()),
        logical_model: Some("m".into()),
        upstream_model_id: Some("upstream-m".into()),
        provider_id: Some("p".into()),
        source_id: Some("source-a".into()),
        client_source: Some("cli".into()),
        account_id: Some("a".into()),
        protocol_in: Some("openai_chat_completions".into()),
        protocol_upstream: Some("anthropic_messages".into()),
        virtual_key_id: Some(7),
        success: Some(false),
        status_code: Some(429),
        usage_source: Some("estimated".into()),
    };
    let (sql, binds) = filter_sql(&filter);
    assert!(sql.contains("created_at >= $1"));
    assert!(sql.contains("logical_model = $3"));
    assert!(sql.contains("source_id = $6"));
    assert!(sql.contains("client_source = $7"));
    assert!(sql.contains("protocol_upstream = $10"));
    assert!(sql.contains("virtual_key_id = $12"));
    assert!(sql.contains("status_code = $14"));
    assert_eq!(binds.len(), 14);
}

#[test]
fn cursor_round_trip_preserves_tie_breaker() {
    let cursor = UsageCursor {
        created_at: "2026-01-01T00:00:00.123456Z".parse().unwrap(),
        request_id: "request:with:colons".into(),
    };
    assert_eq!(UsageCursor::decode(&cursor.encode()), Some(cursor));
    assert!(UsageCursor::decode("not-a-cursor").is_none());
}

#[test]
fn initial_schema_keeps_logical_request_and_attempt_idempotency() {
    let schema = include_str!("../../../migrations/0001_init.sql");
    assert!(schema.contains("logical_model TEXT NOT NULL"));
    assert!(schema.contains("UNIQUE (request_id, attempt_no)"));
    assert!(schema.contains("request_id TEXT NOT NULL UNIQUE"));
    let query_schema = include_str!("../../../migrations/0004_usage_query_contract.sql");
    assert!(query_schema.contains("virtual_key_id BIGINT"));
    assert!(query_schema.contains("created_at DESC, request_id DESC"));
    let fields_schema = include_str!("../../../migrations/0005_usage_event_fields.sql");
    assert!(fields_schema.contains("route_id TEXT"));
    assert!(fields_schema.contains("streamed BOOLEAN"));
    assert!(fields_schema.contains("error_summary TEXT"));
    let source_schema = include_str!("../../../migrations/0009_usage_source_dimensions.sql");
    assert!(source_schema.contains("RENAME COLUMN source TO client_source"));
    assert!(source_schema.contains("ADD COLUMN IF NOT EXISTS source_id TEXT"));
    assert!(!source_schema.contains("REFERENCES sources"));
    let provider_schema = include_str!("../../../migrations/0010_usage_provider_attribution.sql");
    assert!(provider_schema.contains("source.provider_preset_id"));
    assert!(provider_schema.contains("provider_id = event.source_id"));
    assert!(provider_schema.contains("provider_id = attempt.source_id"));
    assert!(provider_schema.contains("'unknown'"));
    let health_schema = include_str!("../../../migrations/0012_health_persistence.sql");
    for marker in [
        "health_source TEXT",
        "health_updated_at TIMESTAMPTZ",
        "consecutive_failures INTEGER",
        "CREATE TABLE IF NOT EXISTS account_health_events",
        "VALUES (12, 'health_persistence')",
    ] {
        assert!(
            health_schema.contains(marker),
            "missing health marker: {marker}"
        );
    }
}

#[tokio::test]
async fn postgres_migrates_legacy_client_source_without_inventing_runtime_source() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL usage migration test: TEST_DATABASE_URL is not set");
        return;
    };
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect PostgreSQL migration test admin database");
    let schema = format!("usage_source_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
        .execute(&admin)
        .await
        .expect("create isolated usage migration schema");
    let options = PgConnectOptions::from_str(&url)
        .expect("parse TEST_DATABASE_URL")
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("connect isolated usage migration schema");

    sqlx::raw_sql(include_str!("../../../migrations/0001_init.sql"))
        .execute(&pool)
        .await
        .expect("apply legacy usage schema");
    sqlx::query(
        "CREATE INDEX idx_usage_events_source_created_at ON usage_events (source, created_at DESC)",
    )
    .execute(&pool)
    .await
    .expect("create legacy source index");
    sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,source,protocol_in,protocol_upstream,mode,status_code,success) VALUES ('legacy-request','legacy-provider','legacy-account','legacy-model','legacy-model','legacy-cli','openai_responses','openai_responses','native',200,TRUE)")
        .execute(&pool)
        .await
        .expect("insert legacy usage event");
    sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,account_id,status_code,success) VALUES ('legacy-request',0,'legacy-provider','legacy-account',200,TRUE)")
        .execute(&pool)
        .await
        .expect("insert legacy usage attempt");

    sqlx::raw_sql(include_str!(
        "../../../migrations/0009_usage_source_dimensions.sql"
    ))
    .execute(&pool)
    .await
    .expect("apply Source dimension migration");
    // The gateway embeds idempotent scripts and replays them at startup.
    sqlx::raw_sql(include_str!(
        "../../../migrations/0004_usage_query_contract.sql"
    ))
    .execute(&pool)
    .await
    .expect("replay earlier usage indexes after migration");
    sqlx::raw_sql(include_str!(
        "../../../migrations/0009_usage_source_dimensions.sql"
    ))
    .execute(&pool)
    .await
    .expect("replay Source dimension migration");

    let event: (Option<String>, String) = sqlx::query_as(
        "SELECT source_id,client_source FROM usage_events WHERE request_id='legacy-request'",
    )
    .fetch_one(&pool)
    .await
    .expect("read migrated usage event");
    assert_eq!(event, (None, "legacy-cli".into()));
    let attempt_source: Option<String> = sqlx::query_scalar(
        "SELECT source_id FROM usage_event_attempts WHERE request_id='legacy-request'",
    )
    .fetch_one(&pool)
    .await
    .expect("read migrated usage attempt");
    assert_eq!(attempt_source, None);
    let legacy_column_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema=current_schema() AND table_name='usage_events' AND column_name='source')",
    )
    .fetch_one(&pool)
    .await
    .expect("inspect migrated usage columns");
    assert!(!legacy_column_exists);

    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
        .execute(&admin)
        .await
        .expect("drop isolated usage migration schema");
    admin.close().await;
}

#[tokio::test]
async fn postgres_repairs_db_first_provider_attribution_without_guessing_deleted_sources() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!(
            "skipping PostgreSQL provider attribution migration test: TEST_DATABASE_URL is not set"
        );
        return;
    };
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect PostgreSQL provider attribution migration admin database");
    let schema = format!("usage_provider_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
        .execute(&admin)
        .await
        .expect("create isolated provider attribution migration schema");
    let options = PgConnectOptions::from_str(&url)
        .expect("parse TEST_DATABASE_URL")
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("connect isolated provider attribution migration schema");
    let database = Database::from_test_pool(pool.clone())
        .await
        .expect("apply migrations in provider attribution schema");

    sqlx::query("INSERT INTO provider_presets (id,version,display_name,definition) VALUES ('provider-a',1,'Provider A','{}'::jsonb)")
        .execute(&pool)
        .await
        .expect("insert provider preset fixture");
    sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url) VALUES ('source-a','Source A','provider-a',1,'{}'::jsonb,'https://source-a.example'),('source-b','Source B','provider-a',1,'{}'::jsonb,'https://source-b.example')")
        .execute(&pool)
        .await
        .expect("insert Source fixtures");
    sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success) VALUES ('mapped-a','source-a','account-a','model','model','source-a','test','openai_responses','openai_responses','native',200,TRUE),('mapped-b','source-b','account-b','model','model','source-b','test','openai_responses','openai_responses','native',200,TRUE),('deleted','deleted-source','account-deleted','model','model','deleted-source','test','openai_responses','openai_responses','native',200,TRUE),('legacy','legacy-provider','legacy-account','model','model',NULL,'test','openai_responses','openai_responses','native',200,TRUE)")
        .execute(&pool)
        .await
        .expect("insert provider attribution event fixtures");
    sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,source_id,account_id,status_code,success) VALUES ('mapped-a',0,'source-a','source-a','account-a',200,TRUE),('mapped-b',0,'source-b','source-b','account-b',200,TRUE),('deleted',0,'deleted-source','deleted-source','account-deleted',200,TRUE),('legacy',0,'legacy-provider',NULL,'legacy-account',200,TRUE)")
        .execute(&pool)
        .await
        .expect("insert provider attribution attempt fixtures");

    for _ in 0..2 {
        sqlx::raw_sql(include_str!(
            "../../../migrations/0010_usage_provider_attribution.sql"
        ))
        .execute(&pool)
        .await
        .expect("replay provider attribution migration");
    }

    let events: Vec<(String, String)> =
        sqlx::query_as("SELECT request_id,provider_id FROM usage_events ORDER BY request_id")
            .fetch_all(&pool)
            .await
            .expect("query repaired provider event attribution");
    assert_eq!(
        events,
        vec![
            ("deleted".into(), "unknown".into()),
            ("legacy".into(), "legacy-provider".into()),
            ("mapped-a".into(), "provider-a".into()),
            ("mapped-b".into(), "provider-a".into()),
        ]
    );
    let attempts: Vec<(String, String)> = sqlx::query_as(
        "SELECT request_id,provider_id FROM usage_event_attempts ORDER BY request_id",
    )
    .fetch_all(&pool)
    .await
    .expect("query repaired provider attempt attribution");
    assert_eq!(attempts, events);

    drop(database);
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
        .execute(&admin)
        .await
        .expect("drop isolated provider attribution migration schema");
    admin.close().await;
}

async fn postgres_test_database() -> Option<Database> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("connect TEST_DATABASE_URL");
    let database = Database::from_pool(pool);
    database.migrate().await.expect("apply test migrations");
    Some(database)
}

#[tokio::test]
async fn postgres_repairs_only_empty_builtin_source_auth_snapshots() {
    let Some(database) = postgres_test_database().await else {
        eprintln!("skipping PostgreSQL Source auth migration test: TEST_DATABASE_URL is not set");
        return;
    };
    let repository = ModelCatalogRepository::new(database.pool.clone());
    install_builtin_presets(&repository)
        .await
        .expect("install built-in ProviderPresets");
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let empty_source = format!("migration-empty-auth-{suffix}");
    let override_source = format!("migration-override-auth-{suffix}");
    let endpoints = json!({"openai_chat_completions":"/custom/chat/completions"});
    let capabilities = json!({
        "openai_chat_completions": {"mode":"native"}
    });
    let explicit_auth = json!({
        "credential_header": {"header":"x-api-key","prefix":""},
        "default_headers": {"x-source-override":"true"}
    });
    for (source_id, auth_config) in [
        (empty_source.as_str(), json!({})),
        (override_source.as_str(), explicit_auth.clone()),
    ] {
        sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities) SELECT $1,$1,'minimax',3,definition,'https://example.invalid',$2,$3,$4 FROM provider_presets WHERE id='minimax' AND version=3")
            .bind(source_id)
            .bind(&endpoints)
            .bind(&auth_config)
            .bind(&capabilities)
            .execute(&database.pool)
            .await
            .expect("seed Source auth migration fixture");
    }

    sqlx::raw_sql(include_str!(
        "../../../migrations/0021_repair_builtin_source_auth_snapshots.sql"
    ))
    .execute(&database.pool)
    .await
    .expect("replay Source auth snapshot repair");

    let repaired: (serde_json::Value, serde_json::Value, serde_json::Value) = sqlx::query_as(
        "SELECT auth_config,endpoints,protocol_capabilities FROM sources WHERE id=$1",
    )
    .bind(&empty_source)
    .fetch_one(&database.pool)
    .await
    .expect("read repaired Source");
    let repaired_auth: SourceAuthConfig =
        serde_json::from_value(repaired.0).expect("repaired auth snapshot deserializes");
    assert_eq!(repaired_auth.credential_header.header, "authorization");
    assert_eq!(repaired_auth.credential_header.prefix, "Bearer ");
    assert_eq!(repaired.1, endpoints);
    assert_eq!(repaired.2, capabilities);

    let preserved_auth: serde_json::Value =
        sqlx::query_scalar("SELECT auth_config FROM sources WHERE id=$1")
            .bind(&override_source)
            .fetch_one(&database.pool)
            .await
            .expect("read Source auth override");
    assert_eq!(preserved_auth, explicit_auth);

    sqlx::query("DELETE FROM sources WHERE id = ANY($1)")
        .bind(vec![empty_source, override_source])
        .execute(&database.pool)
        .await
        .expect("clean Source auth migration fixtures");
}

#[tokio::test]
async fn postgres_migrates_kimi_sources_to_native_responses() {
    let Some(database) = postgres_test_database().await else {
        eprintln!(
            "skipping PostgreSQL Kimi native Responses migration test: TEST_DATABASE_URL is not set"
        );
        return;
    };
    let repository = ModelCatalogRepository::new(database.pool.clone());
    install_builtin_presets(&repository)
        .await
        .expect("install built-in ProviderPresets");
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let source_id = format!("migration-kimi-native-{suffix}");

    // Seed a kimi_code@3 Source as it existed before #157: Responses
    // routed through the embedded adapter targeting /v1/messages.
    sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities) SELECT $1,$1,'kimi_code',3,definition,'https://api.kimi.com/coding',$2,'{}'::jsonb,$3 FROM provider_presets WHERE id='kimi_code' AND version=3")
        .bind(&source_id)
        .bind(json!({
            "openai_chat_completions": "/v1/chat/completions",
            "openai_responses": "/v1/messages",
            "anthropic_messages": "/v1/messages"
        }))
        .bind(json!({
            "openai_chat_completions": {"mode":"native"},
            "openai_responses": {"mode":"adapter","source_protocol":"anthropic_messages","adapter":"kimi_responses_adapter","features":{"streaming":"translated","usage":"translated"}},
            "anthropic_messages": {"mode":"native"}
        }))
        .execute(&database.pool)
        .await
        .expect("seed kimi_code@3 Source fixture");
    sqlx::query("INSERT INTO source_models (source_id,upstream_model_id,confirmation_status,availability_status,confirmed_at) VALUES ($1,'k3','confirmed','available',NOW())")
        .bind(&source_id)
        .execute(&database.pool)
        .await
        .expect("seed SourceModel fixture");
    sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,source_protocol,adapter,feature_capabilities,field_source,confirmed_at) VALUES ($1,'k3','openai_responses','confirmed','adapter','anthropic_messages','kimi_responses_adapter','{\"streaming\":\"supported\",\"tools\":\"supported\",\"usage\":\"supported\"}'::jsonb,'user',NOW()),($1,'k3','anthropic_messages','confirmed','native',NULL,NULL,'{\"streaming\":\"supported\"}'::jsonb,'user',NOW())")
        .bind(&source_id)
        .execute(&database.pool)
        .await
        .expect("seed capability fixtures");

    // migrate() already applied 0023 before the fixture existed; clear the
    // marker and replay it against the pre-#157 rows.
    sqlx::query("DELETE FROM gateway_schema_migrations WHERE version=23")
        .execute(&database.pool)
        .await
        .expect("clear migration marker");
    sqlx::raw_sql(include_str!(
        "../../../migrations/0023_kimi_native_responses.sql"
    ))
    .execute(&database.pool)
    .await
    .expect("replay Kimi native Responses migration");

    let kimi_v4 = crate::domain::provider_preset::builtin_provider_presets()
        .expect("builtin presets")
        .into_iter()
        .find(|preset| preset.id == "kimi_code" && preset.version == 4)
        .expect("kimi_code@4 preset");
    let (version, snapshot, endpoints, protocol_capabilities): (
        i32,
        Value,
        Value,
        Value,
    ) = sqlx::query_as("SELECT provider_preset_version,provider_preset_snapshot,endpoints,protocol_capabilities FROM sources WHERE id=$1")
        .bind(&source_id)
        .fetch_one(&database.pool)
        .await
        .expect("read migrated Source");
    assert_eq!(version, 4);
    assert_eq!(snapshot, kimi_v4.definition);
    assert_eq!(endpoints["openai_responses"], "/v1/responses");
    // Other per-Source endpoint overrides are preserved.
    assert_eq!(endpoints["openai_chat_completions"], "/v1/chat/completions");
    assert_eq!(endpoints["anthropic_messages"], "/v1/messages");
    assert_eq!(protocol_capabilities["openai_responses"]["mode"], "native");
    assert!(protocol_capabilities["openai_responses"]["adapter"].is_null());
    assert!(protocol_capabilities["openai_responses"]["source_protocol"].is_null());
    assert_eq!(
        protocol_capabilities["openai_responses"]["features"]["web_search"],
        "supported"
    );
    assert_eq!(
        protocol_capabilities["openai_responses"]["features"]["web_search_citations"],
        "unknown"
    );

    let (mode, source_protocol, adapter, status): (
        String,
        Option<String>,
        Option<String>,
        String,
    ) = sqlx::query_as("SELECT mode::text,source_protocol::text,adapter,status::text FROM source_model_capabilities WHERE source_id=$1 AND upstream_model_id='k3' AND protocol='openai_responses'")
        .bind(&source_id)
        .fetch_one(&database.pool)
        .await
        .expect("read migrated capability row");
    assert_eq!(mode, "native");
    assert_eq!(source_protocol, None);
    assert_eq!(adapter, None);
    // Confirmed status survives the protocol-chain switch.
    assert_eq!(status, "confirmed");

    // The migration is guarded: a replay must not stomp Source edits made
    // after the first run.
    sqlx::query("UPDATE sources SET endpoints = jsonb_set(endpoints, '{openai_responses}', '\"/custom/responses\"') WHERE id=$1")
        .bind(&source_id)
        .execute(&database.pool)
        .await
        .expect("customize migrated endpoint");
    sqlx::raw_sql(include_str!(
        "../../../migrations/0023_kimi_native_responses.sql"
    ))
    .execute(&database.pool)
    .await
    .expect("replay migration is a no-op");
    let preserved: Value = sqlx::query_scalar("SELECT endpoints FROM sources WHERE id=$1")
        .bind(&source_id)
        .fetch_one(&database.pool)
        .await
        .expect("read customized endpoint");
    assert_eq!(preserved["openai_responses"], "/custom/responses");

    sqlx::query("DELETE FROM sources WHERE id=$1")
        .bind(&source_id)
        .execute(&database.pool)
        .await
        .expect("clean Kimi native migration fixtures");
}

#[tokio::test]
async fn postgres_migrates_kimi_discovery_and_default_names_without_overwriting_customizations() {
    let Some(database) = postgres_test_database().await else {
        eprintln!("skipping Kimi discovery migration test: TEST_DATABASE_URL is not set");
        return;
    };
    // Also checks that the SQL-installed v5 matches the immutable Rust preset.
    install_builtin_presets(&ModelCatalogRepository::new(database.pool.clone()))
        .await
        .expect("migration and builtin presets agree");
    let latest = crate::domain::provider_preset::builtin_provider_presets()
        .unwrap()
        .into_iter()
        .find(|preset| preset.id == "kimi_code" && preset.version == 5)
        .unwrap();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let mut tx = database.pool.begin().await.unwrap();
    sqlx::query("DELETE FROM gateway_schema_migrations WHERE version=26")
        .execute(&mut *tx)
        .await
        .unwrap();

    let mut fixtures = Vec::new();
    for (kind, provider, version, name) in [
        ("default", "kimi_code", 4, "Kimi Code"),
        ("custom", "kimi_code", 4, "Personal Kimi"),
        ("other", "deepseek", 3, "Kimi Code"),
    ] {
        let source_id = format!("kimi-discovery-{kind}-{suffix}");
        sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities) SELECT $1,$2,id,version,jsonb_set(definition,'{default_headers,x-custom}','\"preserved\"'),'https://example.invalid/coding',$5,$6,$7 FROM provider_presets WHERE id=$3 AND version=$4")
            .bind(&source_id)
            .bind(name)
            .bind(provider)
            .bind(version)
            .bind(json!({"openai_responses":"/custom/responses"}))
            .bind(json!({"credential_header":{"header":"x-api-key","prefix":""}}))
            .bind(json!({"openai_responses":{"mode":"native","features":{"tools":"unknown"}}}))
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO accounts (id,source_id,display_name,credential_env) VALUES ($1,$1,$2,'KIMI_MIGRATION_TEST_KEY')")
            .bind(&source_id)
            .bind(name)
            .execute(&mut *tx)
            .await
            .unwrap();
        let before: Value = sqlx::query_scalar(
            "SELECT to_jsonb(source)-'updated_at' FROM sources source WHERE id=$1",
        )
        .bind(&source_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        fixtures.push((source_id, provider, name, before));
    }

    let migration = include_str!("../../../migrations/0026_kimi_code_cn_discovery.sql");
    sqlx::raw_sql(migration).execute(&mut *tx).await.unwrap();
    for (source_id, provider, name, mut expected) in fixtures {
        let expected_name = if provider == "kimi_code" && name == "Kimi Code" {
            "Kimi Code CN"
        } else {
            name
        };
        if provider == "kimi_code" {
            expected["provider_preset_version"] = json!(5);
            expected["provider_preset_snapshot"]["discovery"] =
                latest.definition["discovery"].clone();
            expected["display_name"] = json!(expected_name);
        }
        let after: Value = sqlx::query_scalar(
            "SELECT to_jsonb(source)-'updated_at' FROM sources source WHERE id=$1",
        )
        .bind(&source_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(after, expected);
        let account_name: String =
            sqlx::query_scalar("SELECT display_name FROM accounts WHERE id=$1")
                .bind(&source_id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(account_name, expected_name);
    }

    let source_id = format!("kimi-discovery-default-{suffix}");
    sqlx::query("UPDATE sources SET display_name='Kimi Code',provider_preset_snapshot=jsonb_set(provider_preset_snapshot,'{discovery,endpoint}','\"/custom/models\"') WHERE id=$1")
        .bind(&source_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql(migration).execute(&mut *tx).await.unwrap();
    let (name, endpoint): (String, String) = sqlx::query_as("SELECT display_name,provider_preset_snapshot#>>'{discovery,endpoint}' FROM sources WHERE id=$1")
        .bind(&source_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(name, "Kimi Code");
    assert_eq!(endpoint, "/custom/models");
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn postgres_repairs_multimodal_model_metadata_and_capabilities() {
    let Some(database) = postgres_test_database().await else {
        eprintln!("skipping multimodal capability migration test: TEST_DATABASE_URL is not set");
        return;
    };
    // The SQL-installed v2 rows and the immutable Rust presets must agree.
    install_builtin_presets(&ModelCatalogRepository::new(database.pool.clone()))
        .await
        .expect("migration and multimodal model presets agree");

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let deepseek_source = format!("multimodal-deepseek-{suffix}");
    let kimi_source = format!("multimodal-kimi-{suffix}");
    let mut tx = database.pool.begin().await.unwrap();
    sqlx::query("DELETE FROM gateway_schema_migrations WHERE version=27")
        .execute(&mut *tx)
        .await
        .unwrap();

    for (source_id, provider, version) in [
        (&deepseek_source, "deepseek", 3),
        (&kimi_source, "kimi_code", 5),
    ] {
        sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url,endpoints,auth_config,protocol_capabilities) SELECT $1,$1,id,version,definition,'https://example.invalid','{}'::jsonb,'{}'::jsonb,'{}'::jsonb FROM provider_presets WHERE id=$2 AND version=$3")
            .bind(source_id)
            .bind(provider)
            .bind(version)
            .execute(&mut *tx)
            .await
            .unwrap();
    }

    for (source_id, model_id) in [
        (&deepseek_source, "deepseek-flash"),
        (&kimi_source, "k3"),
        (&kimi_source, "k3-256k"),
        (&kimi_source, "kimi-for-coding"),
        (&kimi_source, "kimi-for-coding-highspeed"),
    ] {
        sqlx::query("INSERT INTO source_models (source_id,upstream_model_id,confirmation_status,availability_status,raw_snapshot,metadata,field_sources,confirmed_at) VALUES ($1,$2,'confirmed','available','{}'::jsonb,$3,$4,NOW())")
            .bind(source_id)
            .bind(model_id)
            .bind(json!({
                "display_name": model_id,
                "context_window": 1,
                "input_modalities": ["text"]
            }))
            .bind(json!({
                "display_name": "preset",
                "context_window": "preset",
                "input_modalities": "preset"
            }))
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO source_model_capabilities (source_id,upstream_model_id,protocol,status,mode,feature_capabilities,field_source,confirmed_at) VALUES ($1,$2,'openai_chat_completions','confirmed','native','{\"vision\":\"unsupported\"}'::jsonb,'user',NOW())")
            .bind(source_id)
            .bind(model_id)
            .execute(&mut *tx)
            .await
            .unwrap();
    }

    let migration = include_str!("../../../migrations/0027_multimodal_model_capabilities.sql");
    sqlx::raw_sql(migration).execute(&mut *tx).await.unwrap();

    let rows: Vec<(String, i32, Value, Value)> = sqlx::query_as(
        "SELECT upstream_model_id,matched_model_preset_version,metadata,field_sources FROM source_models WHERE source_id IN ($1,$2) ORDER BY upstream_model_id",
    )
    .bind(&deepseek_source)
    .bind(&kimi_source)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(rows.len(), 5);
    for (model_id, version, metadata, field_sources) in rows {
        assert_eq!(version, 2, "{model_id} must move to ModelPreset v2");
        assert!(metadata["input_modalities"]
            .as_array()
            .is_some_and(|modalities| modalities.iter().any(|value| value == "image")));
        assert_eq!(field_sources["input_modalities"], "preset");
        let expected_context =
            if matches!(model_id.as_str(), "k3-256k" | "kimi-for-coding-highspeed") {
                262_144
            } else {
                1_048_576
            };
        assert_eq!(metadata["context_window"], expected_context);
    }

    let visions: Vec<String> = sqlx::query_scalar(
        "SELECT feature_capabilities->>'vision' FROM source_model_capabilities WHERE source_id IN ($1,$2) ORDER BY upstream_model_id",
    )
    .bind(&deepseek_source)
    .bind(&kimi_source)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(visions, vec!["supported"; 5]);

    // Replaying after the migration marker is present must preserve later edits.
    sqlx::query("UPDATE source_model_capabilities SET feature_capabilities=jsonb_set(feature_capabilities,'{vision}','\"unsupported\"') WHERE source_id=$1 AND upstream_model_id='k3'")
        .bind(&kimi_source)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql(migration).execute(&mut *tx).await.unwrap();
    let replayed: String = sqlx::query_scalar("SELECT feature_capabilities->>'vision' FROM source_model_capabilities WHERE source_id=$1 AND upstream_model_id='k3' AND protocol='openai_chat_completions'")
        .bind(&kimi_source)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(replayed, "unsupported");
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn postgres_queries_keep_logical_attempt_and_utc_boundary_semantics() {
    let Some(database) = postgres_test_database().await else {
        eprintln!("skipping PostgreSQL usage query test: TEST_DATABASE_URL is not set");
        return;
    };
    let prefix = format!("usage-contract-{}-", uuid::Uuid::new_v4());
    let logical_model = format!("logical-{prefix}");
    let empty_filter = UsageFilter {
        logical_model: Some(format!("missing-{prefix}")),
        ..Default::default()
    };
    let empty = database
        .usage_aggregate(&empty_filter)
        .await
        .expect("empty summary");
    assert_eq!(empty.logical_requests, 0);
    assert_eq!(empty.upstream_attempts, 0);
    assert_eq!(empty.total_tokens, 0);
    assert!(database
        .usage_timeseries(&empty_filter, "hour")
        .await
        .expect("empty timeseries")
        .is_empty());
    assert!(database
        .usage_breakdown(&empty_filter, "logical_model")
        .await
        .expect("empty breakdown")
        .is_empty());
    assert!(database
        .list_usage_events_page(&empty_filter, 100, None)
        .await
        .expect("empty event page")
        .data
        .is_empty());
    let (virtual_key_id, virtual_key) = database
        .create_virtual_key(&format!("key-{prefix}"), &[])
        .await
        .expect("create virtual key fixture");
    assert_eq!(
        database
            .authenticate_virtual_key(&virtual_key, Some(&logical_model))
            .await
            .expect("authenticate virtual key fixture"),
        Some(virtual_key_id)
    );
    let fixtures = [
        ("a", "2026-01-01T00:00:00Z", true, "upstream", 10_i64, 1_i32),
        (
            "b",
            "2026-01-01T23:59:59.999999Z",
            false,
            "missing",
            0_i64,
            0_i32,
        ),
        (
            "c",
            "2026-01-02T00:00:00Z",
            true,
            "estimated",
            30_i64,
            0_i32,
        ),
    ];
    for (suffix, created_at, success, usage_source, tokens, retry_count) in fixtures {
        let request_id = format!("{prefix}{suffix}");
        sqlx::query("INSERT INTO usage_events (request_id,virtual_key_id,provider_id,account_id,model,logical_model,upstream_model_id,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,input_tokens,output_tokens,total_tokens,usage_source,created_at) VALUES ($1,$2,'provider-a','account-a',$3,$3,'upstream-a','source-a','test','openai_responses','anthropic_messages','adapter',$4,$5,$6,25,$7,0,$7,$8,$9)")
            .bind(&request_id)
            .bind(virtual_key_id)
            .bind(&logical_model)
            .bind(if success { 200 } else { 429 })
            .bind(success)
            .bind(retry_count)
            .bind(tokens)
            .bind(usage_source)
            .bind(created_at.parse::<DateTime<Utc>>().unwrap())
            .execute(&database.pool)
            .await
            .expect("insert usage fixture");
        for attempt_no in 0..=retry_count {
            sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,source_id,account_id,status_code,success,latency_ms) VALUES ($1,$2,'provider-a','source-a','account-a',$3,$4,10)")
                .bind(&request_id)
                .bind(attempt_no)
                .bind(if success { 200 } else { 429 })
                .bind(success)
                .execute(&database.pool)
                .await
                .expect("insert attempt fixture");
        }
    }
    let filter = UsageFilter {
        from: Some("2026-01-01T00:00:00Z".parse().unwrap()),
        to: Some("2026-01-02T00:00:00Z".parse().unwrap()),
        logical_model: Some(logical_model.clone()),
        upstream_model_id: Some("upstream-a".into()),
        provider_id: Some("provider-a".into()),
        source_id: Some("source-a".into()),
        client_source: Some("test".into()),
        account_id: Some("account-a".into()),
        protocol_in: Some("openai_responses".into()),
        protocol_upstream: Some("anthropic_messages".into()),
        virtual_key_id: Some(virtual_key_id),
        ..Default::default()
    };
    let summary = database.usage_aggregate(&filter).await.expect("summary");
    assert_eq!(summary.logical_requests, 2);
    assert_eq!(summary.upstream_attempts, 3);
    assert_eq!(summary.retries, 1);
    assert_eq!(summary.successes, 1);
    assert_eq!(summary.failures, 1);
    assert_eq!(summary.total_tokens, 10);
    let mut failed_filter = filter.clone();
    failed_filter.success = Some(false);
    failed_filter.status_code = Some(429);
    failed_filter.usage_source = Some("missing".into());
    let failed = database
        .usage_aggregate(&failed_filter)
        .await
        .expect("failure and missing-usage filter");
    assert_eq!(failed.logical_requests, 1);
    assert_eq!(failed.failures, 1);
    assert_eq!(failed.total_tokens, 0);
    let timeseries = database
        .usage_timeseries(&filter, "day")
        .await
        .expect("timeseries");
    assert_eq!(timeseries.len(), 1);
    assert_eq!(timeseries[0].upstream_attempts, 3);
    let breakdown = database
        .usage_breakdown(&filter, "usage_source")
        .await
        .expect("breakdown");
    assert_eq!(breakdown.len(), 2);
    let source_breakdown = database
        .usage_breakdown(&filter, "source_id")
        .await
        .expect("Source breakdown");
    assert_eq!(source_breakdown[0].key.as_deref(), Some("source-a"));
    let client_source_breakdown = database
        .usage_breakdown(&filter, "client_source")
        .await
        .expect("Client Source breakdown");
    assert_eq!(client_source_breakdown[0].key.as_deref(), Some("test"));
    let exported = database
        .export_usage_events(&filter, 10_000)
        .await
        .expect("export");
    assert_eq!(exported.len(), 2);
    sqlx::query("DELETE FROM usage_events WHERE request_id LIKE $1")
        .bind(format!("{prefix}%"))
        .execute(&database.pool)
        .await
        .expect("clean usage fixtures");
    sqlx::query("DELETE FROM virtual_keys WHERE id=$1")
        .bind(virtual_key_id)
        .execute(&database.pool)
        .await
        .expect("clean virtual key fixture");
}

#[tokio::test]
async fn postgres_parsed_usage_source_is_filterable() {
    let Some(database) = postgres_test_database().await else {
        eprintln!("skipping PostgreSQL parsed usage test: TEST_DATABASE_URL is not set");
        return;
    };
    let prefix = format!("usage-parsed-{}-", uuid::Uuid::new_v4());
    let logical_model = format!("logical-{prefix}");
    let event = UsageEvent {
        request_id: format!("{prefix}request"),
        virtual_key_id: None,
        provider_id: "provider-parsed".into(),
        account_id: "account-parsed".into(),
        model: logical_model.clone(),
        logical_model: logical_model.clone(),
        upstream_model_id: Some("upstream-parsed".into()),
        source_id: "source-parsed".into(),
        client_source: "test".into(),
        protocol_in: "openai_responses".into(),
        protocol_upstream: "anthropic_messages".into(),
        mode: "adapter".into(),
        status_code: 200,
        success: true,
        retry_count: 0,
        latency_ms: 12,
        ttft_ms: None,
        input_tokens: 2,
        output_tokens: 3,
        reasoning_tokens: 0,
        cached_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        total_tokens: 5,
        usage_source: "parsed".into(),
        degraded: false,
        route_id: Some("route-parsed".into()),
        streamed: true,
        error_summary: None,
        fallback_reason: Some("upstream_http_429".into()),
    };
    database
        .insert_usage(&event)
        .await
        .expect("insert parsed usage fixture");

    let filter = UsageFilter {
        logical_model: Some(logical_model),
        usage_source: Some("parsed".into()),
        ..Default::default()
    };
    let page = database
        .list_usage_events_page(&filter, 10, None)
        .await
        .expect("query parsed usage events");
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].usage_source, "parsed");
    assert_eq!(
        page.data[0].fallback_reason.as_deref(),
        Some("upstream_http_429")
    );
    let aggregate = database
        .usage_aggregate(&filter)
        .await
        .expect("aggregate parsed usage events");
    assert_eq!(aggregate.logical_requests, 1);
    assert_eq!(aggregate.total_tokens, 5);
    let breakdown = database
        .usage_breakdown(&filter, "usage_source")
        .await
        .expect("break down parsed usage events");
    assert_eq!(breakdown.len(), 1);
    assert_eq!(breakdown[0].key.as_deref(), Some("parsed"));

    database
        .delete_usage_events_for_test(&prefix)
        .await
        .expect("clean parsed usage fixture");
}

#[tokio::test]
async fn postgres_source_deletion_preserves_usage_history() {
    let Some(database) = postgres_test_database().await else {
        eprintln!("skipping PostgreSQL Source history test: TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = uuid::Uuid::new_v4().to_string();
    let source_id = format!("deleted-source-{suffix}");
    let request_id = format!("deleted-source-request-{suffix}");
    sqlx::query("INSERT INTO sources (id,display_name,provider_preset_id,provider_preset_version,provider_preset_snapshot,base_url) VALUES ($1,$2,'custom',1,'{}'::jsonb,'https://deleted-source.example')")
        .bind(&source_id)
        .bind(format!("Deleted Source {suffix}"))
        .execute(&database.pool)
        .await
        .expect("insert Source history fixture");
    let event = UsageEvent {
        request_id: request_id.clone(),
        virtual_key_id: None,
        provider_id: source_id.clone(),
        account_id: format!("deleted-account-{suffix}"),
        model: "history-model".into(),
        logical_model: "history-model".into(),
        upstream_model_id: Some("history-upstream".into()),
        source_id: source_id.clone(),
        client_source: "history-client".into(),
        protocol_in: "openai_chat_completions".into(),
        protocol_upstream: "openai_chat_completions".into(),
        mode: "native".into(),
        status_code: 200,
        success: true,
        retry_count: 0,
        latency_ms: 5,
        ttft_ms: None,
        input_tokens: 1,
        output_tokens: 1,
        reasoning_tokens: 0,
        cached_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        total_tokens: 2,
        usage_source: "upstream".into(),
        degraded: false,
        route_id: Some("history-route".into()),
        streamed: false,
        error_summary: None,
        fallback_reason: None,
    };
    database
        .insert_usage_with_attempts(
            &event,
            &[UsageAttempt {
                attempt_no: 0,
                provider_id: source_id.clone(),
                source_id: source_id.clone(),
                account_id: format!("deleted-account-{suffix}"),
                upstream_model_id: Some("history-upstream".into()),
                status_code: 200,
                success: true,
                latency_ms: 5,
            }],
        )
        .await
        .expect("insert Source history usage fixture");

    sqlx::query("DELETE FROM sources WHERE id=$1")
        .bind(&source_id)
        .execute(&database.pool)
        .await
        .expect("delete control-plane Source");

    let persisted = database
        .get_usage_event_detail(&request_id)
        .await
        .expect("query usage after Source deletion")
        .expect("usage history survives Source deletion");
    assert_eq!(persisted.source_id.as_deref(), Some(source_id.as_str()));
    assert_eq!(persisted.client_source, "history-client");
    let attempts = database
        .list_attempts_for_event(&request_id)
        .await
        .expect("query attempts after Source deletion");
    assert_eq!(attempts[0].source_id.as_deref(), Some(source_id.as_str()));

    sqlx::query("DELETE FROM usage_events WHERE request_id=$1")
        .bind(&request_id)
        .execute(&database.pool)
        .await
        .expect("clean Source history usage fixture");
}

#[tokio::test]
async fn postgres_cursor_handles_large_pages_without_duplicates_or_omissions() {
    let Some(database) = postgres_test_database().await else {
        eprintln!("skipping PostgreSQL pagination test: TEST_DATABASE_URL is not set");
        return;
    };
    let prefix = format!("usage-page-{}-", uuid::Uuid::new_v4());
    let logical_model = format!("logical-{prefix}");
    sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,usage_source,created_at) SELECT $1 || LPAD(i::TEXT,4,'0'),'provider-page','account-page',$2,$2,'source-page','test','openai_responses','openai_responses','native',200,TRUE,0,1,'missing','2026-02-01T00:00:00Z'::TIMESTAMPTZ FROM generate_series(1,503) AS i")
        .bind(&prefix)
        .bind(&logical_model)
        .execute(&database.pool)
        .await
        .expect("insert pagination fixtures");
    let filter = UsageFilter {
        logical_model: Some(logical_model),
        ..Default::default()
    };
    let first = database
        .list_usage_events_page(&filter, 500, None)
        .await
        .expect("first page");
    assert_eq!(first.data.len(), 500);
    assert!(first.has_more);
    let cursor = UsageCursor::decode(first.next_cursor.as_deref().unwrap()).unwrap();
    let second = database
        .list_usage_events_page(&filter, 500, Some(&cursor))
        .await
        .expect("second page");
    assert_eq!(second.data.len(), 3);
    assert!(!second.has_more);
    let ids = first
        .data
        .iter()
        .chain(&second.data)
        .map(|event| event.request_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 503);
    sqlx::query("DELETE FROM usage_events WHERE request_id LIKE $1")
        .bind(format!("{prefix}%"))
        .execute(&database.pool)
        .await
        .expect("clean pagination fixtures");
}

#[test]
fn model_catalog_schema_declares_required_keys_and_routability_guard() {
    let schema = include_str!("../../../migrations/0003_model_catalog.sql");
    assert!(schema.contains("PRIMARY KEY (source_id, upstream_model_id, protocol)"));
    assert!(schema
        .contains("UNIQUE (logical_model_id, source_id, account_id, upstream_model_id, protocol)"));
    assert!(schema.contains("provider_preset_snapshot JSONB NOT NULL"));
    assert!(schema.contains("confirmed model binding is not routable"));
    let discovery_schema = include_str!("../../../migrations/0008_provider_discovery.sql");
    assert!(discovery_schema.contains("CREATE TABLE IF NOT EXISTS source_discovery_runs"));
    assert!(discovery_schema.contains("CREATE TABLE IF NOT EXISTS source_connection_tests"));
    assert!(discovery_schema.contains("raw_snapshot JSONB"));
    assert!(discovery_schema.contains("error_message TEXT"));
}

#[test]
fn retention_schema_declares_independent_policies_and_operation_state() {
    let schema = include_str!("../../../migrations/0011_retention_backup.sql");
    for marker in [
        "CREATE TABLE IF NOT EXISTS retention_policies",
        "CREATE TABLE IF NOT EXISTS retention_cleanup_runs",
        "CREATE TABLE IF NOT EXISTS audit_logs",
        "CREATE TABLE IF NOT EXISTS backup_runs",
        "CREATE TABLE IF NOT EXISTS gateway_schema_migrations",
        "CREATE TABLE IF NOT EXISTS gateway_schema_metadata",
        "policy_key IN ('usage_events', 'usage_attempts', 'audit', 'discovery')",
        "progress JSONB",
    ] {
        assert!(
            schema.contains(marker),
            "missing migration marker: {marker}"
        );
    }
    let system_event_schema = include_str!("../../../migrations/0024_system_events.sql");
    for marker in [
        "VALUES (24, 'system_events')",
        "CREATE TABLE IF NOT EXISTS system_events",
        "'system_events'",
        "scanned_system_events BIGINT",
        "deleted_system_events BIGINT",
    ] {
        assert!(
            system_event_schema.contains(marker),
            "missing system-event retention marker: {marker}"
        );
    }
}

/// Set TEST_DATABASE_URL to run the PostgreSQL constraint and repository
/// coverage. It is intentionally separate from DATABASE_URL so a normal
/// test run cannot mutate an operator's configured gateway database.
#[tokio::test]
async fn model_catalog_database_refresh_constraints_and_binding_states() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("TEST_DATABASE_URL is not set; skipping PostgreSQL model catalog test");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect model catalog test database");
    let database = Database::from_pool(pool.clone());
    database.migrate().await.expect("migrate test database");
    let repository = ModelCatalogRepository::new(pool.clone());
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let imported_provider_id = format!("test-import-provider-{suffix}");
    let imported_account_id = format!("test-import-account-{suffix}");
    let preset_id = format!("test-provider-{suffix}");
    let model_preset_id = format!("test-model-preset-{suffix}");
    let source_id = format!("test-source-{suffix}");
    let account_id = format!("test-account-{suffix}");
    let logical_id = format!("test-logical-{suffix}");
    let upstream_model_id = "upstream-model";
    let public_name = format!("public-{suffix}");

    let mut imported_config: GatewayConfig = serde_json::from_value(json!({
        "listen_addr": "127.0.0.1:0",
        "providers": [{
            "id": imported_provider_id,
            "name": "Imported Provider",
            "base_url": "https://imported.example"
        }],
        "accounts": [{
            "id": imported_account_id,
            "provider_id": imported_provider_id,
            "display_name": "Imported Account"
        }],
        "routes": []
    }))
    .expect("build import config");
    database
        .sync_control_plane(&imported_config)
        .await
        .expect("first config import creates source before account");
    imported_config.providers[0].base_url = "https://changed.example".into();
    database
        .sync_control_plane(&imported_config)
        .await
        .expect("repeat config import preserves independent source snapshot");
    let imported: (String, String, String) = sqlx::query_as("SELECT s.provider_preset_id,a.source_id,s.base_url FROM sources s JOIN accounts a ON a.source_id=s.id WHERE s.id=$1 AND a.id=$2")
        .bind(&imported_provider_id)
        .bind(&imported_account_id)
        .fetch_one(&pool)
        .await
        .expect("load imported source and account");
    assert_eq!(
        imported,
        (
            "custom".into(),
            imported_provider_id.clone(),
            "https://imported.example".into()
        )
    );

    repository
        .insert_provider_preset(&ProviderPresetInput {
            id: preset_id.clone(),
            version: 1,
            display_name: "Test Provider".into(),
            definition: json!({"default_base_url":"https://preset.example"}),
        })
        .await
        .expect("insert provider preset");
    assert!(repository
        .insert_provider_preset(&ProviderPresetInput {
            id: preset_id.clone(),
            version: 1,
            display_name: "Changed Provider".into(),
            definition: json!({"default_base_url":"https://changed.example"}),
        })
        .await
        .is_err());
    let source = repository
        .create_source(&SourceInput {
            id: source_id.clone(),
            display_name: "Test Source".into(),
            provider_preset_id: preset_id.clone(),
            provider_preset_version: 1,
            base_url: "https://source.example".into(),
            endpoints: json!({"openai_chat_completions":"/v1/chat/completions"}),
            auth_config: json!({}),
            protocol_capabilities: json!({}),
        })
        .await
        .expect("create source from immutable preset snapshot");
    assert_eq!(
        source.provider_preset_snapshot,
        json!({"default_base_url":"https://preset.example"})
    );

    let preset_values = MetadataValues::from_fields([
        (MetadataField::ContextWindow, json!(32_768)),
        (MetadataField::Thinking, json!("supported")),
    ])
    .unwrap();
    let model_preset = repository
        .insert_model_preset(&ModelPresetInput {
            id: model_preset_id.clone(),
            version: 1,
            canonical_model_id: upstream_model_id.into(),
            aliases: vec!["model-alias".into()],
            metadata: CatalogMetadata::resolve(&MetadataValues::default(), Some(&preset_values))
                .unwrap(),
        })
        .await
        .expect("insert immutable model preset version");
    assert_eq!(model_preset.version, 1);
    assert!(repository
        .insert_model_preset(&ModelPresetInput {
            id: model_preset_id.clone(),
            version: 1,
            canonical_model_id: upstream_model_id.into(),
            aliases: vec!["different-alias".into()],
            metadata: CatalogMetadata::resolve(&MetadataValues::default(), Some(&preset_values),)
                .unwrap(),
        })
        .await
        .is_err());

    sqlx::query("INSERT INTO providers (id,name,base_url) VALUES ($1,$2,$3)")
        .bind(&source_id)
        .bind("Legacy provider bridge")
        .bind("https://source.example")
        .execute(&pool)
        .await
        .expect("insert current provider bridge");
    sqlx::query(
        "INSERT INTO accounts (id,provider_id,source_id,display_name) VALUES ($1,$2,$3,$4)",
    )
    .bind(&account_id)
    .bind(&source_id)
    .bind(&source_id)
    .bind("Test Account")
    .execute(&pool)
    .await
    .expect("insert source account");

    let initial_refresh = SourceModelRefresh {
        source_id: source_id.clone(),
        upstream_model_id: upstream_model_id.into(),
        raw_snapshot: json!({"id":upstream_model_id,"revision":1}),
        upstream_metadata: MetadataValues::from_fields([
            (MetadataField::ContextWindow, json!(8_192)),
            (MetadataField::Tools, json!("unsupported")),
        ])
        .unwrap(),
        matched_preset: Some(ModelPresetRef {
            id: model_preset_id.clone(),
            version: 1,
        }),
        preset_metadata: Some(preset_values),
        discovered_at: Utc::now(),
    };
    repository
        .refresh_source_model(&initial_refresh)
        .await
        .expect("insert discovered source model");
    repository
        .refresh_source_model(&initial_refresh)
        .await
        .expect("repeat refresh is idempotent");
    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM source_models WHERE source_id=$1 AND upstream_model_id=$2",
    )
    .bind(&source_id)
    .bind(upstream_model_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row_count, 1);

    repository
        .confirm_source_model(
            &source_id,
            upstream_model_id,
            &MetadataValues::from_fields([(MetadataField::ContextWindow, json!(65_536))]).unwrap(),
        )
        .await
        .expect("confirm source model with user override");
    let refreshed = repository
        .refresh_source_model(&SourceModelRefresh {
            raw_snapshot: json!({"id":upstream_model_id,"revision":2}),
            upstream_metadata: MetadataValues::from_fields([(
                MetadataField::ContextWindow,
                json!(128_000),
            )])
            .unwrap(),
            matched_preset: None,
            preset_metadata: None,
            discovered_at: Utc::now(),
            ..initial_refresh
        })
        .await
        .expect("refresh confirmed source model");
    let metadata = refreshed.catalog_metadata().unwrap();
    assert_eq!(
        metadata.values.0[&MetadataField::ContextWindow],
        json!(65_536)
    );
    assert_eq!(
        metadata.field_sources[&MetadataField::ContextWindow],
        MetadataSource::User
    );
    assert_eq!(refreshed.raw_snapshot["revision"], json!(2));
    assert_eq!(
        refreshed.matched_model_preset_id.as_deref(),
        Some(model_preset_id.as_str())
    );

    repository
        .create_logical_model(&LogicalModelInput {
            id: logical_id.clone(),
            public_name: public_name.clone(),
            display_name: "Public Model".into(),
            status: CatalogStatus::Confirmed,
            model_preset: None,
            metadata: CatalogMetadata::resolve(&MetadataValues::default(), None).unwrap(),
        })
        .await
        .expect("create confirmed logical model");

    let capability = SourceModelCapabilityInput {
        source_id: source_id.clone(),
        upstream_model_id: upstream_model_id.into(),
        protocol: Protocol::OpenAiChatCompletions,
        status: CatalogStatus::Confirmed,
        mode: SourceProtocolMode::Unsupported,
        source_protocol: None,
        adapter: None,
        feature_capabilities: BTreeMap::from([("tools".into(), CapabilitySupport::Unsupported)]),
        field_source: MetadataSource::Upstream,
        observed_at: Utc::now(),
    };
    repository
        .upsert_source_model_capability(&capability)
        .await
        .expect("record explicit unsupported capability");
    let binding = repository
        .create_model_binding(&ModelBindingInput {
            logical_model_id: logical_id.clone(),
            source_id: source_id.clone(),
            account_id: account_id.clone(),
            upstream_model_id: upstream_model_id.into(),
            protocol: Protocol::OpenAiChatCompletions,
            priority: 100,
        })
        .await
        .expect("create pending binding");
    assert!(repository
        .transition_model_binding_status(binding.id, CatalogStatus::Confirmed)
        .await
        .is_err());
    assert!(repository
        .list_routable_bindings(&public_name, Protocol::OpenAiChatCompletions)
        .await
        .unwrap()
        .is_empty());

    repository
        .upsert_source_model_capability(&SourceModelCapabilityInput {
            mode: SourceProtocolMode::Native,
            feature_capabilities: BTreeMap::from([("tools".into(), CapabilitySupport::Supported)]),
            field_source: MetadataSource::User,
            ..capability
        })
        .await
        .expect("confirm native capability");
    repository
        .transition_model_binding_status(binding.id, CatalogStatus::Confirmed)
        .await
        .expect("confirm routable binding");
    let routable = repository
        .list_routable_bindings(&public_name, Protocol::OpenAiChatCompletions)
        .await
        .unwrap();
    assert_eq!(routable.len(), 1);
    assert_eq!(routable[0].upstream_model_id, upstream_model_id);

    assert!(repository
        .create_model_binding(&ModelBindingInput {
            logical_model_id: logical_id,
            source_id: source_id.clone(),
            account_id,
            upstream_model_id: upstream_model_id.into(),
            protocol: Protocol::OpenAiChatCompletions,
            priority: 10,
        })
        .await
        .is_err());

    repository
        .mark_source_model_unavailable(&source_id, upstream_model_id, Utc::now())
        .await
        .expect("mark missing discovery result unavailable");
    assert!(repository
        .list_routable_bindings(&public_name, Protocol::OpenAiChatCompletions)
        .await
        .unwrap()
        .is_empty());
    repository
        .transition_model_binding_status(binding.id, CatalogStatus::Unavailable)
        .await
        .expect("mark binding unavailable");
    assert!(repository
        .transition_model_binding_status(binding.id, CatalogStatus::Confirmed)
        .await
        .is_err());

    sqlx::query("DELETE FROM logical_models WHERE public_name=$1")
        .bind(&public_name)
        .execute(&pool)
        .await
        .expect("delete test logical model cascade");
    sqlx::query("DELETE FROM sources WHERE id=$1")
        .bind(&source_id)
        .execute(&pool)
        .await
        .expect("delete test source cascade");
    sqlx::query("DELETE FROM providers WHERE id=$1")
        .bind(&source_id)
        .execute(&pool)
        .await
        .expect("delete test provider bridge");
    sqlx::query("DELETE FROM provider_presets WHERE id=$1 AND version=1")
        .bind(&preset_id)
        .execute(&pool)
        .await
        .expect("delete test provider preset");
    sqlx::query("DELETE FROM model_presets WHERE id=$1 AND version=1")
        .bind(&model_preset_id)
        .execute(&pool)
        .await
        .expect("delete test model preset");
    sqlx::query("DELETE FROM sources WHERE id=$1")
        .bind(&imported_provider_id)
        .execute(&pool)
        .await
        .expect("delete imported test source cascade");
    sqlx::query("DELETE FROM providers WHERE id=$1")
        .bind(&imported_provider_id)
        .execute(&pool)
        .await
        .expect("delete imported test provider");
}
