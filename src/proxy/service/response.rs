/// Map an unusable primary account to the persisted `fallback_reason` code.
/// Only called when `health.available == false` (cooldown / unhealthy / stale
/// with residual failures / unknown with no row); the `disabled` status covers
/// account or source being turned off in the control plane.
fn primary_unavailable_reason(health: &health::AccountHealth) -> String {
    if health.status == "disabled" {
        "account_disabled".into()
    } else {
        match health.status.as_str() {
            "cooling_down" => "account_cooling_down".into(),
            "unhealthy" => "account_unhealthy".into(),
            _ => "account_unavailable".into(),
        }
    }
}

fn finish_proxy(
    protocol: Protocol,
    model: &str,
    started: Instant,
    is_stream: bool,
    response: Response<Body>,
) -> Response<Body> {
    observability::record_proxy_request(
        &protocol.to_string(),
        model,
        response.status().as_u16(),
        started,
        is_stream,
    );
    response
}

fn client_source_from_headers(headers: &HeaderMap) -> String {
    if let Some(value) = headers
        .get("x-client-source")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return value.to_owned();
    }
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(value) = user_agent {
        if let Some(product) = known_client_user_agent(value) {
            return product.to_owned();
        }
    }
    if user_agent.is_some_and(|ua| !ua.is_empty()) {
        tracing::debug!(
            user_agent = user_agent.unwrap_or(""),
            "unrecognised User-Agent mapped to client_source=unknown"
        );
    }
    "unknown".to_owned()
}

/// Recognised upstream products and the canonical `client_source` value
/// they map to.  Prefixes are matched case-sensitively against the first
/// whitespace-delimited token of the User-Agent (parenthesised comments
/// stripped first).  Order matters only when one prefix is a prefix of
/// another — entries are kept short and distinct so the linear walk is
/// sufficient.
///
/// Sources verified against upstream source code:
///   * `Kimi Code CLI`  — `kimi-code-cli/<ver>` set in
///     `MoonshotAI/kimi-code/apps/kimi-code/src/constant/app.ts`
///     (`CLI_USER_AGENT_PRODUCT = "kimi-code-cli"`) and emitted by
///     `packages/oauth/src/identity.ts::createKimiUserAgent`.
///   * `Anthropic SDK Python`  — `Anthropic/Python <ver>` /
///     `AsyncAnthropic/Python <ver>` emitted by
///     `anthropics/anthropic-sdk-python/src/anthropic/_base_client.py`
///     (property `user_agent`).
///   * `Anthropic SDK JS`  — `Anthropic/JS <ver>` from
///     `anthropics/anthropic-sdk-typescript/src/client.ts` (`getUserAgent`).
///   * `Anthropic SDK Go`  — `Anthropic/Go <ver>` from
///     `anthropics/anthropic-sdk-go/internal/requestconfig/requestconfig.go`.
///   * `OpenAI SDK Python`  — `OpenAI/Python <ver>` /
///     `AsyncOpenAI/Python <ver>` from
///     `openai/openai-python/src/openai/_base_client.py`.
///   * `OpenAI SDK Go`  — `OpenAI/Go <ver>` from
///     `openai/openai-go/internal/requestconfig/requestconfig.go`.
///   * `OpenAI Codex CLI`  — `codex_app_server_daemon/<ver> (...) codex_cli_rs/<ver>`
///     from `openai/codex/codex-rs/app-server-daemon/src/client.rs`
///     (round-trip user-agent parser).
///
/// Entries without a verifiable upstream source are deliberately omitted
/// from this table; extend it only after confirming the format in source.
const KNOWN_CLIENT_USER_AGENTS: &[(&str, &str)] = &[
    // Kimi Code CLI / Kimi Code web UI — both ship `kimi-code-cli/<ver>`
    // (the web UI is the same product with a `(web)` suffix in the UA
    // parenthesised comment, which we strip before matching).
    ("kimi-code-cli", "kimi-code-cli"),
    // Kimi Code VS Code extension — ships `kimi-code/<ver>` without the
    // `-cli` suffix, or may appear as `kimi_code/<ver>`.
    ("kimi-code", "kimi-code"),
    ("kimi_code", "kimi-code"),
    // OpenAI Codex CLI — both product tokens seen in the daemon UA.
    ("codex_app_server_daemon", "codex-cli"),
    ("codex_cli_rs", "codex-cli"),
    // Official Anthropic SDKs — Stainless-generated, format `<Brand>/<Lang>`
    // with `Async<Brand>/<Lang>` for async clients.
    ("AsyncAnthropic/", "anthropic-sdk"),
    ("Anthropic/", "anthropic-sdk"),
    // Official OpenAI SDKs — same Stainless shape as Anthropic.
    ("AsyncOpenAI/", "openai-sdk"),
    ("OpenAI/", "openai-sdk"),
];

/// Map a User-Agent string to a stable `client_source` identifier when the
/// upstream product is recognised.  Unknown agents return `None` and the
/// caller falls back to `unknown`.
fn known_client_user_agent(user_agent: &str) -> Option<&'static str> {
    let head = user_agent
        .split_once('(')
        .map(|(h, _)| h)
        .unwrap_or(user_agent);
    let product = head.split_whitespace().next()?.trim_end_matches('/');
    if product.is_empty() {
        return None;
    }
    for (prefix, label) in KNOWN_CLIENT_USER_AGENTS {
        if product == *prefix || product.starts_with(prefix) {
            return Some(*label);
        }
    }
    None
}

fn warn_degraded_route(request_id: &str, route: &ResolvedRoute) {
    if !route.is_degraded() {
        return;
    }
    warn_degraded_features(request_id, &route.route_id, &route.degraded_features);
}

fn warn_degraded_features(request_id: &str, route_id: &str, degraded_features: &[String]) {
    tracing::warn!(
        request_id = %request_id,
        route_id = %route_id,
        degraded_features = ?degraded_features,
        "route has degraded features due to adapter conversion"
    );
}

fn is_event_stream(response: &Response<Body>) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}

#[allow(clippy::too_many_arguments)]
fn wrap_stream_usage(
    response: Response<Body>,
    database: db::Database,
    mut event: db::UsageEvent,
    request_body: Bytes,
    mut attempts: Vec<db::UsageAttempt>,
    request_started: Instant,
    health: health::HealthRegistry,
    protocol: Protocol,
    model: &str,
) -> Response<Body> {
    observability::track_stream_start();
    let model = model.to_owned();
    let protocol = protocol.to_string();
    let (parts, body) = response.into_parts();
    let body = usage::observe_stream_body(body, request_started, move |observation| {
        observability::track_stream_end();
        event.latency_ms = request_started.elapsed().as_millis() as i64;
        let account_id = attempts.last().map(|attempt| attempt.account_id.clone());
        let ttft_ms = observation.ttft_ms;
        finalize_stream_usage(&mut event, &mut attempts, &request_body, observation);
        observability::record_proxy_request(
            &protocol,
            &model,
            event.status_code as u16,
            request_started,
            true,
        );
        if let Some(ttft_ms) = ttft_ms {
            observability::record_ttft(&protocol, &model, Duration::from_millis(ttft_ms as u64));
        }
        if event.input_tokens > 0 {
            observability::record_tokens(&model, "input", event.input_tokens as u64);
        }
        if event.output_tokens > 0 {
            observability::record_tokens(&model, "output", event.output_tokens as u64);
        }
        if event.error_summary.as_deref() == Some("upstream stream error") {
            if let Some(account_id) = account_id {
                tokio::spawn(async move {
                    health
                        .mark_failure_with_details(
                            &account_id,
                            "passive",
                            Some("upstream_stream_error"),
                            Some("upstream stream failed"),
                        )
                        .await;
                });
            }
        }
        tokio::spawn(async move {
            if let Err(error) = database.insert_usage_with_attempts(&event, &attempts).await {
                tracing::warn!(%error, "failed to persist streaming usage event");
            }
        });
    });
    Response::from_parts(parts, body)
}

pub(crate) fn finalize_stream_usage(
    event: &mut db::UsageEvent,
    attempts: &mut [db::UsageAttempt],
    request_body: &[u8],
    observation: usage::StreamObservation,
) {
    event.ttft_ms = observation.ttft_ms;
    let termination = if observation.failed && !observation.termination.is_failure() {
        stream::StreamTermination::UpstreamError
    } else {
        observation.termination
    };
    tracing::debug!(
        termination = termination.code(),
        ttft_ms = ?observation.ttft_ms,
        "stream terminated"
    );
    if termination.is_failure() {
        event.status_code = termination.status_code();
        event.success = false;
        event.error_summary = Some(
            match termination {
                stream::StreamTermination::UpstreamError => "upstream stream error",
                stream::StreamTermination::EmptyStream => "upstream stream ended without an event",
                stream::StreamTermination::ClientCancelled => "client disconnected",
                stream::StreamTermination::ConnectionTimeout => "upstream connection timeout",
                stream::StreamTermination::FirstEventTimeout => "first event timeout",
                stream::StreamTermination::IdleTimeout => "upstream idle timeout",
                stream::StreamTermination::TotalTimeout => "stream total timeout",
                stream::StreamTermination::Completed => "",
            }
            .into(),
        );
        if let Some(attempt) = attempts.last_mut() {
            attempt.status_code = termination.status_code();
            attempt.success = false;
        }
    }
    let report = usage::usage_for_sse_response(
        &event.request_id,
        event.success,
        request_body,
        &observation.captured,
    );
    event.input_tokens = report.input_tokens;
    event.output_tokens = report.output_tokens;
    event.reasoning_tokens = report.reasoning_tokens;
    event.cached_tokens = report.cached_tokens;
    event.cache_read_tokens = report.cache_read_tokens;
    event.cache_creation_tokens = report.cache_creation_tokens;
    event.total_tokens = report.total_tokens;
    event.usage_source = report.source;
}
