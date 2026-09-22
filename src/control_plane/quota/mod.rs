//! Read-only upstream quota use cases. HTTP envelope mapping belongs to api.
mod providers;
mod repository;
mod types;

use crate::{
    domain::provider_preset::SourceAuthConfig,
    http::SourceHttpClient,
    infra::secrets::{SecretResolver, SecretResolverError},
    source_url::reqwest_error_is_policy_violation,
};
use chrono::{DateTime, Utc};
use futures_util::{future::join_all, StreamExt};
use providers::{parse_provider_quota, quota_status};
use repository::QuotaTarget;
use reqwest::{header::HeaderName, Method, StatusCode, Url};
use serde_json::Value;
use sqlx::PgPool;
use std::time::{Duration, Instant};
pub(crate) use types::UpstreamQuotaSnapshot;
use types::{FetchFailure, ProviderQuota, QuotaAccountView, QuotaRefreshError};

const QUOTA_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_QUOTA_RESPONSE_BYTES: usize = 1024 * 1024;
const MINIMAX_GLOBAL_QUOTA_URL: &str = "https://www.minimax.io/v1/token_plan/remains";
const MINIMAX_CN_QUOTA_URL: &str = "https://www.minimaxi.com/v1/token_plan/remains";

pub(crate) struct QuotaService<'a> {
    pool: &'a PgPool,
    http: &'a SourceHttpClient,
    secrets: &'a SecretResolver,
}
impl<'a> QuotaService<'a> {
    pub(crate) fn new(
        pool: &'a PgPool,
        http: &'a SourceHttpClient,
        secrets: &'a SecretResolver,
    ) -> Self {
        Self {
            pool,
            http,
            secrets,
        }
    }
    pub(crate) async fn list(&self) -> Result<Vec<UpstreamQuotaSnapshot>, sqlx::Error> {
        let targets = repository::list_targets(self.pool).await?;
        Ok(join_all(
            targets
                .into_iter()
                .map(|target| fetch_snapshot(self, target, false)),
        )
        .await)
    }
    pub(crate) async fn get(
        &self,
        account_id: &str,
    ) -> Result<Option<UpstreamQuotaSnapshot>, sqlx::Error> {
        let Some(target) = repository::get_target(self.pool, account_id).await? else {
            return Ok(None);
        };
        Ok(Some(fetch_snapshot(self, target, true).await))
    }
}

async fn fetch_snapshot(
    service: &QuotaService<'_>,
    target: QuotaTarget,
    include_raw: bool,
) -> UpstreamQuotaSnapshot {
    let attempted_at = Utc::now();
    let started = Instant::now();
    let account = account_view(&target);
    if !target.account_enabled || !target.source_enabled {
        return UpstreamQuotaSnapshot {
            account,
            status: "disabled",
            resources: Vec::new(),
            fetched_at: None,
            attempted_at,
            latency_ms: elapsed_ms(started),
            stale: false,
            refresh_error: None,
            raw: None,
        };
    }

    if !matches!(
        target.provider_preset_id.as_str(),
        "deepseek" | "minimax" | "kimi_code"
    ) {
        return UpstreamQuotaSnapshot {
            account,
            status: "unsupported",
            resources: Vec::new(),
            fetched_at: Some(Utc::now()),
            attempted_at,
            latency_ms: elapsed_ms(started),
            stale: false,
            refresh_error: None,
            raw: None,
        };
    }

    let credential = match service.secrets.resolve_account(
        &target.source_id,
        &target.account_id,
        target.credential_env.as_deref(),
        target.credential_ciphertext.as_deref(),
        None,
    ) {
        Ok(credential) => credential,
        Err(error) => {
            return failed_snapshot(account, attempted_at, started, secret_failure(error))
        }
    };

    let result = fetch_provider_quota(service, &target, credential.as_str()).await;
    match result {
        Ok(provider) => {
            let status = provider
                .status_override
                .unwrap_or_else(|| quota_status(&provider.resources));
            UpstreamQuotaSnapshot {
                account,
                status,
                resources: provider.resources,
                fetched_at: Some(Utc::now()),
                attempted_at,
                latency_ms: elapsed_ms(started),
                stale: false,
                refresh_error: None,
                raw: include_raw.then_some(provider.raw),
            }
        }
        Err(failure) => failed_snapshot(account, attempted_at, started, failure),
    }
}

fn account_view(target: &QuotaTarget) -> QuotaAccountView {
    QuotaAccountView {
        account_id: target.account_id.clone(),
        account_display_name: target.account_display_name.clone(),
        source_id: target.source_id.clone(),
        source_display_name: target.source_display_name.clone(),
        provider_id: target.provider_preset_id.clone(),
        enabled: target.account_enabled && target.source_enabled,
    }
}

fn failed_snapshot(
    account: QuotaAccountView,
    attempted_at: DateTime<Utc>,
    started: Instant,
    failure: FetchFailure,
) -> UpstreamQuotaSnapshot {
    UpstreamQuotaSnapshot {
        account,
        status: failure.status,
        resources: Vec::new(),
        fetched_at: None,
        attempted_at,
        latency_ms: elapsed_ms(started),
        stale: false,
        refresh_error: Some(QuotaRefreshError {
            code: failure.code.into(),
            message: failure.message.into(),
            http_status: failure.http_status,
        }),
        raw: None,
    }
}

fn secret_failure(error: SecretResolverError) -> FetchFailure {
    FetchFailure {
        status: "auth_error",
        code: error.code(),
        message: error.public_message(),
        http_status: None,
    }
}

async fn fetch_provider_quota(
    service: &QuotaService<'_>,
    target: &QuotaTarget,
    credential: &str,
) -> Result<ProviderQuota, FetchFailure> {
    let url = quota_url(target)?;
    let auth =
        serde_json::from_value::<SourceAuthConfig>(target.auth_config.clone()).map_err(|_| {
            FetchFailure {
                status: "refresh_failed",
                code: "invalid_auth_config",
                message: "source authentication configuration is invalid",
                http_status: None,
            }
        })?;
    let mut request = service
        .http
        .request(Method::GET, url)
        .map_err(|_| FetchFailure {
            status: "refresh_failed",
            code: "source_url_blocked",
            message: "quota URL is blocked by server policy",
            http_status: None,
        })?
        .timeout(QUOTA_TIMEOUT);
    for (name, value) in &auth.default_headers {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| FetchFailure {
            status: "refresh_failed",
            code: "invalid_header_template",
            message: "source header template is invalid",
            http_status: None,
        })?;
        request = request.header(name, value);
    }
    let credential_name = HeaderName::from_bytes(auth.credential_header.header.as_bytes())
        .map_err(|_| FetchFailure {
            status: "refresh_failed",
            code: "invalid_header_template",
            message: "source credential header is invalid",
            http_status: None,
        })?;
    request = request.header(
        credential_name,
        format!("{}{}", auth.credential_header.prefix, credential),
    );

    let response = request.send().await.map_err(transport_failure)?;
    let status = response.status();
    if !status.is_success() {
        return Err(FetchFailure {
            status: if matches!(status.as_u16(), 401 | 403) {
                "auth_error"
            } else {
                "refresh_failed"
            },
            code: if matches!(status.as_u16(), 401 | 403) {
                "upstream_auth_failed"
            } else {
                "upstream_http_error"
            },
            message: if matches!(status.as_u16(), 401 | 403) {
                "upstream rejected the account credential"
            } else {
                "upstream quota endpoint returned a non-success status"
            },
            http_status: Some(status.as_u16()),
        });
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_QUOTA_RESPONSE_BYTES as u64)
    {
        return Err(quota_response_too_large(status));
    }
    let bytes = read_quota_body(response, status).await?;
    let raw: Value = serde_json::from_slice(&bytes).map_err(|_| FetchFailure {
        status: "refresh_failed",
        code: "invalid_quota_response",
        message: "upstream quota response is not valid JSON",
        http_status: Some(status.as_u16()),
    })?;
    parse_provider_quota(&target.provider_preset_id, raw, Utc::now())
}

async fn read_quota_body(
    response: reqwest::Response,
    status: StatusCode,
) -> Result<Vec<u8>, FetchFailure> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(transport_failure)?;
        append_quota_chunk(&mut body, &chunk, status)?;
    }
    Ok(body)
}

fn append_quota_chunk(
    body: &mut Vec<u8>,
    chunk: &[u8],
    status: StatusCode,
) -> Result<(), FetchFailure> {
    if body.len().saturating_add(chunk.len()) > MAX_QUOTA_RESPONSE_BYTES {
        return Err(quota_response_too_large(status));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

fn quota_response_too_large(status: StatusCode) -> FetchFailure {
    FetchFailure {
        status: "refresh_failed",
        code: "quota_response_too_large",
        message: "upstream quota response is too large",
        http_status: Some(status.as_u16()),
    }
}

fn quota_url(target: &QuotaTarget) -> Result<Url, FetchFailure> {
    match target.provider_preset_id.as_str() {
        "deepseek" => join_source_url(&target.base_url, "/user/balance"),
        "kimi_code" => join_source_url(&target.base_url, "/v1/usages"),
        "minimax" => {
            let base = Url::parse(&target.base_url).map_err(|_| invalid_quota_url())?;
            let host = base.host_str().unwrap_or_default().to_ascii_lowercase();
            Url::parse(if host.ends_with("minimaxi.com") {
                MINIMAX_CN_QUOTA_URL
            } else {
                MINIMAX_GLOBAL_QUOTA_URL
            })
            .map_err(|_| invalid_quota_url())
        }
        _ => Err(invalid_quota_url()),
    }
}

fn join_source_url(base_url: &str, endpoint: &str) -> Result<Url, FetchFailure> {
    let base = Url::parse(base_url).map_err(|_| invalid_quota_url())?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(invalid_quota_url());
    }
    Url::parse(&format!(
        "{}{}",
        base.as_str().trim_end_matches('/'),
        endpoint
    ))
    .map_err(|_| invalid_quota_url())
}

fn invalid_quota_url() -> FetchFailure {
    FetchFailure {
        status: "refresh_failed",
        code: "invalid_quota_url",
        message: "upstream quota URL is invalid",
        http_status: None,
    }
}

fn transport_failure(error: reqwest::Error) -> FetchFailure {
    if reqwest_error_is_policy_violation(&error) {
        FetchFailure {
            status: "refresh_failed",
            code: "source_url_blocked",
            message: "quota URL is blocked by server policy",
            http_status: None,
        }
    } else if error.is_timeout() {
        FetchFailure {
            status: "refresh_failed",
            code: "upstream_timeout",
            message: "upstream quota request timed out",
            http_status: None,
        }
    } else if error.is_connect() {
        FetchFailure {
            status: "refresh_failed",
            code: "upstream_connect_failed",
            message: "upstream quota connection failed",
            http_status: None,
        }
    } else {
        FetchFailure {
            status: "refresh_failed",
            code: "upstream_request_failed",
            message: "upstream quota request failed",
            http_status: None,
        }
    }
}

fn elapsed_ms(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn target(base_url: String) -> QuotaTarget {
        QuotaTarget {
            account_id: "account".into(),
            source_id: "source".into(),
            account_display_name: "Account".into(),
            credential_env: None,
            credential_ciphertext: None,
            account_enabled: true,
            source_display_name: "Source".into(),
            provider_preset_id: "deepseek".into(),
            base_url,
            auth_config: json!({"default_headers": {}, "credential_header": {"header": "authorization", "prefix": "Bearer "}}),
            source_enabled: true,
        }
    }

    #[tokio::test]
    async fn disabled_and_unsupported_targets_do_not_resolve_credentials_or_use_database() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let http = crate::http::test_client().unwrap();
        let secrets = SecretResolver::empty();
        let service = QuotaService::new(&pool, &http, &secrets);
        let mut disabled = target("http://127.0.0.1:1".into());
        disabled.account_enabled = false;
        assert_eq!(
            fetch_snapshot(&service, disabled, false).await.status,
            "disabled"
        );
        let mut unsupported = target("http://127.0.0.1:1".into());
        unsupported.provider_preset_id = "custom".into();
        assert_eq!(
            fetch_snapshot(&service, unsupported, false).await.status,
            "unsupported"
        );
    }

    #[tokio::test]
    async fn service_uses_source_auth_and_keeps_upstream_errors_metadata_only() {
        use axum::{routing::get, Router};
        let router = Router::new().route(
            "/user/balance",
            get(|headers: axum::http::HeaderMap| async move {
                assert_eq!(headers["authorization"], "Bearer test-credential");
                (
                    StatusCode::FORBIDDEN,
                    "private upstream diagnostic must not escape",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = target(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let http = crate::http::test_client().unwrap();
        let secrets = SecretResolver::empty();
        let service = QuotaService::new(&pool, &http, &secrets);
        let failure = fetch_provider_quota(&service, &target, "test-credential")
            .await
            .unwrap_err();
        server.abort();
        assert_eq!(failure.code, "upstream_auth_failed");
        assert_eq!(failure.http_status, Some(403));
        assert!(!failure.message.contains("private upstream"));
    }

    #[test]
    fn list_projection_omits_raw_provider_payload() {
        let snapshot = UpstreamQuotaSnapshot {
            account: QuotaAccountView {
                account_id: "a".into(),
                account_display_name: "A".into(),
                source_id: "s".into(),
                source_display_name: "S".into(),
                provider_id: "kimi_code".into(),
                enabled: true,
            },
            status: "ok",
            resources: Vec::new(),
            fetched_at: Some(Utc::now()),
            attempted_at: Utc::now(),
            latency_ms: 1,
            stale: false,
            refresh_error: None,
            raw: None,
        };
        let serialized = serde_json::to_value(snapshot).unwrap();
        assert!(serialized.get("raw").is_none());
    }

    #[test]
    fn response_limit_rejects_chunk_before_appending_past_one_mib() {
        let mut body = vec![0_u8; MAX_QUOTA_RESPONSE_BYTES - 1];
        let error = append_quota_chunk(&mut body, &[1, 2], StatusCode::OK).unwrap_err();
        assert_eq!(error.code, "quota_response_too_large");
        assert_eq!(body.len(), MAX_QUOTA_RESPONSE_BYTES - 1);
    }
}
