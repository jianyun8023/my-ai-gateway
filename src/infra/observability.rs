use std::{
    env,
    sync::{
        atomic::{AtomicI64, Ordering},
        OnceLock,
    },
    time::{Duration, Instant},
};

use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::{trace::SdkTracerProvider, Resource};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

pub(crate) const METRIC_REQUESTS_TOTAL: &str = "gateway_requests_total";
pub(crate) const METRIC_ATTEMPTS_TOTAL: &str = "gateway_upstream_attempts_total";
pub(crate) const METRIC_TOKENS_TOTAL: &str = "gateway_tokens_total";
pub(crate) const METRIC_REQUEST_DURATION: &str = "gateway_request_duration_seconds";
pub(crate) const METRIC_TTFT: &str = "gateway_time_to_first_token_seconds";
pub(crate) const METRIC_HEALTH_COOLDOWNS: &str = "gateway_health_cooldowns_total";
pub(crate) const METRIC_SNAPSHOT_REVISION: &str = "gateway_snapshot_revision";
pub(crate) const METRIC_ACTIVE_STREAMS: &str = "gateway_active_streams";

static ACTIVE_STREAMS: AtomicI64 = AtomicI64::new(0);

pub(crate) fn prometheus_handle() -> PrometheusHandle {
    static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
    HANDLE
        .get_or_init(|| {
            PrometheusBuilder::new()
                .install_recorder()
                .expect("failed to install Prometheus recorder")
        })
        .clone()
}

pub(crate) fn spawn_upkeep(handle: PrometheusHandle) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            handle.run_upkeep();
        }
    });
}

/// Initializes the global tracing subscriber.
///
/// When `OTEL_EXPORTER_OTLP_ENDPOINT` is set, an OpenTelemetry tracing layer
/// is added alongside the fmt layer, exporting spans via OTLP/gRPC.
/// When not set, only the fmt layer is active (current default behavior).
///
/// Returns `Option<SdkTracerProvider>` — caller should call `shutdown()` on
/// graceful exit when `Some`.
pub(crate) fn init_tracing() -> Option<SdkTracerProvider> {
    let fmt_layer = fmt::layer();
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    match env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(endpoint) if !endpoint.is_empty() => {
            let service_name =
                env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "my-ai-gateway".to_string());

            match init_otel_provider(&endpoint, &service_name) {
                Ok(provider) => {
                    let tracer = provider.tracer("my-ai-gateway");
                    let otel_layer = OpenTelemetryLayer::new(tracer);

                    Registry::default()
                        .with(env_filter)
                        .with(fmt_layer)
                        .with(otel_layer)
                        .init();

                    tracing::info!(
                        otel_endpoint = %endpoint,
                        otel_service = %service_name,
                        "OpenTelemetry tracing enabled"
                    );
                    Some(provider)
                }
                Err(err) => {
                    Registry::default().with(env_filter).with(fmt_layer).init();

                    tracing::warn!(
                        %err,
                        "failed to initialize OpenTelemetry; falling back to fmt-only"
                    );
                    None
                }
            }
        }
        _ => {
            Registry::default().with(env_filter).with(fmt_layer).init();
            None
        }
    }
}

fn init_otel_provider(
    _endpoint: &str,
    service_name: &str,
) -> Result<SdkTracerProvider, Box<dyn std::error::Error + Send + Sync>> {
    let exporter = SpanExporter::builder().with_tonic().build()?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_service_name(service_name.to_owned())
                .build(),
        )
        .build();

    Ok(provider)
}

// ── Prometheus metrics (unchanged) ──────────────────────────────────────

pub(crate) fn record_request(
    protocol: &str,
    model: &str,
    status: u16,
    duration: Duration,
    is_stream: bool,
) {
    let status_str = status.to_string();
    let stream_str = if is_stream { "stream" } else { "non_stream" };
    counter!(
        METRIC_REQUESTS_TOTAL,
        "protocol" => protocol.to_owned(),
        "model" => model.to_owned(),
        "status" => status_str.clone(),
        "mode" => stream_str.to_owned(),
    )
    .increment(1);
    histogram!(
        METRIC_REQUEST_DURATION,
        "protocol" => protocol.to_owned(),
        "model" => model.to_owned(),
        "status" => status_str,
    )
    .record(duration.as_secs_f64());
}

pub(crate) fn record_attempt(
    protocol: &str,
    source_id: &str,
    account_id: &str,
    status: u16,
    is_fallback: bool,
) {
    counter!(
        METRIC_ATTEMPTS_TOTAL,
        "protocol" => protocol.to_owned(),
        "source" => source_id.to_owned(),
        "account" => account_id.to_owned(),
        "status" => status.to_string(),
        "fallback" => is_fallback.to_string(),
    )
    .increment(1);
}

pub(crate) fn record_tokens(model: &str, direction: &str, count: u64) {
    counter!(
        METRIC_TOKENS_TOTAL,
        "model" => model.to_owned(),
        "direction" => direction.to_owned(),
    )
    .increment(count);
}

pub(crate) fn record_ttft(protocol: &str, model: &str, duration: Duration) {
    histogram!(
        METRIC_TTFT,
        "protocol" => protocol.to_owned(),
        "model" => model.to_owned(),
    )
    .record(duration.as_secs_f64());
}

pub(crate) fn record_cooldown(source_id: &str, account_id: &str) {
    counter!(
        METRIC_HEALTH_COOLDOWNS,
        "source" => source_id.to_owned(),
        "account" => account_id.to_owned(),
    )
    .increment(1);
}

pub(crate) fn set_snapshot_revision(revision: i64) {
    gauge!(METRIC_SNAPSHOT_REVISION).set(revision as f64);
}

pub(crate) fn set_active_streams(count: i64) {
    gauge!(METRIC_ACTIVE_STREAMS).set(count as f64);
}

pub(crate) fn track_stream_start() {
    set_active_streams(ACTIVE_STREAMS.fetch_add(1, Ordering::Relaxed) + 1);
}

pub(crate) fn track_stream_end() {
    set_active_streams(
        ACTIVE_STREAMS
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(1))
            })
            .unwrap_or(0),
    );
}

pub(crate) fn record_proxy_request(
    protocol: &str,
    model: &str,
    status: u16,
    started: Instant,
    is_stream: bool,
) {
    record_request(protocol, model, status, started.elapsed(), is_stream);
}
