use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct QuotaAccountView {
    pub(super) account_id: String,
    pub(super) account_display_name: String,
    pub(super) source_id: String,
    pub(super) source_display_name: String,
    pub(super) provider_id: String,
    pub(super) enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct QuotaResource {
    #[serde(rename = "type")]
    pub(super) resource_type: &'static str,
    pub(super) key: String,
    pub(super) label: String,
    pub(super) unit: String,
    pub(super) used: Option<f64>,
    pub(super) remaining: Option<f64>,
    pub(super) limit: Option<f64>,
    pub(super) reset_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct QuotaRefreshError {
    pub(super) code: String,
    pub(super) message: String,
    pub(super) http_status: Option<u16>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UpstreamQuotaSnapshot {
    pub(super) account: QuotaAccountView,
    pub(super) status: &'static str,
    pub(super) resources: Vec<QuotaResource>,
    pub(super) fetched_at: Option<DateTime<Utc>>,
    pub(super) attempted_at: DateTime<Utc>,
    pub(super) latency_ms: i64,
    pub(super) stale: bool,
    pub(super) refresh_error: Option<QuotaRefreshError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) raw: Option<Value>,
}

#[derive(Debug)]
pub(super) struct FetchFailure {
    pub(super) status: &'static str,
    pub(super) code: &'static str,
    pub(super) message: &'static str,
    pub(super) http_status: Option<u16>,
}

#[derive(Debug)]
pub(super) struct ProviderQuota {
    pub(super) resources: Vec<QuotaResource>,
    pub(super) status_override: Option<&'static str>,
    pub(super) raw: Value,
}
