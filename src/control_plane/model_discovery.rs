use super::model_catalog::{
    CatalogError, ConnectionTestRecord, DiscoveryApplyResult, DiscoveryRunRecord,
    ModelCatalogRepository, SourceModelRecord, SourceRecord,
};
use crate::{
    domain::{
        catalog::{
            ConnectionTestInput, DiscoveryApplyInput, DiscoveryDiff, DiscoveryFailureInput,
            MetadataValues, ModelPresetRef, SourceModelRefresh, SourceProtocolMode,
        },
        protocol::Protocol,
        provider_preset::{
            CredentialHeaderTemplate, DiscoveryParser, DiscoveryPreset, HttpMethod,
            ProviderPresetDefinition, SourceAuthConfig, SourceProtocolCapability,
        },
    },
    http::SourceHttpClient,
    source_url::{reqwest_error_is_policy_violation, SourceUrlPolicyError},
};
use bytes::BytesMut;
use chrono::{DateTime, Utc};
use reqwest::{header::HeaderName, StatusCode, Url};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fmt,
    time::{Duration, Instant},
};

const MAX_DISCOVERY_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub(crate) enum DiscoveryServiceError {
    Catalog(CatalogError),
    InvalidPreset,
    InvalidSourceUrl,
    SourceUrlBlocked,
    InvalidHeaderTemplate,
    AccountCredentialUnavailable,
}

impl DiscoveryServiceError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Catalog(CatalogError::NotFound(_)) => "not_found",
            Self::Catalog(CatalogError::InvalidMetadata(_) | CatalogError::InvalidState(_)) => {
                "invalid_catalog_state"
            }
            Self::Catalog(_) => "database_error",
            Self::InvalidPreset => "invalid_provider_preset",
            Self::InvalidSourceUrl => "invalid_source_url",
            Self::SourceUrlBlocked => "source_url_blocked",
            Self::InvalidHeaderTemplate => "invalid_header_template",
            Self::AccountCredentialUnavailable => "credential_unavailable",
        }
    }

    pub(crate) fn public_message(&self) -> &str {
        match self {
            Self::Catalog(CatalogError::NotFound(message))
            | Self::Catalog(CatalogError::InvalidMetadata(message))
            | Self::Catalog(CatalogError::InvalidState(message)) => message,
            Self::Catalog(_) => "catalog database operation failed",
            Self::InvalidPreset => "source provider preset snapshot is invalid",
            Self::InvalidSourceUrl => "source base URL or endpoint is invalid",
            Self::SourceUrlBlocked => "source URL is blocked by server policy",
            Self::InvalidHeaderTemplate => "source header template is invalid",
            Self::AccountCredentialUnavailable => {
                "account credential environment variable is unavailable"
            }
        }
    }
}

impl From<SourceUrlPolicyError> for DiscoveryServiceError {
    fn from(error: SourceUrlPolicyError) -> Self {
        if error == SourceUrlPolicyError::InvalidUrl {
            Self::InvalidSourceUrl
        } else {
            Self::SourceUrlBlocked
        }
    }
}

impl fmt::Display for DiscoveryServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.public_message())
    }
}

impl std::error::Error for DiscoveryServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Catalog(error) => Some(error),
            _ => None,
        }
    }
}

impl From<CatalogError> for DiscoveryServiceError {
    fn from(value: CatalogError) -> Self {
        Self::Catalog(value)
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiscoveryExecution {
    pub(crate) run: DiscoveryRunRecord,
    pub(crate) diff: DiscoveryDiff,
    pub(crate) models: Vec<SourceModelRecord>,
}

impl From<DiscoveryApplyResult> for DiscoveryExecution {
    fn from(value: DiscoveryApplyResult) -> Self {
        Self {
            run: value.run,
            diff: value.diff,
            models: value.models,
        }
    }
}

#[derive(Clone, Debug)]
struct SafeFailure {
    code: &'static str,
    message: &'static str,
    http_status: Option<i32>,
}

impl SafeFailure {
    fn transport(error: &reqwest::Error) -> Self {
        if reqwest_error_is_policy_violation(error) {
            Self {
                code: "source_url_blocked",
                message: "source URL is blocked by server policy",
                http_status: None,
            }
        } else if error.is_timeout() {
            Self {
                code: "upstream_timeout",
                message: "upstream request timed out",
                http_status: None,
            }
        } else if error.is_connect() {
            Self {
                code: "upstream_connect_failed",
                message: "upstream connection failed",
                http_status: None,
            }
        } else {
            Self {
                code: "upstream_request_failed",
                message: "upstream request failed",
                http_status: None,
            }
        }
    }

    fn http(status: StatusCode) -> Self {
        Self {
            code: "upstream_http_error",
            message: "upstream returned a non-success HTTP status",
            http_status: Some(i32::from(status.as_u16())),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ModelDiscoveryService {
    repository: ModelCatalogRepository,
    http: SourceHttpClient,
}

impl ModelDiscoveryService {
    pub(crate) fn new(repository: ModelCatalogRepository, http: SourceHttpClient) -> Self {
        Self { repository, http }
    }

    pub(crate) async fn test_connection(
        &self,
        source_id: &str,
        account_id: &str,
        protocol: Protocol,
        model: Option<&str>,
        requested_by: &str,
    ) -> Result<ConnectionTestRecord, DiscoveryServiceError> {
        let source = self.repository.get_source(source_id).await?;
        let preset = parse_preset(&source)?;
        let protocol_preset = preset
            .protocols
            .get(&protocol)
            .ok_or(DiscoveryServiceError::InvalidPreset)?;
        let source_capabilities = source_protocol_capabilities(&source)?;
        let source_capability = source_capabilities
            .get(&protocol)
            .ok_or(DiscoveryServiceError::InvalidPreset)?;
        let upstream_protocol = source_capability.source_protocol.unwrap_or(protocol);
        let auth = source_auth(&source)?;
        let started = Instant::now();
        let tested_at = Utc::now();

        if matches!(
            source_capability.mode,
            SourceProtocolMode::Unknown | SourceProtocolMode::Unsupported
        ) {
            let failure = SafeFailure {
                code: "protocol_unsupported",
                message: "source preset does not provide this protocol",
                http_status: None,
            };
            return self
                .record_connection_failure(
                    source_id,
                    Some(account_id),
                    protocol,
                    upstream_protocol,
                    source_capability.mode,
                    requested_by,
                    tested_at,
                    elapsed_ms(started),
                    failure,
                )
                .await;
        }

        let credential = match self.resolve_credential(source_id, account_id).await {
            Ok(credential) => credential,
            Err(DiscoveryServiceError::AccountCredentialUnavailable) => {
                let failure = SafeFailure {
                    code: "credential_unavailable",
                    message: "account credential environment variable is unavailable",
                    http_status: None,
                };
                return self
                    .record_connection_failure(
                        source_id,
                        Some(account_id),
                        protocol,
                        upstream_protocol,
                        source_capability.mode,
                        requested_by,
                        tested_at,
                        elapsed_ms(started),
                        failure,
                    )
                    .await;
            }
            Err(error) => return Err(error),
        };
        let endpoint = source_endpoint(&source, protocol, &protocol_preset.endpoint)?;
        let url = source_url(&source.base_url, &endpoint)?;
        let model = model
            .filter(|model| !model.trim().is_empty())
            .unwrap_or(&protocol_preset.connection_test.default_model);
        let body = render_template(&protocol_preset.connection_test.body, model);
        let request = request_builder(
            &self.http,
            protocol_preset.connection_test.method,
            url,
            &auth.credential_header,
            &credential,
            auth.default_headers
                .iter()
                .chain(protocol_preset.headers.iter()),
        )?
        .timeout(Duration::from_secs(120))
        .body(serde_json::to_vec(&body).map_err(|_| DiscoveryServiceError::InvalidPreset)?);
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                return self
                    .record_connection_failure(
                        source_id,
                        Some(account_id),
                        protocol,
                        upstream_protocol,
                        source_capability.mode,
                        requested_by,
                        tested_at,
                        elapsed_ms(started),
                        SafeFailure::transport(&error),
                    )
                    .await
            }
        };
        let status = response.status();
        drop(response);
        let latency_ms = elapsed_ms(started);
        if !status.is_success() {
            return self
                .record_connection_failure(
                    source_id,
                    Some(account_id),
                    protocol,
                    upstream_protocol,
                    source_capability.mode,
                    requested_by,
                    tested_at,
                    latency_ms,
                    SafeFailure::http(status),
                )
                .await;
        }
        let record = self
            .repository
            .record_connection_test(&ConnectionTestInput {
                source_id: source_id.into(),
                account_id: Some(account_id.into()),
                protocol,
                upstream_protocol,
                mode: source_capability.mode,
                status: "succeeded".into(),
                http_status: Some(i32::from(status.as_u16())),
                latency_ms,
                error_code: None,
                error_message: None,
                requested_by: normalized_actor(requested_by),
                tested_at,
            })
            .await?;
        tracing::info!(
            source_id,
            account_id,
            %protocol,
            %upstream_protocol,
            status = "succeeded",
            http_status = status.as_u16(),
            latency_ms,
            "provider connection test completed"
        );
        Ok(record)
    }

    pub(crate) async fn discover(
        &self,
        source_id: &str,
        account_id: &str,
        requested_by: &str,
    ) -> Result<DiscoveryExecution, DiscoveryServiceError> {
        let source = self.repository.get_source(source_id).await?;
        let preset = parse_preset(&source)?;
        let auth = source_auth(&source)?;
        let started_at = Utc::now();
        let started = Instant::now();
        let (method, endpoint, parser) = match &preset.discovery {
            DiscoveryPreset::Supported {
                method,
                endpoint,
                parser,
            } => (*method, endpoint, parser),
            DiscoveryPreset::Unsupported { reason } => {
                let run = self
                    .repository
                    .record_discovery_failure(&DiscoveryFailureInput {
                        source_id: source_id.into(),
                        account_id: Some(account_id.into()),
                        status: "unsupported".into(),
                        http_status: None,
                        latency_ms: elapsed_ms(started),
                        error_code: "discovery_unsupported".into(),
                        error_message: reason.clone(),
                        requested_by: normalized_actor(requested_by),
                        started_at,
                        completed_at: Utc::now(),
                    })
                    .await?;
                tracing::info!(
                    source_id,
                    account_id,
                    status = "unsupported",
                    error_code = "discovery_unsupported",
                    "provider model discovery completed"
                );
                return Ok(DiscoveryExecution {
                    run,
                    diff: DiscoveryDiff::default(),
                    models: Vec::new(),
                });
            }
        };

        let credential = match self.resolve_credential(source_id, account_id).await {
            Ok(credential) => credential,
            Err(DiscoveryServiceError::AccountCredentialUnavailable) => {
                return self
                    .record_discovery_failure(
                        &source,
                        account_id,
                        requested_by,
                        started_at,
                        started,
                        SafeFailure {
                            code: "credential_unavailable",
                            message: "account credential environment variable is unavailable",
                            http_status: None,
                        },
                    )
                    .await
            }
            Err(error) => return Err(error),
        };
        let url = source_url(&source.base_url, endpoint)?;
        let request = request_builder(
            &self.http,
            method,
            url,
            &auth.credential_header,
            &credential,
            auth.default_headers.iter(),
        )?;
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                return self
                    .record_discovery_failure(
                        &source,
                        account_id,
                        requested_by,
                        started_at,
                        started,
                        SafeFailure::transport(&error),
                    )
                    .await
            }
        };
        let status = response.status();
        if !status.is_success() {
            drop(response);
            return self
                .record_discovery_failure(
                    &source,
                    account_id,
                    requested_by,
                    started_at,
                    started,
                    SafeFailure::http(status),
                )
                .await;
        }
        let raw = match read_json_limited(response).await {
            Ok(raw) => raw,
            Err(failure) => {
                return self
                    .record_discovery_failure(
                        &source,
                        account_id,
                        requested_by,
                        started_at,
                        started,
                        failure,
                    )
                    .await
            }
        };
        let parsed = match parse_discovered_models(&raw, parser) {
            Ok(parsed) => parsed,
            Err(failure) => {
                return self
                    .record_discovery_failure(
                        &source,
                        account_id,
                        requested_by,
                        started_at,
                        started,
                        failure,
                    )
                    .await
            }
        };
        let completed_at = Utc::now();
        let mut models = Vec::with_capacity(parsed.len());
        for (upstream_model_id, raw_snapshot, upstream_metadata) in parsed {
            let matched = self
                .repository
                .match_model_preset(&upstream_model_id)
                .await?;
            let (matched_preset, preset_metadata) = match matched {
                Some(record) => {
                    let metadata = record.catalog_metadata()?;
                    (
                        Some(ModelPresetRef {
                            id: record.id,
                            version: record.version,
                        }),
                        Some(metadata.values),
                    )
                }
                None => (None, None),
            };
            models.push(SourceModelRefresh {
                source_id: source_id.into(),
                upstream_model_id,
                raw_snapshot,
                upstream_metadata,
                matched_preset,
                preset_metadata,
                discovered_at: completed_at,
            });
        }
        let result = self
            .repository
            .apply_discovery(&DiscoveryApplyInput {
                source_id: source_id.into(),
                account_id: Some(account_id.into()),
                raw_snapshot: raw,
                models,
                http_status: i32::from(status.as_u16()),
                latency_ms: elapsed_ms(started),
                requested_by: normalized_actor(requested_by),
                started_at,
                completed_at,
            })
            .await?;
        tracing::info!(
            source_id,
            account_id,
            status = "succeeded",
            http_status = status.as_u16(),
            discovered_models = result.run.discovered_model_count,
            added = result.diff.added.len(),
            changed = result.diff.changed.len(),
            missing = result.diff.missing.len(),
            "provider model discovery completed"
        );
        Ok(result.into())
    }

    async fn resolve_credential(
        &self,
        source_id: &str,
        account_id: &str,
    ) -> Result<String, DiscoveryServiceError> {
        let account = self
            .repository
            .get_account_credential_ref(source_id, account_id)
            .await?;
        let Some(env_name) = account.credential_env else {
            return Err(DiscoveryServiceError::AccountCredentialUnavailable);
        };
        std::env::var(env_name)
            .ok()
            .filter(|credential| !credential.is_empty())
            .ok_or(DiscoveryServiceError::AccountCredentialUnavailable)
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_connection_failure(
        &self,
        source_id: &str,
        account_id: Option<&str>,
        protocol: Protocol,
        upstream_protocol: Protocol,
        mode: SourceProtocolMode,
        requested_by: &str,
        tested_at: DateTime<Utc>,
        latency_ms: i64,
        failure: SafeFailure,
    ) -> Result<ConnectionTestRecord, DiscoveryServiceError> {
        let record = self
            .repository
            .record_connection_test(&ConnectionTestInput {
                source_id: source_id.into(),
                account_id: account_id.map(str::to_owned),
                protocol,
                upstream_protocol,
                mode,
                status: "failed".into(),
                http_status: failure.http_status,
                latency_ms,
                error_code: Some(failure.code.into()),
                error_message: Some(failure.message.into()),
                requested_by: normalized_actor(requested_by),
                tested_at,
            })
            .await?;
        tracing::warn!(
            source_id,
            account_id,
            %protocol,
            %upstream_protocol,
            status = "failed",
            http_status = failure.http_status,
            error_code = failure.code,
            latency_ms,
            "provider connection test completed"
        );
        Ok(record)
    }

    async fn record_discovery_failure(
        &self,
        source: &SourceRecord,
        account_id: &str,
        requested_by: &str,
        started_at: DateTime<Utc>,
        started: Instant,
        failure: SafeFailure,
    ) -> Result<DiscoveryExecution, DiscoveryServiceError> {
        let latency_ms = elapsed_ms(started);
        let run = self
            .repository
            .record_discovery_failure(&DiscoveryFailureInput {
                source_id: source.id.clone(),
                account_id: Some(account_id.into()),
                status: "failed".into(),
                http_status: failure.http_status,
                latency_ms,
                error_code: failure.code.into(),
                error_message: failure.message.into(),
                requested_by: normalized_actor(requested_by),
                started_at,
                completed_at: Utc::now(),
            })
            .await?;
        tracing::warn!(
            source_id = source.id,
            account_id,
            status = "failed",
            http_status = failure.http_status,
            error_code = failure.code,
            latency_ms,
            "provider model discovery completed"
        );
        Ok(DiscoveryExecution {
            run,
            diff: DiscoveryDiff::default(),
            models: Vec::new(),
        })
    }
}

fn parse_preset(source: &SourceRecord) -> Result<ProviderPresetDefinition, DiscoveryServiceError> {
    let definition: ProviderPresetDefinition =
        serde_json::from_value(source.provider_preset_snapshot.clone())
            .map_err(|error| invalid_source_snapshot(source, "provider_preset_snapshot", error))?;
    definition
        .validate()
        .map_err(|error| invalid_source_snapshot(source, "provider_preset_snapshot", error))?;
    Ok(definition)
}

fn source_auth(source: &SourceRecord) -> Result<SourceAuthConfig, DiscoveryServiceError> {
    serde_json::from_value(source.auth_config.clone())
        .map_err(|error| invalid_source_snapshot(source, "auth_config", error))
}

fn source_protocol_capabilities(
    source: &SourceRecord,
) -> Result<BTreeMap<Protocol, SourceProtocolCapability>, DiscoveryServiceError> {
    serde_json::from_value(source.protocol_capabilities.clone())
        .map_err(|error| invalid_source_snapshot(source, "protocol_capabilities", error))
}

fn source_endpoint(
    source: &SourceRecord,
    protocol: Protocol,
    fallback: &str,
) -> Result<String, DiscoveryServiceError> {
    let endpoints = serde_json::from_value::<BTreeMap<Protocol, String>>(source.endpoints.clone())
        .map_err(|error| invalid_source_snapshot(source, "endpoints", error))?;
    Ok(endpoints
        .get(&protocol)
        .filter(|endpoint| !endpoint.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| fallback.into()))
}

fn invalid_source_snapshot(
    source: &SourceRecord,
    field: &'static str,
    error: impl std::fmt::Display,
) -> DiscoveryServiceError {
    tracing::warn!(
        source_id = %source.id,
        provider_preset_id = %source.provider_preset_id,
        provider_preset_version = source.provider_preset_version,
        field,
        %error,
        "source runtime snapshot is invalid"
    );
    DiscoveryServiceError::InvalidPreset
}

fn source_url(base_url: &str, endpoint: &str) -> Result<Url, DiscoveryServiceError> {
    let base = Url::parse(base_url).map_err(|_| DiscoveryServiceError::InvalidSourceUrl)?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
        || !endpoint.starts_with('/')
        || endpoint.starts_with("//")
    {
        return Err(DiscoveryServiceError::InvalidSourceUrl);
    }
    Url::parse(&format!(
        "{}{}",
        base.as_str().trim_end_matches('/'),
        endpoint
    ))
    .map_err(|_| DiscoveryServiceError::InvalidSourceUrl)
}

fn request_builder<'a>(
    client: &SourceHttpClient,
    method: HttpMethod,
    url: Url,
    credential_header: &CredentialHeaderTemplate,
    credential: &str,
    headers: impl Iterator<Item = (&'a String, &'a String)>,
) -> Result<reqwest::RequestBuilder, DiscoveryServiceError> {
    let method = match method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
    };
    let mut request = client
        .request(method, url)
        .map_err(DiscoveryServiceError::from)?;
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| DiscoveryServiceError::InvalidHeaderTemplate)?;
        request = request.header(name, value);
    }
    let credential_name = HeaderName::from_bytes(credential_header.header.as_bytes())
        .map_err(|_| DiscoveryServiceError::InvalidHeaderTemplate)?;
    request = request.header(
        credential_name,
        format!("{}{}", credential_header.prefix, credential),
    );
    Ok(request)
}

fn render_template(value: &Value, model: &str) -> Value {
    match value {
        Value::String(value) if value == "{{model}}" => Value::String(model.into()),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| render_template(item, model))
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), render_template(value, model)))
                .collect(),
        ),
        value => value.clone(),
    }
}

async fn read_json_limited(response: reqwest::Response) -> Result<Value, SafeFailure> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DISCOVERY_RESPONSE_BYTES as u64)
    {
        return Err(SafeFailure {
            code: "discovery_response_too_large",
            message: "upstream model-list response exceeded the size limit",
            http_status: Some(i32::from(response.status().as_u16())),
        });
    }
    let status = response.status();
    let mut response = response;
    let mut body = BytesMut::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| SafeFailure::transport(&error))?
    {
        if body.len() + chunk.len() > MAX_DISCOVERY_RESPONSE_BYTES {
            return Err(SafeFailure {
                code: "discovery_response_too_large",
                message: "upstream model-list response exceeded the size limit",
                http_status: Some(i32::from(status.as_u16())),
            });
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| SafeFailure {
        code: "invalid_discovery_json",
        message: "upstream model-list response was not valid JSON",
        http_status: Some(i32::from(status.as_u16())),
    })
}

fn parse_discovered_models(
    raw: &Value,
    parser: &DiscoveryParser,
) -> Result<Vec<(String, Value, MetadataValues)>, SafeFailure> {
    let list = value_at_path(raw, &parser.list_path)
        .and_then(Value::as_array)
        .ok_or(SafeFailure {
            code: "invalid_discovery_shape",
            message: "upstream model-list response did not match the preset parser",
            http_status: Some(200),
        })?;
    let mut parsed = BTreeMap::new();
    for item in list {
        if !item.is_object() {
            return Err(SafeFailure {
                code: "invalid_discovery_shape",
                message: "upstream model-list response did not match the preset parser",
                http_status: Some(200),
            });
        }
        let id = value_at_path(item, &parser.id_path)
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or(SafeFailure {
                code: "invalid_discovery_shape",
                message: "upstream model-list response did not match the preset parser",
                http_status: Some(200),
            })?
            .to_owned();
        if parsed.contains_key(&id) {
            return Err(SafeFailure {
                code: "duplicate_discovered_model",
                message: "upstream model-list response contained duplicate model IDs",
                http_status: Some(200),
            });
        }
        let fields = parser.metadata_paths.iter().filter_map(|(field, path)| {
            value_at_path(item, path).map(|value| (*field, value.clone()))
        });
        let metadata = MetadataValues::from_fields(fields).map_err(|_| SafeFailure {
            code: "invalid_discovery_metadata",
            message: "upstream model metadata did not match the preset parser",
            http_status: Some(200),
        })?;
        parsed.insert(id, (item.clone(), metadata));
    }
    Ok(parsed
        .into_iter()
        .map(|(id, (snapshot, metadata))| (id, snapshot, metadata))
        .collect())
}

fn value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .try_fold(value, |current, segment| current.get(segment))
}

fn normalized_actor(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "admin_api".into()
    } else {
        value.chars().take(128).collect()
    }
}

fn elapsed_ms(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane::model_catalog::{install_builtin_presets, ModelCatalogRepository};
    use crate::{
        domain::catalog::{
            CatalogAvailability, CatalogStatus, MetadataField, MetadataSource, SourceInput,
            SourceModelConfirmation,
        },
        domain::provider_preset::{builtin_provider_presets, ProviderPresetDefinition},
        http,
        infra::db::Database,
    };
    use axum::{
        body::{to_bytes, Body},
        extract::Request,
        http::{header, Response},
        Router,
    };
    use serde_json::json;
    use std::{
        collections::VecDeque,
        io::{self, Write},
        sync::{Arc, Mutex},
    };
    use tokio::task::JoinHandle;

    #[test]
    fn parser_is_stable_and_rejects_duplicate_ids() {
        let preset = builtin_provider_presets().unwrap().remove(0);
        let definition: ProviderPresetDefinition =
            serde_json::from_value(preset.definition).unwrap();
        let DiscoveryPreset::Supported { parser, .. } = definition.discovery else {
            panic!("DeepSeek discovery must be supported")
        };
        let parsed =
            parse_discovered_models(&json!({"data":[{"id":"b"},{"id":"a"}]}), &parser).unwrap();
        assert_eq!(parsed[0].0, "a");
        assert_eq!(parsed[1].0, "b");
        assert!(
            parse_discovered_models(&json!({"data":[{"id":"same"},{"id":"same"}]}), &parser)
                .is_err()
        );
    }

    #[test]
    fn credential_and_response_content_never_appear_in_safe_failures() {
        let credential = "super-secret-api-key";
        let response_body = "private complete response body";
        let failure = SafeFailure::http(StatusCode::UNAUTHORIZED);
        let rendered = format!(
            "{} {} {:?}",
            failure.code, failure.message, failure.http_status
        );
        assert!(!rendered.contains(credential));
        assert!(!rendered.contains(response_body));
        assert_eq!(failure.code, "upstream_http_error");
    }

    #[test]
    fn source_urls_append_endpoints_without_losing_base_paths() {
        assert_eq!(
            source_url("https://api.kimi.com/coding", "/v1/messages")
                .unwrap()
                .as_str(),
            "https://api.kimi.com/coding/v1/messages"
        );
        assert!(source_url("https://key@example.com", "/models").is_err());
        assert!(source_url("https://example.com", "//attacker.example").is_err());
    }

    #[derive(Clone)]
    struct MockReply {
        status: StatusCode,
        body: String,
    }

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        path: String,
        authorization: Option<String>,
        body: String,
    }

    async fn spawn_mock_upstream(
        replies: Vec<MockReply>,
    ) -> (String, Arc<Mutex<Vec<RecordedRequest>>>, JoinHandle<()>) {
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new().fallback({
            let replies = replies.clone();
            let recorded = recorded.clone();
            move |request: Request| {
                let replies = replies.clone();
                let recorded = recorded.clone();
                async move {
                    let (parts, body) = request.into_parts();
                    let body = to_bytes(body, 1024 * 1024).await.unwrap_or_default();
                    recorded.lock().unwrap().push(RecordedRequest {
                        path: parts.uri.path().into(),
                        authorization: parts
                            .headers
                            .get(header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_owned),
                        body: String::from_utf8_lossy(&body).into_owned(),
                    });
                    let reply = replies.lock().unwrap().pop_front().unwrap_or(MockReply {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        body: "unexpected mock request".into(),
                    });
                    Response::builder()
                        .status(reply.status)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(reply.body))
                        .unwrap()
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock provider");
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock provider server")
        });
        (format!("http://{address}"), recorded, task)
    }

    struct Fixture {
        source_id: String,
        account_id: String,
        credential_env: String,
        credential: String,
    }

    async fn postgres_database() -> Option<Database> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let database = Database::connect(&url)
            .await
            .expect("connect discovery PostgreSQL test database");
        install_builtin_presets(&ModelCatalogRepository::new(database.pool().clone()))
            .await
            .expect("install built-in presets");
        Some(database)
    }

    async fn create_fixture(
        database: &Database,
        provider_preset_id: &str,
        base_url: String,
    ) -> Fixture {
        let repository = ModelCatalogRepository::new(database.pool().clone());
        let preset = repository
            .latest_provider_preset(provider_preset_id)
            .await
            .expect("load provider preset");
        let definition: ProviderPresetDefinition =
            serde_json::from_value(preset.definition).unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let source_id = format!("discovery-source-{suffix}");
        let account_id = format!("discovery-account-{suffix}");
        let credential_env = format!("DISCOVERY_TEST_CREDENTIAL_{}", suffix.to_uppercase());
        let credential = format!("secret-{suffix}");
        let endpoints = definition
            .protocols
            .iter()
            .map(|(protocol, preset)| (*protocol, preset.endpoint.clone()))
            .collect::<BTreeMap<_, _>>();
        repository
            .create_source(&SourceInput {
                id: source_id.clone(),
                display_name: format!("Discovery {provider_preset_id}"),
                provider_preset_id: provider_preset_id.into(),
                provider_preset_version: preset.version,
                base_url: base_url.clone(),
                endpoints: serde_json::to_value(endpoints).unwrap(),
                auth_config: definition.auth_snapshot(),
                protocol_capabilities: definition.protocol_capabilities_snapshot(),
            })
            .await
            .expect("create discovery source");
        sqlx::query("INSERT INTO providers (id,name,base_url) VALUES ($1,$2,$3)")
            .bind(&source_id)
            .bind(format!("Provider {provider_preset_id}"))
            .bind(&base_url)
            .execute(database.pool())
            .await
            .expect("create provider bridge");
        sqlx::query("INSERT INTO accounts (id,provider_id,source_id,display_name,credential_env) VALUES ($1,$2,$2,$3,$4)")
            .bind(&account_id)
            .bind(&source_id)
            .bind("Discovery account")
            .bind(&credential_env)
            .execute(database.pool())
            .await
            .expect("create discovery account");
        std::env::set_var(&credential_env, &credential);
        Fixture {
            source_id,
            account_id,
            credential_env,
            credential,
        }
    }

    async fn cleanup_fixture(database: &Database, fixture: Fixture) {
        sqlx::query("DELETE FROM sources WHERE id=$1")
            .bind(&fixture.source_id)
            .execute(database.pool())
            .await
            .expect("delete discovery source fixture");
        sqlx::query("DELETE FROM providers WHERE id=$1")
            .bind(&fixture.source_id)
            .execute(database.pool())
            .await
            .expect("delete discovery provider fixture");
        std::env::remove_var(fixture.credential_env);
    }

    #[tokio::test]
    async fn postgres_deepseek_discovery_is_stable_and_preserves_confirmed_user_fields() {
        let Some(database) = postgres_database().await else {
            eprintln!("skipping DeepSeek discovery test: TEST_DATABASE_URL is not set");
            return;
        };
        let first = json!({"object":"list","data":[
            {"id":"deepseek-v4-flash","object":"model","owned_by":"deepseek"},
            {"id":"custom-model","object":"model","revision":1}
        ]});
        let changed = json!({"object":"list","data":[
            {"id":"deepseek-v4-flash","object":"model","owned_by":"deepseek"},
            {"id":"custom-model","object":"model","revision":2}
        ]});
        let missing = json!({"object":"list","data":[
            {"id":"deepseek-v4-flash","object":"model","owned_by":"deepseek"}
        ]});
        let replies = vec![
            MockReply {
                status: StatusCode::OK,
                body: first.clone().to_string(),
            },
            MockReply {
                status: StatusCode::OK,
                body: first.to_string(),
            },
            MockReply {
                status: StatusCode::OK,
                body: changed.to_string(),
            },
            MockReply {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: "private failed discovery response".into(),
            },
            MockReply {
                status: StatusCode::OK,
                body: missing.clone().to_string(),
            },
        ];
        let (base_url, recorded, server) = spawn_mock_upstream(replies).await;
        let fixture = create_fixture(&database, "deepseek", base_url).await;
        let repository = ModelCatalogRepository::new(database.pool().clone());
        let service = ModelDiscoveryService::new(
            repository.clone(),
            http::test_client().expect("discovery client"),
        );

        let initial = service
            .discover(&fixture.source_id, &fixture.account_id, "integration-test")
            .await
            .expect("initial discovery");
        assert_eq!(initial.run.status, "succeeded");
        assert_eq!(initial.diff.added.len(), 2);
        assert!(initial.diff.changed.is_empty());
        assert!(initial.diff.missing.is_empty());
        assert!(initial.models.iter().all(|model| {
            model.confirmation_status == CatalogStatus::Pending
                && model.availability_status == CatalogAvailability::Available
        }));
        let deepseek = initial
            .models
            .iter()
            .find(|model| model.upstream_model_id == "deepseek-v4-flash")
            .unwrap();
        assert_eq!(
            deepseek.matched_model_preset_id.as_deref(),
            Some("deepseek-v4-flash")
        );

        repository
            .update_source_model_user_overrides(
                &fixture.source_id,
                "custom-model",
                &MetadataValues::from_fields([(MetadataField::ContextWindow, json!(654_321))])
                    .unwrap(),
            )
            .await
            .expect("edit custom model with user override");
        let confirmed = repository
            .confirm_source_models(
                &fixture.source_id,
                &[
                    SourceModelConfirmation {
                        upstream_model_id: "custom-model".into(),
                        user_overrides: MetadataValues::default(),
                    },
                    SourceModelConfirmation {
                        upstream_model_id: "deepseek-v4-flash".into(),
                        user_overrides: MetadataValues::default(),
                    },
                ],
            )
            .await
            .expect("batch confirm discovered models");
        assert_eq!(confirmed.len(), 2);
        assert!(confirmed
            .iter()
            .all(|model| model.confirmation_status == CatalogStatus::Confirmed));
        let repeated = service
            .discover(&fixture.source_id, &fixture.account_id, "integration-test")
            .await
            .expect("repeat discovery");
        assert_eq!(repeated.diff, DiscoveryDiff::default());

        let changed = service
            .discover(&fixture.source_id, &fixture.account_id, "integration-test")
            .await
            .expect("changed discovery");
        assert_eq!(changed.diff.changed.len(), 1);
        assert_eq!(changed.diff.changed[0].upstream_model_id, "custom-model");
        assert_eq!(changed.diff.changed[0].changed_fields, vec!["raw_snapshot"]);
        let custom = changed
            .models
            .iter()
            .find(|model| model.upstream_model_id == "custom-model")
            .unwrap();
        let metadata = custom.catalog_metadata().unwrap();
        assert_eq!(
            metadata.values.0[&MetadataField::ContextWindow],
            json!(654_321)
        );
        assert_eq!(
            metadata.field_sources[&MetadataField::ContextWindow],
            MetadataSource::User
        );
        assert_eq!(custom.confirmation_status, CatalogStatus::Confirmed);

        let failed = service
            .discover(&fixture.source_id, &fixture.account_id, "integration-test")
            .await
            .expect("failed discovery is recorded as a structured result");
        assert_eq!(failed.run.status, "failed");
        assert_eq!(failed.run.http_status, Some(500));
        assert_eq!(
            failed.run.error_code.as_deref(),
            Some("upstream_http_error")
        );
        assert!(failed.models.is_empty());
        let after_failure = repository
            .list_source_models(&fixture.source_id, None, None)
            .await
            .unwrap()
            .into_iter()
            .find(|model| model.upstream_model_id == "custom-model")
            .unwrap();
        assert_eq!(
            after_failure.availability_status,
            CatalogAvailability::Available
        );
        assert_eq!(after_failure.confirmation_status, CatalogStatus::Confirmed);

        let removed = service
            .discover(&fixture.source_id, &fixture.account_id, "integration-test")
            .await
            .expect("missing discovery");
        assert_eq!(removed.diff.missing.len(), 1);
        assert_eq!(removed.diff.missing[0].upstream_model_id, "custom-model");
        let custom = repository
            .list_source_models(&fixture.source_id, None, None)
            .await
            .unwrap()
            .into_iter()
            .find(|model| model.upstream_model_id == "custom-model")
            .unwrap();
        assert_eq!(custom.availability_status, CatalogAvailability::Unavailable);
        assert_eq!(custom.confirmation_status, CatalogStatus::Confirmed);
        let latest = repository
            .latest_discovery_run(&fixture.source_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.raw_snapshot, Some(missing));
        assert!(!latest
            .raw_snapshot
            .unwrap()
            .to_string()
            .contains(&fixture.credential));

        let binding_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM model_bindings WHERE source_id=$1")
                .bind(&fixture.source_id)
                .fetch_one(database.pool())
                .await
                .unwrap();
        let capability_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM source_model_capabilities WHERE source_id=$1")
                .bind(&fixture.source_id)
                .fetch_one(database.pool())
                .await
                .unwrap();
        let route_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM routes WHERE provider_id=$1")
                .bind(&fixture.source_id)
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!((binding_count, capability_count, route_count), (0, 0, 0));
        let requests = recorded.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        assert!(requests.iter().all(|request| request.path == "/models"));
        assert!(requests.iter().all(|request| {
            request.authorization.as_deref()
                == Some(format!("Bearer {}", fixture.credential).as_str())
        }));
        assert!(requests.iter().all(|request| request.body.is_empty()));

        cleanup_fixture(&database, fixture).await;
        server.abort();
    }

    #[tokio::test]
    async fn postgres_minimax_empty_discovery_marks_models_missing_without_deleting_them() {
        let Some(database) = postgres_database().await else {
            eprintln!("skipping MiniMax discovery test: TEST_DATABASE_URL is not set");
            return;
        };
        let replies = vec![
            MockReply {
                status: StatusCode::OK,
                body: json!({"object":"list","data":[{"id":"MiniMax-M3"}]}).to_string(),
            },
            MockReply {
                status: StatusCode::OK,
                body: json!({"object":"list","data":[]}).to_string(),
            },
        ];
        let (base_url, recorded, server) = spawn_mock_upstream(replies).await;
        let fixture = create_fixture(&database, "minimax", base_url).await;
        let service = ModelDiscoveryService::new(
            ModelCatalogRepository::new(database.pool().clone()),
            http::test_client().expect("discovery client"),
        );
        let first = service
            .discover(&fixture.source_id, &fixture.account_id, "integration-test")
            .await
            .unwrap();
        assert_eq!(first.diff.added.len(), 1);
        let empty = service
            .discover(&fixture.source_id, &fixture.account_id, "integration-test")
            .await
            .unwrap();
        assert_eq!(empty.run.status, "succeeded");
        assert_eq!(empty.run.discovered_model_count, 0);
        assert_eq!(empty.diff.missing.len(), 1);
        let models = ModelCatalogRepository::new(database.pool().clone())
            .list_source_models(&fixture.source_id, None, None)
            .await
            .unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(
            models[0].availability_status,
            CatalogAvailability::Unavailable
        );
        let requests = recorded.lock().unwrap().clone();
        assert!(requests.iter().all(|request| request.path == "/v1/models"));
        cleanup_fixture(&database, fixture).await;
        server.abort();
    }

    #[derive(Clone)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for SharedWriter {
        type Writer = SharedWriter;

        fn make_writer(&'writer self) -> Self::Writer {
            self.clone()
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn postgres_kimi_protocol_tests_and_failure_logs_are_redacted() {
        let Some(database) = postgres_database().await else {
            eprintln!("skipping Kimi connection test: TEST_DATABASE_URL is not set");
            return;
        };
        let private_body = "private complete response body";
        let replies = vec![
            MockReply {
                status: StatusCode::OK,
                body: "{}".into(),
            },
            MockReply {
                status: StatusCode::OK,
                body: "{}".into(),
            },
            MockReply {
                status: StatusCode::OK,
                body: "{}".into(),
            },
            MockReply {
                status: StatusCode::UNAUTHORIZED,
                body: private_body.into(),
            },
        ];
        let (base_url, recorded, server) = spawn_mock_upstream(replies).await;
        let fixture = create_fixture(&database, "kimi_code", format!("{base_url}/coding")).await;
        let service = ModelDiscoveryService::new(
            ModelCatalogRepository::new(database.pool().clone()),
            http::test_client().expect("discovery client"),
        );

        let unsupported = service
            .discover(&fixture.source_id, &fixture.account_id, "integration-test")
            .await
            .unwrap();
        assert_eq!(unsupported.run.status, "unsupported");
        assert_eq!(
            unsupported.run.error_code.as_deref(),
            Some("discovery_unsupported")
        );

        for protocol in [
            Protocol::OpenAiChatCompletions,
            Protocol::OpenAiResponses,
            Protocol::AnthropicMessages,
        ] {
            let result = service
                .test_connection(
                    &fixture.source_id,
                    &fixture.account_id,
                    protocol,
                    None,
                    "integration-test",
                )
                .await
                .unwrap();
            assert_eq!(result.status, "succeeded");
        }

        let log_buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(SharedWriter(log_buffer.clone()))
            .finish();
        let _subscriber_guard = tracing::subscriber::set_default(subscriber);
        let failed = service
            .test_connection(
                &fixture.source_id,
                &fixture.account_id,
                Protocol::OpenAiChatCompletions,
                None,
                "integration-test",
            )
            .await
            .unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.http_status, Some(401));
        assert_eq!(failed.error_code.as_deref(), Some("upstream_http_error"));
        let logs = String::from_utf8(log_buffer.lock().unwrap().clone()).unwrap();
        assert!(!logs.contains(&fixture.credential));
        assert!(!logs.contains(private_body));
        assert!(!logs.to_ascii_lowercase().contains("authorization"));

        let requests = recorded.lock().unwrap().clone();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/coding/v1/chat/completions",
                "/coding/v1/responses",
                "/coding/v1/messages",
                "/coding/v1/chat/completions",
            ]
        );
        assert!(requests.iter().all(|request| {
            request.authorization.as_deref()
                == Some(format!("Bearer {}", fixture.credential).as_str())
        }));
        cleanup_fixture(&database, fixture).await;
        server.abort();
    }
}
