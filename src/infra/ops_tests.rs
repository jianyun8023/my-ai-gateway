#[cfg(test)]
mod tests {
    use super::*;
    use crate::{control_plane::ControlPlane, domain::config::GatewayConfig};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn account_export_uses_a_secret_reference_and_never_ciphertext() {
        let mut row = json!({
            "id": "account-a",
            "credential_env": "UPSTREAM_KEY",
            "credential_ciphertext": "ciphertext-that-must-not-escape"
        });
        sanitize_account_row(&mut row);
        let serialized = row.to_string();
        assert!(!serialized.contains("ciphertext-that-must-not-escape"));
        assert_eq!(row["credential"]["kind"], "secret_ref");
        assert_eq!(row["credential"]["name"], "UPSTREAM_KEY");
        assert!(row.get("credential_ciphertext").is_none());
    }

    #[test]
    fn virtual_key_export_never_contains_authentication_or_recovery_material() {
        let mut row = json!({
            "id": 7,
            "name": "personal",
            "key_prefix": "mgk_public",
            "key_hash": "hash-that-must-not-escape",
            "key_ciphertext": "ciphertext-that-must-not-escape"
        });
        sanitize_virtual_key_row(&mut row);

        assert_eq!(row["key_prefix"], "mgk_public");
        assert!(row.get("key_hash").is_none());
        assert!(row.get("key_ciphertext").is_none());
    }

    #[test]
    fn sensitive_nested_source_values_are_redacted_but_model_limits_survive() {
        let sanitized = sanitize_json(json!({
            "auth": {"api_key": "secret", "authorization": "Bearer secret"},
            "headers": {"X-Key": "sk-1234567890123456"},
            "metadata": {"max_output_tokens": 4096, "context_window": 128000}
        }));
        assert_eq!(sanitized["auth"]["api_key"], "[REDACTED]");
        assert_eq!(sanitized["auth"]["authorization"], "[REDACTED]");
        assert_eq!(sanitized["headers"]["X-Key"], "[REDACTED]");
        assert_eq!(sanitized["metadata"]["max_output_tokens"], 4096);
        assert_eq!(
            sanitize_json(json!(
                "https://user:password@example.test/path?api_key=secret"
            )),
            json!("https://example.test/path")
        );
    }

    #[test]
    fn cleanup_defaults_are_operational_and_invalid_batches_are_rejected() {
        let request = CleanupRequest::default();
        assert_eq!(request.batch_size, DEFAULT_BATCH_SIZE);
        assert_eq!(request.max_batches, DEFAULT_MAX_BATCHES);
        assert!(validate_cleanup_request(&request).is_ok());
        let invalid = CleanupRequest {
            batch_size: 0,
            ..request
        };
        assert!(validate_cleanup_request(&invalid).is_err());
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL; run with the PostgreSQL regression suite"]
    async fn postgres_retention_is_independent_resumable_and_restore_verified() {
        let Some(url) = std::env::var("TEST_DATABASE_URL").ok() else {
            eprintln!("TEST_DATABASE_URL is not set; skipping ops PostgreSQL regression");
            return;
        };
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect PostgreSQL test admin database");
        let schema = format!("ops_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create ops test schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse TEST_DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect ops isolated schema");
        let database = Database::from_test_pool(pool.clone())
            .await
            .expect("migrate ops isolated schema");
        let repository = OpsRepository::from_database(&database);
        let control_plane = ControlPlane::new(database.pool().clone(), "127.0.0.1:0");
        crate::control_plane::model_catalog::install_builtin_presets(
            &crate::control_plane::model_catalog::ModelCatalogRepository::new(
                database.pool().clone(),
            ),
        )
        .await
        .expect("install built-in presets for export fixture");

        let policies = RETENTION_KEYS
            .iter()
            .map(|key| RetentionPolicyWrite {
                policy_key: (*key).into(),
                retention_days: 1,
                enabled: true,
            })
            .collect::<Vec<_>>();
        repository
            .update_retention_policies(&policies, "ops-test")
            .await
            .expect("set zero-day test policies");

        let old_request = format!("old-{}", Uuid::new_v4());
        let fresh_request = format!("fresh-{}", Uuid::new_v4());
        for (request_id, age) in [(&old_request, "2 days"), (&fresh_request, "0 seconds")] {
            sqlx::query("INSERT INTO usage_events (request_id,provider_id,account_id,model,logical_model,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success,created_at) VALUES ($1,'provider','account','model','model','source','test','openai_chat_completions','openai_chat_completions','native',200,TRUE,NOW()-$2::interval)")
                .bind(request_id)
                .bind(age)
                .execute(&pool)
                .await
                .expect("insert usage event fixture");
        }
        sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,source_id,account_id,status_code,success,created_at) VALUES ($1,0,'provider','source','account',200,TRUE,NOW()-INTERVAL '2 days')")
            .bind(&old_request)
            .execute(&pool)
            .await
            .expect("insert old attempt fixture");

        // Both existing audit domains are independently covered by the audit
        // policy; their rows are deliberately older than the cut-off.
        sqlx::query("INSERT INTO audit_logs (operation_id,action,status,actor,details,created_at) VALUES ('old-audit','test','progress','ops-test','{}',NOW()-INTERVAL '2 days')")
            .execute(&pool)
            .await
            .expect("insert old audit fixture");

        let dry = repository
            .start_cleanup(&CleanupRequest {
                dry_run: true,
                batch_size: 1,
                max_batches: 1,
                operation_id: Some(format!("dry-{}", Uuid::new_v4())),
                requested_by: Some("ops-test".into()),
                policy_keys: None,
            })
            .await
            .expect("dry-run cleanup");
        assert_eq!(dry.status, "completed");
        assert!(dry.progress["candidates"]["usage_events"]
            .as_i64()
            .is_some_and(|count| count >= 1));
        let old_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM usage_events WHERE request_id=$1")
                .bind(&old_request)
                .fetch_one(&pool)
                .await
                .expect("check dry-run fixture");
        assert_eq!(old_count, 1);

        let operation_id = format!("cleanup-{}", Uuid::new_v4());
        let request = CleanupRequest {
            dry_run: false,
            batch_size: 1,
            max_batches: 1,
            operation_id: Some(operation_id.clone()),
            requested_by: Some("ops-test".into()),
            policy_keys: None,
        };
        let mut run = repository
            .start_cleanup(&request)
            .await
            .expect("first bounded cleanup batch");
        assert!(matches!(run.status.as_str(), "running" | "completed"));
        for _ in 0..8 {
            if run.status != "running" {
                break;
            }
            run = repository
                .start_cleanup(&request)
                .await
                .expect("resume cleanup with same operation id");
        }
        assert_eq!(run.status, "completed");
        let old_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM usage_events WHERE request_id=$1")
                .bind(&old_request)
                .fetch_one(&pool)
                .await
                .expect("check cleaned event");
        let fresh_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM usage_events WHERE request_id=$1")
                .bind(&fresh_request)
                .fetch_one(&pool)
                .await
                .expect("check fresh event");
        assert_eq!(old_count, 0);
        assert_eq!(fresh_count, 1);
        let orphan_attempts: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM usage_event_attempts WHERE request_id=$1")
                .bind(&old_request)
                .fetch_one(&pool)
                .await
                .expect("check cleaned attempt");
        assert_eq!(orphan_attempts, 0);

        let config: GatewayConfig = serde_json::from_value(json!({
            "listen_addr":"127.0.0.1:0",
            "providers":[{"id":"restore-source","name":"Restore Source","base_url":"https://example.com","models":["restore-model"],"native_protocols":["openai_chat_completions"],"endpoints":{"openai_chat_completions":"/v1/chat/completions"},"capabilities":{"streaming":true,"usage":true}}],
            "accounts":[{"id":"restore-account","provider_id":"restore-source","display_name":"Restore Account","credential_env":"OPS_RESTORE_KEY"}],
            "routes":[{"id":"restore-route","model":"restore-model","provider_id":"restore-source","protocols":["openai_chat_completions"],"primary_account_id":"restore-account","mode":"native"}]
        }))
        .expect("build restore config");
        control_plane
            .initialize_from_config(&config, true)
            .await
            .expect("initialize restore fixture");
        sqlx::query("INSERT INTO source_connection_tests (source_id,account_id,protocol,upstream_protocol,mode,status,http_status,latency_ms,error_code,error_message,requested_by,tested_at) VALUES ('restore-source','restore-account','openai_chat_completions','openai_chat_completions','native','failed',503,10,'upstream_unavailable','fixed','ops-test',NOW()-INTERVAL '2 days')")
            .execute(&pool)
            .await
            .expect("insert old connection-test audit fixture");
        sqlx::query("INSERT INTO source_discovery_runs (source_id,account_id,provider_preset_id,provider_preset_version,status,raw_snapshot,diff,discovered_model_count,http_status,latency_ms,error_code,error_message,requested_by,started_at,completed_at) VALUES ('restore-source','restore-account','custom',1,'failed',NULL,'{\"added\":[],\"changed\":[],\"missing\":[]}',0,503,10,'discovery_failed','fixed','ops-test',NOW()-INTERVAL '2 days',NOW()-INTERVAL '2 days')")
            .execute(&pool)
            .await
            .expect("insert old discovery fixture");
        let history_cleanup = repository
            .start_cleanup(&CleanupRequest {
                batch_size: 10,
                max_batches: 10,
                operation_id: Some(format!("history-{}", Uuid::new_v4())),
                requested_by: Some("ops-test".into()),
                ..CleanupRequest::default()
            })
            .await
            .expect("clean old audit and discovery history");
        assert_eq!(history_cleanup.status, "completed");
        let old_connection_tests: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM source_connection_tests WHERE source_id='restore-source'",
        )
        .fetch_one(&pool)
        .await
        .expect("check old connection-test history");
        let old_discoveries: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM source_discovery_runs WHERE source_id='restore-source'",
        )
        .fetch_one(&pool)
        .await
        .expect("check old discovery history");
        assert_eq!(old_connection_tests, 0);
        assert_eq!(old_discoveries, 0);
        let exported = repository
            .export_control_plane(&control_plane, "ops-test")
            .await
            .expect("export control plane")
            .export;
        let exported_json = serde_json::to_string(&exported).expect("serialize export");
        assert!(exported_json.contains("OPS_RESTORE_KEY"));
        assert!(!exported_json.contains("credential_ciphertext"));
        assert!(!exported_json.contains("Authorization"));
        let mut invalid_export = exported.clone();
        invalid_export.runtime_snapshot.fingerprint = "0".repeat(64);
        assert!(matches!(
            repository
                .restore_control_plane(&control_plane, &invalid_export, true, "ops-test")
                .await,
            Err(OpsError::Snapshot(_))
        ));
        let source_after_failed_restore: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sources WHERE id='restore-source'")
                .fetch_one(&pool)
                .await
                .expect("check rollback after failed restore");
        assert_eq!(source_after_failed_restore, 1);
        let restored = repository
            .restore_control_plane(&control_plane, &exported, true, "ops-test")
            .await
            .expect("restore and verify control plane");
        assert!(restored.verified);
        assert_eq!(
            restored.snapshot.revision,
            exported.runtime_snapshot.revision
        );
        let credential_ciphertext: Option<String> = sqlx::query_scalar(
            "SELECT credential_ciphertext FROM accounts WHERE id='restore-account'",
        )
        .fetch_one(&pool)
        .await
        .expect("check restored credential ciphertext");
        assert!(credential_ciphertext.is_none());

        drop(database);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
            .execute(&admin)
            .await
            .expect("drop ops test schema");
        admin.close().await;
    }
}
