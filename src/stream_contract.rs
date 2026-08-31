//! Streaming response contract shared by the native gateway paths.
//!
//! The contract deliberately lives outside the protocol adapters.  Native
//! providers keep their bytes and event order untouched; this module only
//! adds gateway-owned SSE comments and terminal error frames when a stream
//! cannot complete normally.

use axum::body::{Body, Bytes};
use futures_util::{stream, Stream, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    time::{Duration, Instant},
};

use crate::protocol::Protocol;

/// A comment frame is valid for all three supported SSE protocols.  Keeping
/// the marker stable lets usage observation filter gateway heartbeats without
/// ever treating them as provider data or TTFT.
pub const HEARTBEAT_FRAME: &[u8] = b": gateway-heartbeat\n\n";
pub const HEARTBEAT_MARKER: &str = ": gateway-heartbeat";

/// Process-level streaming policy.  A zero duration disables that limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamConfig {
    pub heartbeat_interval: Duration,
    pub connection_timeout: Duration,
    pub first_event_timeout: Duration,
    pub idle_timeout: Duration,
    pub total_timeout: Duration,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(15),
            connection_timeout: Duration::from_secs(10),
            first_event_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(60),
            total_timeout: Duration::from_secs(300),
        }
    }
}

impl StreamConfig {
    /// Load the gateway policy from environment variables.  The `_MS` names
    /// are canonical, while duration strings (for example `2s` or `500ms`)
    /// are accepted for operational convenience.  A bare number in a
    /// non-`_MS` variable is interpreted as seconds.
    pub fn from_env() -> Self {
        Self::from_env_prefix("GATEWAY_SSE")
    }

    /// Same parser for embedded/standalone adapters with their own prefix.
    pub fn from_env_prefix(prefix: &str) -> Self {
        let defaults = Self::default();
        Self {
            heartbeat_interval: env_duration_suffixes(
                prefix,
                &[
                    "HEARTBEAT_INTERVAL_MS",
                    "HEARTBEAT_MS",
                    "HEARTBEAT_INTERVAL",
                ],
                defaults.heartbeat_interval,
            ),
            connection_timeout: env_duration_suffixes(
                prefix,
                &[
                    "CONNECTION_TIMEOUT_MS",
                    "CONNECT_TIMEOUT_MS",
                    "CONNECTION_TIMEOUT",
                    "CONNECT_TIMEOUT",
                ],
                defaults.connection_timeout,
            ),
            first_event_timeout: env_duration_suffixes(
                prefix,
                &["FIRST_EVENT_TIMEOUT_MS", "FIRST_EVENT_TIMEOUT"],
                defaults.first_event_timeout,
            ),
            idle_timeout: env_duration_suffixes(
                prefix,
                &["IDLE_TIMEOUT_MS", "IDLE_TIMEOUT"],
                defaults.idle_timeout,
            ),
            total_timeout: env_duration_suffixes(
                prefix,
                &["TOTAL_TIMEOUT_MS", "TOTAL_TIMEOUT"],
                defaults.total_timeout,
            ),
        }
    }
}

fn env_duration_suffixes(prefix: &str, suffixes: &[&str], default: Duration) -> Duration {
    for suffix in suffixes {
        let name = format!("{prefix}_{suffix}");
        let Ok(raw) = std::env::var(&name) else {
            continue;
        };
        if let Some(value) =
            parse_duration(&raw, suffix.ends_with("_MS") || *suffix == "HEARTBEAT_MS")
        {
            return value;
        }
    }
    default
}

fn parse_duration(raw: &str, bare_ms: bool) -> Option<Duration> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if matches!(
        value.to_ascii_lowercase().as_str(),
        "off" | "none" | "disable" | "disabled"
    ) {
        return Some(Duration::ZERO);
    }
    let (number, multiplier) = if let Some(number) = value.strip_suffix("ms") {
        (number, 1u64)
    } else if let Some(number) = value.strip_suffix('s') {
        (number, 1_000u64)
    } else if let Some(number) = value.strip_suffix('m') {
        (number, 60_000u64)
    } else if bare_ms {
        (value, 1u64)
    } else {
        (value, 1_000u64)
    };
    let millis = number.trim().parse::<u64>().ok()?.checked_mul(multiplier)?;
    Some(Duration::from_millis(millis))
}

/// Why a streaming response stopped.  `Completed` is the only successful
/// outcome; every other variant is persisted as a failed logical request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamTermination {
    Completed,
    EmptyStream,
    UpstreamError,
    ClientCancelled,
    ConnectionTimeout,
    FirstEventTimeout,
    IdleTimeout,
    TotalTimeout,
}

impl StreamTermination {
    pub fn is_failure(self) -> bool {
        self != Self::Completed
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::EmptyStream => "gateway_empty_stream",
            Self::UpstreamError => "gateway_upstream_error",
            Self::ClientCancelled => "gateway_client_cancelled",
            Self::ConnectionTimeout => "gateway_connection_timeout",
            Self::FirstEventTimeout => "gateway_first_event_timeout",
            Self::IdleTimeout => "gateway_idle_timeout",
            Self::TotalTimeout => "gateway_total_timeout",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Completed => "stream completed",
            Self::EmptyStream => "upstream stream ended without an event",
            Self::UpstreamError => "upstream stream failed",
            Self::ClientCancelled => "client disconnected while streaming",
            Self::ConnectionTimeout => "timed out waiting for the upstream connection",
            Self::FirstEventTimeout => "timed out waiting for the first upstream event",
            Self::IdleTimeout => "upstream stream was idle for too long",
            Self::TotalTimeout => "upstream stream exceeded its total time limit",
        }
    }

    pub fn status_code(self) -> i32 {
        match self {
            Self::Completed => 200,
            Self::ClientCancelled => 499,
            Self::ConnectionTimeout
            | Self::FirstEventTimeout
            | Self::IdleTimeout
            | Self::TotalTimeout => 504,
            Self::EmptyStream | Self::UpstreamError => 599,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SseActivity {
    pub provider_event: bool,
    pub terminal: Option<StreamTermination>,
    pub error: bool,
}

/// Small, allocation-bounded SSE frame tracker used only for timing and
/// termination decisions.  It never rewrites or buffers bytes sent to the
/// client.
#[derive(Clone, Debug, Default)]
pub struct SseEventTracker {
    line: Vec<u8>,
    event_name: String,
    data_lines: Vec<String>,
    frame_non_comment: bool,
    saw_provider_event: bool,
    saw_sse_frame: bool,
    terminal: Option<StreamTermination>,
}

impl SseEventTracker {
    pub fn feed(&mut self, bytes: &[u8]) -> SseActivity {
        let mut activity = SseActivity::default();
        if self.line.is_empty()
            && self.data_lines.is_empty()
            && self.event_name.is_empty()
            && !bytes.is_empty()
            && !looks_like_sse_control(bytes)
        {
            // Providers that stream raw JSON/text rather than SSE still have
            // a meaningful first event as soon as non-whitespace bytes arrive.
            if bytes.iter().any(|byte| !byte.is_ascii_whitespace()) {
                activity.provider_event = true;
                self.saw_provider_event = true;
            }
        }
        for byte in bytes {
            if *byte == b'\n' {
                self.process_line(&mut activity);
            } else if *byte != b'\r' {
                self.line.push(*byte);
                // A non-SSE body (some providers occasionally return a raw
                // JSON/text stream) is still an event as soon as bytes arrive.
                if self.line.len() == 1 && !matches!(self.line[0], b':' | b' ') {
                    self.frame_non_comment = true;
                }
            }
        }
        activity
    }

    pub fn finish_eof(&mut self) -> SseActivity {
        let mut activity = SseActivity::default();
        if !self.line.is_empty() {
            self.process_line(&mut activity);
        }
        self.dispatch(&mut activity);
        activity
    }

    pub fn saw_provider_event(&self) -> bool {
        self.saw_provider_event
    }

    pub fn terminal(&self) -> Option<StreamTermination> {
        self.terminal
    }

    pub fn saw_sse_frame(&self) -> bool {
        self.saw_sse_frame
    }

    pub fn safe_for_heartbeat(&self) -> bool {
        self.line.is_empty()
            && self.data_lines.is_empty()
            && self.event_name.is_empty()
            && !self.frame_non_comment
    }

    fn process_line(&mut self, activity: &mut SseActivity) {
        let line = std::mem::take(&mut self.line);
        if line.is_empty() {
            self.dispatch(activity);
            return;
        }
        if line.first() == Some(&b':') {
            return;
        }
        let text = String::from_utf8_lossy(&line).trim().to_owned();
        if let Some(value) = text.strip_prefix("event:") {
            self.saw_sse_frame = true;
            self.event_name = value.trim().to_owned();
        } else if let Some(value) = text.strip_prefix("data:") {
            self.saw_sse_frame = true;
            self.data_lines.push(value.trim().to_owned());
        } else if text.starts_with("id:") || text.starts_with("retry:") {
            self.saw_sse_frame = true;
            // SSE metadata does not represent a provider event.
        } else {
            self.frame_non_comment = true;
        }
    }

    fn dispatch(&mut self, activity: &mut SseActivity) {
        if !self.frame_non_comment && self.data_lines.is_empty() && self.event_name.is_empty() {
            return;
        }
        let event_name = std::mem::take(&mut self.event_name);
        let data = std::mem::take(&mut self.data_lines).join("\n");
        let frame_non_comment = std::mem::take(&mut self.frame_non_comment);
        let (terminal, error, gateway) = classify_frame(&event_name, &data);
        if gateway.is_some() {
            activity.error |= gateway.is_some_and(StreamTermination::is_failure);
            merge_terminal(&mut activity.terminal, gateway);
            merge_terminal(&mut self.terminal, gateway);
        } else if frame_non_comment || !data.is_empty() || !event_name.is_empty() {
            activity.provider_event = true;
            self.saw_provider_event = true;
            if error {
                activity.error = true;
                merge_terminal(
                    &mut activity.terminal,
                    Some(StreamTermination::UpstreamError),
                );
                merge_terminal(&mut self.terminal, Some(StreamTermination::UpstreamError));
            } else {
                merge_terminal(&mut activity.terminal, terminal);
                merge_terminal(&mut self.terminal, terminal);
            }
        }
    }
}

fn merge_terminal(current: &mut Option<StreamTermination>, candidate: Option<StreamTermination>) {
    let Some(candidate) = candidate else {
        return;
    };
    match current {
        None => *current = Some(candidate),
        Some(existing) if *existing == StreamTermination::Completed && candidate.is_failure() => {
            *existing = candidate;
        }
        // A gateway/provider failure is more informative than a later
        // protocol success marker in the same coalesced chunk.
        Some(existing) if existing.is_failure() && candidate == StreamTermination::Completed => {}
        Some(existing) if existing.is_failure() => {}
        Some(existing) => *existing = candidate,
    }
}

fn classify_frame(
    event_name: &str,
    data: &str,
) -> (Option<StreamTermination>, bool, Option<StreamTermination>) {
    let event_name = event_name.trim().to_ascii_lowercase();
    if event_name == "gateway.error" {
        return (None, true, termination_from_code(data));
    }
    if data.trim() == "[DONE]" {
        return (Some(StreamTermination::Completed), false, None);
    }
    if matches!(
        event_name.as_str(),
        "message_stop" | "response.completed" | "response.incomplete" | "response.done"
    ) {
        return (Some(StreamTermination::Completed), false, None);
    }
    let parsed = serde_json::from_str::<Value>(data).ok();
    if let Some(code) = parsed.as_ref().and_then(gateway_code) {
        return (None, true, termination_from_code(code));
    }
    let typ = parsed
        .as_ref()
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if typ == "gateway.error" || typ.starts_with("gateway_") {
        return (None, true, termination_from_code(data));
    }
    if typ == "response.completed" || typ == "response.incomplete" || typ == "response.done" {
        return (Some(StreamTermination::Completed), false, None);
    }
    if event_name == "error"
        || typ == "error"
        || typ == "response.failed"
        || parsed
            .as_ref()
            .is_some_and(|value| value.get("error").is_some())
    {
        return (None, true, None);
    }
    (None, false, None)
}

fn gateway_code(value: &Value) -> Option<&str> {
    value
        .get("code")
        .and_then(Value::as_str)
        .filter(|code| code.starts_with("gateway_"))
        .or_else(|| {
            value
                .get("error")
                .and_then(|error| error.get("code"))
                .and_then(Value::as_str)
                .filter(|code| code.starts_with("gateway_"))
        })
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("error"))
                .and_then(|error| error.get("code"))
                .and_then(Value::as_str)
                .filter(|code| code.starts_with("gateway_"))
        })
}

fn looks_like_sse_control(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start();
    [":", "data:", "event:", "id:", "retry:"]
        .iter()
        .any(|prefix| text.starts_with(prefix) || prefix.starts_with(text))
}

fn termination_from_code(data: &str) -> Option<StreamTermination> {
    let code = if data.starts_with("gateway_") {
        data.to_owned()
    } else {
        let parsed = serde_json::from_str::<Value>(data).ok();
        parsed
            .as_ref()
            .and_then(|value| value.get("code"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                parsed
                    .as_ref()
                    .and_then(|value| value.get("error"))
                    .and_then(|error| error.get("code"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| {
                parsed
                    .as_ref()
                    .and_then(|value| value.get("response"))
                    .and_then(|response| response.get("error"))
                    .and_then(|error| error.get("code"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default()
    };
    Some(match code.as_str() {
        "gateway_empty_stream" => StreamTermination::EmptyStream,
        "gateway_client_cancelled" => StreamTermination::ClientCancelled,
        "gateway_connection_timeout" => StreamTermination::ConnectionTimeout,
        "gateway_first_event_timeout" => StreamTermination::FirstEventTimeout,
        "gateway_idle_timeout" => StreamTermination::IdleTimeout,
        "gateway_total_timeout" => StreamTermination::TotalTimeout,
        _ => StreamTermination::UpstreamError,
    })
}

pub fn is_gateway_heartbeat(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.trim() == HEARTBEAT_MARKER
}

/// Build a protocol-compatible gateway error frame.  The custom error code is
/// stable and intentionally contains no upstream response text.
pub fn gateway_error_frame(protocol: Protocol, termination: StreamTermination) -> Bytes {
    let code = termination.code();
    let message = termination.message();
    let payload = match protocol {
        Protocol::OpenAiChatCompletions => json!({
            "error": {"message": message, "type": code, "code": code}
        }),
        Protocol::OpenAiResponses | Protocol::AnthropicMessages => json!({
            "type": "error",
            "error": {"type": code, "code": code, "message": message}
        }),
    };
    let body = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
    let frame = match protocol {
        Protocol::OpenAiChatCompletions => format!("data: {body}\n\ndata: [DONE]\n\n"),
        Protocol::OpenAiResponses | Protocol::AnthropicMessages => {
            format!("event: error\ndata: {body}\n\n")
        }
    };
    Bytes::from(frame)
}

type BoxByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>>;

struct NativeState {
    upstream: BoxByteStream,
    protocol: Protocol,
    config: StreamConfig,
    request_started: Instant,
    connected_at: Instant,
    tracker: SseEventTracker,
    pending: VecDeque<Result<Bytes, io::Error>>,
    finish_after_pending: bool,
    done: bool,
    next_heartbeat: Option<Instant>,
    last_event: Option<Instant>,
}

impl NativeState {
    fn new(
        upstream: BoxByteStream,
        protocol: Protocol,
        config: StreamConfig,
        request_started: Instant,
    ) -> Self {
        let connected_at = Instant::now();
        let next_heartbeat = (!config.heartbeat_interval.is_zero())
            .then(|| connected_at.checked_add(config.heartbeat_interval))
            .flatten();
        Self {
            upstream,
            protocol,
            config,
            request_started,
            connected_at,
            tracker: SseEventTracker::default(),
            pending: VecDeque::new(),
            finish_after_pending: false,
            done: false,
            next_heartbeat,
            last_event: None,
        }
    }

    fn queue_failure(&mut self, termination: StreamTermination) {
        self.pending
            .push_back(Ok(gateway_error_frame(self.protocol, termination)));
        self.finish_after_pending = true;
    }

    fn total_deadline(&self) -> Option<Instant> {
        (!self.config.total_timeout.is_zero())
            .then(|| self.request_started.checked_add(self.config.total_timeout))
            .flatten()
    }

    fn first_deadline(&self) -> Option<Instant> {
        (!self.config.first_event_timeout.is_zero() && !self.tracker.saw_provider_event())
            .then(|| {
                self.connected_at
                    .checked_add(self.config.first_event_timeout)
            })
            .flatten()
    }

    fn idle_deadline(&self) -> Option<Instant> {
        self.last_event
            .filter(|_| !self.config.idle_timeout.is_zero())
            .and_then(|at| at.checked_add(self.config.idle_timeout))
    }
}

/// Wrap an upstream byte body with heartbeats and stream timing.  The input
/// bytes are yielded unchanged and in order; gateway frames are separate
/// chunks and therefore cannot alter provider frame bytes.
pub fn wrap_native_body(
    body: Body,
    protocol: Protocol,
    config: StreamConfig,
    request_started: Instant,
) -> Body {
    let upstream = body
        .into_data_stream()
        .map(|result| result.map_err(|error| io::Error::other(error.to_string())));
    let state = NativeState::new(Box::pin(upstream), protocol, config, request_started);
    Body::from_stream(stream::unfold(state, next_native))
}

async fn next_native(mut state: NativeState) -> Option<(Result<Bytes, io::Error>, NativeState)> {
    loop {
        if let Some(item) = state.pending.pop_front() {
            return Some((item, state));
        }
        if state.done || state.finish_after_pending {
            state.done = true;
            return None;
        }

        let now = Instant::now();
        if state
            .total_deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            state.queue_failure(StreamTermination::TotalTimeout);
            continue;
        }
        if state
            .first_deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            state.queue_failure(StreamTermination::FirstEventTimeout);
            continue;
        }
        if state
            .idle_deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            state.queue_failure(StreamTermination::IdleTimeout);
            continue;
        }

        let total_deadline = state.total_deadline();
        let first_deadline = state.first_deadline();
        let idle_deadline = state.idle_deadline();
        let heartbeat_deadline = state.next_heartbeat;
        let far = tokio::time::Instant::now() + Duration::from_secs(31_536_000);
        let total_sleep = tokio::time::sleep_until(
            total_deadline
                .map(tokio::time::Instant::from_std)
                .unwrap_or(far),
        );
        let first_sleep = tokio::time::sleep_until(
            first_deadline
                .map(tokio::time::Instant::from_std)
                .unwrap_or(far),
        );
        let idle_sleep = tokio::time::sleep_until(
            idle_deadline
                .map(tokio::time::Instant::from_std)
                .unwrap_or(far),
        );
        let heartbeat_sleep = tokio::time::sleep_until(
            heartbeat_deadline
                .map(tokio::time::Instant::from_std)
                .unwrap_or(far),
        );
        tokio::pin!(total_sleep);
        tokio::pin!(first_sleep);
        tokio::pin!(idle_sleep);
        tokio::pin!(heartbeat_sleep);

        tokio::select! {
            biased;
            item = state.upstream.next() => {
                match item {
                    Some(Ok(bytes)) if bytes.is_empty() => continue,
                    Some(Ok(bytes)) => {
                        let activity = state.tracker.feed(&bytes);
                        if activity.provider_event {
                            let now = Instant::now();
                            state.last_event = Some(now);
                            state.next_heartbeat = (!state.config.heartbeat_interval.is_zero())
                                .then(|| now.checked_add(state.config.heartbeat_interval))
                                .flatten();
                        }
                        state.pending.push_back(Ok(bytes));
                        if activity.error || activity.terminal.is_some() {
                            state.finish_after_pending = true;
                        }
                    }
                    Some(Err(_error)) => {
                        state.queue_failure(StreamTermination::UpstreamError);
                    }
                    None => {
                        state.tracker.finish_eof();
                        match state.tracker.terminal() {
                            Some(termination) if termination.is_failure() => {
                                // The provider already sent an error frame; keep its bytes and
                                // close without appending another.
                                state.finish_after_pending = true;
                            }
                            Some(_) => state.finish_after_pending = true,
                            None if !state.tracker.saw_provider_event() => {
                                state.queue_failure(StreamTermination::EmptyStream)
                            }
                            None if state.tracker.saw_sse_frame() => {
                                state.queue_failure(StreamTermination::UpstreamError)
                            }
                            None => state.finish_after_pending = true,
                        }
                    }
                }
            }
            _ = &mut total_sleep, if total_deadline.is_some() => {
                state.queue_failure(StreamTermination::TotalTimeout);
            }
            _ = &mut first_sleep, if first_deadline.is_some() => {
                state.queue_failure(StreamTermination::FirstEventTimeout);
            }
            _ = &mut idle_sleep, if idle_deadline.is_some() => {
                state.queue_failure(StreamTermination::IdleTimeout);
            }
            _ = &mut heartbeat_sleep, if heartbeat_deadline.is_some() => {
                if state.tracker.safe_for_heartbeat() {
                    state.pending.push_back(Ok(Bytes::from_static(HEARTBEAT_FRAME)));
                }
                state.next_heartbeat = (!state.config.heartbeat_interval.is_zero())
                    .then(|| Instant::now().checked_add(state.config.heartbeat_interval))
                    .flatten();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use futures_util::stream;
    use futures_util::StreamExt;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::task::{Context, Poll};

    fn short_config() -> StreamConfig {
        StreamConfig {
            heartbeat_interval: Duration::ZERO,
            connection_timeout: Duration::ZERO,
            first_event_timeout: Duration::from_millis(25),
            idle_timeout: Duration::from_millis(25),
            total_timeout: Duration::from_millis(150),
        }
    }

    #[test]
    fn defaults_are_explicit_and_duration_parser_accepts_units() {
        let defaults = StreamConfig::default();
        assert_eq!(defaults.heartbeat_interval, Duration::from_secs(15));
        assert_eq!(defaults.connection_timeout, Duration::from_secs(10));
        assert_eq!(defaults.first_event_timeout, Duration::from_secs(30));
        assert_eq!(defaults.idle_timeout, Duration::from_secs(60));
        assert_eq!(defaults.total_timeout, Duration::from_secs(300));
        assert_eq!(
            parse_duration("500ms", false),
            Some(Duration::from_millis(500))
        );
        assert_eq!(parse_duration("2s", false), Some(Duration::from_secs(2)));
        assert_eq!(parse_duration("0", true), Some(Duration::ZERO));
        assert_eq!(parse_duration("off", false), Some(Duration::ZERO));
    }

    #[tokio::test]
    async fn heartbeat_is_a_comment_and_does_not_count_as_an_event() {
        let mut config = short_config();
        config.heartbeat_interval = Duration::from_millis(10);
        config.first_event_timeout = Duration::from_millis(100);
        let source = stream::pending::<Result<Bytes, io::Error>>();
        let body = wrap_native_body(
            Body::from_stream(source),
            Protocol::OpenAiResponses,
            config,
            Instant::now(),
        );
        let mut body = body.into_data_stream();
        let heartbeat = tokio::time::timeout(Duration::from_millis(100), body.next())
            .await
            .expect("heartbeat timeout")
            .expect("heartbeat item")
            .expect("heartbeat bytes");
        assert_eq!(heartbeat.as_ref(), HEARTBEAT_FRAME);
        assert!(!SseEventTracker::default().feed(&heartbeat).provider_event);
    }

    #[tokio::test]
    async fn first_event_timeout_emits_gateway_error_and_closes() {
        let mut config = short_config();
        config.first_event_timeout = Duration::from_millis(20);
        let source = stream::pending::<Result<Bytes, io::Error>>();
        let body = wrap_native_body(
            Body::from_stream(source),
            Protocol::OpenAiChatCompletions,
            config,
            Instant::now(),
        );
        let mut body = body.into_data_stream();
        let error = tokio::time::timeout(Duration::from_millis(100), body.next())
            .await
            .expect("timeout wait")
            .expect("timeout item")
            .expect("timeout bytes");
        let text = String::from_utf8_lossy(&error);
        assert!(text.contains("gateway_first_event_timeout"));
        assert!(body.next().await.is_none());
    }

    #[tokio::test]
    async fn provider_bytes_are_forwarded_in_order_without_heartbeat() {
        let config = StreamConfig {
            heartbeat_interval: Duration::from_secs(60),
            connection_timeout: Duration::ZERO,
            first_event_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(5),
            total_timeout: Duration::from_secs(5),
        };
        let first = Bytes::from_static(b"data: one\n\n");
        let second = Bytes::from_static(b"data: two\n\ndata: [DONE]\n\n");
        let source = stream::iter([
            Ok::<Bytes, io::Error>(first.clone()),
            Ok::<Bytes, io::Error>(second.clone()),
        ]);
        let body = wrap_native_body(
            Body::from_stream(source),
            Protocol::OpenAiResponses,
            config,
            Instant::now(),
        );
        let mut body = body.into_data_stream();
        assert_eq!(body.next().await.unwrap().unwrap(), first);
        assert_eq!(body.next().await.unwrap().unwrap(), second);
        assert!(body.next().await.is_none());
    }

    #[tokio::test]
    async fn idle_timeout_is_distinct_after_a_provider_event() {
        let mut config = short_config();
        config.first_event_timeout = Duration::from_secs(1);
        config.idle_timeout = Duration::from_millis(20);
        let source = stream::iter([Ok::<Bytes, io::Error>(Bytes::from_static(b"data: one\n\n"))])
            .chain(stream::pending());
        let body = wrap_native_body(
            Body::from_stream(source),
            Protocol::OpenAiResponses,
            config,
            Instant::now(),
        );
        let mut body = body.into_data_stream();
        assert!(body.next().await.unwrap().is_ok());
        let error = tokio::time::timeout(Duration::from_millis(100), body.next())
            .await
            .expect("idle timeout wait")
            .expect("idle timeout item")
            .expect("idle timeout bytes");
        assert!(String::from_utf8_lossy(&error).contains("gateway_idle_timeout"));
    }

    #[tokio::test]
    async fn total_timeout_is_reported_when_it_precedes_idle_timeout() {
        let mut config = short_config();
        config.first_event_timeout = Duration::from_secs(1);
        config.idle_timeout = Duration::from_secs(1);
        config.total_timeout = Duration::from_millis(20);
        let source = stream::pending::<Result<Bytes, io::Error>>();
        let body = wrap_native_body(
            Body::from_stream(source),
            Protocol::OpenAiResponses,
            config,
            Instant::now(),
        );
        let mut body = body.into_data_stream();
        let error = tokio::time::timeout(Duration::from_millis(100), body.next())
            .await
            .expect("total timeout wait")
            .expect("total timeout item")
            .expect("total timeout bytes");
        assert!(String::from_utf8_lossy(&error).contains("gateway_total_timeout"));
    }

    #[tokio::test]
    async fn empty_and_upstream_error_streams_emit_deterministic_frames() {
        let empty = wrap_native_body(
            Body::from_stream(stream::empty::<Result<Bytes, io::Error>>()),
            Protocol::OpenAiChatCompletions,
            StreamConfig {
                heartbeat_interval: Duration::ZERO,
                connection_timeout: Duration::ZERO,
                first_event_timeout: Duration::ZERO,
                idle_timeout: Duration::ZERO,
                total_timeout: Duration::ZERO,
            },
            Instant::now(),
        );
        let mut empty = empty.into_data_stream();
        let empty_frame = empty.next().await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&empty_frame).contains("gateway_empty_stream"));
        assert!(empty.next().await.is_none());

        let failed = wrap_native_body(
            Body::from_stream(stream::iter([Err::<Bytes, _>(io::Error::other("boom"))])),
            Protocol::OpenAiResponses,
            StreamConfig {
                heartbeat_interval: Duration::ZERO,
                connection_timeout: Duration::ZERO,
                first_event_timeout: Duration::ZERO,
                idle_timeout: Duration::ZERO,
                total_timeout: Duration::ZERO,
            },
            Instant::now(),
        );
        let mut failed = failed.into_data_stream();
        let failed_frame = failed.next().await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&failed_frame).contains("gateway_upstream_error"));
        assert!(failed.next().await.is_none());
    }

    #[tokio::test]
    async fn provider_stream_without_terminal_event_is_not_reported_as_success() {
        let config = StreamConfig {
            heartbeat_interval: Duration::ZERO,
            connection_timeout: Duration::ZERO,
            first_event_timeout: Duration::ZERO,
            idle_timeout: Duration::ZERO,
            total_timeout: Duration::ZERO,
        };
        let source = stream::iter([Ok::<Bytes, io::Error>(Bytes::from_static(
            b"data: {\"delta\":\"partial\"}\n\n",
        ))]);
        let body = wrap_native_body(
            Body::from_stream(source),
            Protocol::OpenAiResponses,
            config,
            Instant::now(),
        );
        let mut body = body.into_data_stream();
        let original = body.next().await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&original).contains("partial"));
        let error = body.next().await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&error).contains("gateway_upstream_error"));
        assert!(body.next().await.is_none());
    }

    #[test]
    fn gateway_failure_wins_over_coalesced_done_marker() {
        let mut tracker = SseEventTracker::default();
        let bytes = gateway_error_frame(
            Protocol::OpenAiChatCompletions,
            StreamTermination::TotalTimeout,
        );
        let activity = tracker.feed(&bytes);
        assert!(activity.error);
        assert_eq!(activity.terminal, Some(StreamTermination::TotalTimeout));
        assert_eq!(tracker.terminal(), Some(StreamTermination::TotalTimeout));
    }

    struct DropProbe(Arc<AtomicBool>);

    impl Stream for DropProbe {
        type Item = Result<Bytes, io::Error>;

        fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Pending
        }
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn dropping_downstream_body_drops_the_upstream_stream() {
        let dropped = Arc::new(AtomicBool::new(false));
        let source = DropProbe(dropped.clone());
        let body = wrap_native_body(
            Body::from_stream(source),
            Protocol::OpenAiResponses,
            StreamConfig::default(),
            Instant::now(),
        );
        drop(body);
        tokio::task::yield_now().await;
        assert!(dropped.load(Ordering::SeqCst));
    }
}
