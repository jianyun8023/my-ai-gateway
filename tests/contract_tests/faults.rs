//! Deterministic release fault checks (#120), through the actual Gateway Router.
//! Serial execution is required because timeout policy is process-level.
use std::time::Duration;

use axum::{http::StatusCode, Router};
use futures_util::StreamExt;
use my_ai_gateway::test_support::test_gateway_router;
use serde_json::json;
use tokio::time::timeout;

use crate::common::*;
use crate::support::{
    fixtures::{catalog, CaseFixture},
    mock_provider::MockProvider,
    sse::SseChunkPlan,
};

const MODEL: &str = "test-model";
const URIS: [&str; 3] = ["/v1/chat/completions", "/v1/responses", "/v1/messages"];

struct Env(Vec<(&'static str, Option<String>)>);
impl Env {
    fn new(overrides: &[(&'static str, &'static str)]) -> Self {
        let mut saved = Vec::new();
        for &(key, value) in overrides {
            saved.push((key, std::env::var(key).ok()));
            std::env::set_var(key, value);
        }
        Self(saved)
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn payload(stream: bool) -> String {
    json!({"model":MODEL,"messages":[{"role":"user","content":"hello"}],"input":"hello","max_tokens":8,"stream":stream}).to_string()
}

async fn request(router: &Router, uri: &str, stream: bool) -> (StatusCode, String) {
    timeout(Duration::from_secs(5), async {
        let response = gateway_post(router, uri, "fault", &payload(stream)).await;
        let status = response.status();
        let body = text_body(response).await;
        (status, body)
    })
    .await
    .expect("fault request must terminate within five seconds")
}

fn error(status: u16, body: &str) -> CaseFixture {
    CaseFixture::json("fault", StatusCode::from_u16(status).unwrap(), body)
}

#[tokio::test]
async fn faults_http_errors_preserve_status_and_do_not_retry_without_policy() {
    for uri in URIS {
        for status in [400, 401, 403, 404, 429, 500, 502, 503] {
            let mock = spawn_mock_with_default(error(
                status,
                r#"{"error":{"type":"upstream_error","message":"mock failure"}}"#,
            ))
            .await;
            let router = test_gateway_router(native_config(mock.base_url(), MODEL));
            let (actual, body) = request(&router, uri, false).await;
            assert_eq!(actual.as_u16(), status, "{uri}: HTTP {status}");
            assert!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["error"].is_object());
            assert_eq!(mock.request_count(), 1, "no implicit retry of {status}");
        }
    }
}

#[tokio::test]
async fn faults_invalid_and_empty_error_bodies_are_not_wrapped_as_success() {
    for body in ["{invalid-json", ""] {
        for uri in URIS {
            let mock = spawn_mock_with_default(error(502, body)).await;
            let router = test_gateway_router(native_config(mock.base_url(), MODEL));
            let (status, received) = request(&router, uri, false).await;
            assert_eq!(status, StatusCode::BAD_GATEWAY);
            assert_eq!(received, body, "native upstream bytes remain intact");
        }
    }
}

#[tokio::test]
async fn faults_short_retry_after_retries_once_and_keeps_model() {
    let mock = MockProvider::builder()
        .sequence(vec![
            error(429, r#"{"error":{"message":"limited"}}"#).with_header("retry-after", "0"),
            catalog::chat_text_basic(),
        ])
        .build()
        .spawn()
        .await;
    let router = test_gateway_router(native_config(mock.base_url(), MODEL));
    let (status, _) = request(&router, URIS[0], false).await;
    assert_eq!(status, StatusCode::OK);
    let calls = mock.requests();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c.body["model"] == MODEL));
}

#[tokio::test]
async fn faults_fallback_uses_actual_model_after_primary_error() {
    for status in [429, 503] {
        let mock = MockProvider::builder()
            .sequence(vec![
                error(status, r#"{"error":{"message":"unavailable"}}"#),
                catalog::chat_text_basic(),
            ])
            .build()
            .spawn()
            .await;
        let mut config = native_config(mock.base_url(), MODEL);
        let mut account = config.accounts[0].clone();
        account.id = "fallback-account".into();
        account
            .model_map
            .insert(MODEL.into(), "fallback-upstream".into());
        config.accounts.push(account);
        config.routes[0]
            .fallback_accounts
            .push("fallback-account".into());
        let router = test_gateway_router(config);
        let (actual, body) = request(&router, URIS[0], false).await;
        assert_eq!(actual, StatusCode::OK, "{body}");
        let calls = mock.requests();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].body["model"], MODEL);
        assert_eq!(calls[1].body["model"], "fallback-upstream");
        assert_eq!(calls[0].arrival_order, 0);
        assert_eq!(calls[1].arrival_order, 1);
    }
}

#[tokio::test]
async fn faults_connection_close_is_a_gateway_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let reset = tokio::spawn(async move {
        let (connection, _) = listener.accept().await.unwrap();
        drop(connection);
    });
    let router = test_gateway_router(native_config(&format!("http://{address}"), MODEL));
    let (status, body) = request(&router, URIS[0], false).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["error"].is_object());
    timeout(Duration::from_secs(1), reset)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn faults_timeout_before_response_is_http_504() {
    let _env = Env::new(&[("GATEWAY_SSE_CONNECTION_TIMEOUT_MS", "50")]);
    let mock = spawn_mock_with_default(
        catalog::chat_text_basic().with_first_byte_delay(Duration::from_millis(500)),
    )
    .await;
    let router = test_gateway_router(native_config(mock.base_url(), MODEL));
    let (status, body) = request(&router, URIS[0], false).await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert!(body.contains("upstream_request_failed"), "{body}");
    assert!(
        body.contains("timed out waiting for the upstream connection"),
        "{body}"
    );
}

#[tokio::test]
async fn faults_first_event_and_idle_timeouts_are_distinct() {
    let _env = Env::new(&[
        ("GATEWAY_SSE_FIRST_EVENT_TIMEOUT_MS", "100"),
        ("GATEWAY_SSE_IDLE_TIMEOUT_MS", "100"),
        ("GATEWAY_SSE_TOTAL_TIMEOUT_MS", "2000"),
    ]);
    for (start, expected) in [
        (": heartbeat\n\n", "gateway_first_event_timeout"),
        ("data: {\"delta\":\"first\"}\n\n", "gateway_idle_timeout"),
    ] {
        let fixture = CaseFixture::sse(
            "fault",
            StatusCode::OK,
            &format!(
                "{start}event: response.completed\ndata: {{\"type\":\"response.completed\"}}\n\n"
            ),
            SseChunkPlan::per_event().with_delay(Duration::from_millis(500)),
        );
        let mock = spawn_mock_with_default(fixture).await;
        let router = test_gateway_router(native_config(mock.base_url(), MODEL));
        let (status, body) = request(&router, URIS[1], true).await;
        assert_eq!(status, StatusCode::OK, "headers precede stream failures");
        assert!(body.contains(expected), "{body}");
        assert!(!body.contains("response.completed"));
    }
}

#[tokio::test]
async fn faults_incomplete_invalid_and_empty_streams_cannot_complete_successfully() {
    for uri in URIS {
        for (body, expected) in [
            ("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n", "gateway_incomplete_stream"),
            ("data: {invalid}\n\n", "gateway_incomplete_stream"),
            ("", "gateway_empty_stream"),
        ] {
            let mock = spawn_mock_with_default(CaseFixture::sse("fault", StatusCode::OK, body, SseChunkPlan::single_chunk())).await;
            let router = test_gateway_router(native_config(mock.base_url(), MODEL));
            let (_, received) = request(&router, uri, true).await;
            assert!(received.contains(expected), "{uri}: {received}");
            assert!(!received.contains("response.completed"));
            assert!(!received.contains("message_stop"));
            // Chat uses error + [DONE]; the closing sentinel is never the only result.
        }
    }
}

#[tokio::test]
async fn faults_one_finished_choice_does_not_hide_another_truncated_choice() {
    let body = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"a\"},\"finish_reason\":\"stop\"},{\"index\":1,\"delta\":{\"content\":\"b\"},\"finish_reason\":null}]}\n\n";
    let mock = spawn_mock_with_default(CaseFixture::sse(
        "fault",
        StatusCode::OK,
        body,
        SseChunkPlan::single_chunk(),
    ))
    .await;
    let router = test_gateway_router(native_config(mock.base_url(), MODEL));
    assert!(request(&router, URIS[0], true)
        .await
        .1
        .contains("gateway_incomplete_stream"));
}

#[tokio::test]
async fn faults_client_disconnect_closes_the_upstream_connection() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let upstream = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 8192];
        assert!(socket.read(&mut buffer).await.unwrap() > 0);
        let event = "data: {\"delta\":\"first\"}\n\n";
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{event}\r\n", event.len());
        socket.write_all(response.as_bytes()).await.unwrap();
        loop {
            match socket.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    });
    let router = test_gateway_router(native_config(&format!("http://{address}"), MODEL));
    let response = timeout(
        Duration::from_secs(3),
        gateway_post(&router, URIS[1], "fault", &payload(true)),
    )
    .await
    .unwrap();
    let mut stream = response.into_body().into_data_stream();
    timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    drop(stream);
    timeout(Duration::from_secs(3), upstream)
        .await
        .expect("upstream connection must close after downstream drop")
        .unwrap();
}
