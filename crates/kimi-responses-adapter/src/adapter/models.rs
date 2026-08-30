use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use serde_json::Value;

/// ModelInfo describes per-model limits collected from the Kimi Code
/// upstream (or seeded from built-in knowledge).
#[derive(Clone, Debug, Default)]
pub struct ModelInfo {
    pub id: String,
    // Part of the upstream model metadata; kept for completeness though the
    // adapter only makes decisions from max_output_tokens today.
    #[allow(dead_code)]
    pub context_window: i64,
    pub max_output_tokens: i64,
}

/// Seeds the registry. Values are conservative fallbacks for the known Kimi
/// Code models; a successful upstream /v1/models fetch overrides them.
fn builtin_models() -> Vec<ModelInfo> {
    [
        "k3",
        "k3-256k",
        "kimi-for-coding",
        "kimi-for-coding-highspeed",
    ]
    .into_iter()
    .map(|id| ModelInfo {
        id: id.to_string(),
        context_window: 262144,
        max_output_tokens: 32768,
    })
    .collect()
}

struct RegistryState {
    models: HashMap<String, ModelInfo>,
    fetched_at: Option<Instant>,
}

/// Caches model metadata fetched from the upstream GET /v1/models endpoint.
/// Fetching is lazy and single-flight; failures are non-fatal and simply
/// extend the previous cache.
pub struct ModelRegistry {
    state: Mutex<RegistryState>,
    ttl: Duration,
    client: reqwest::Client,
}

impl ModelRegistry {
    pub fn new(ttl: Duration) -> ModelRegistry {
        let models = builtin_models()
            .into_iter()
            .map(|m| (m.id.clone(), m))
            .collect();
        ModelRegistry {
            state: Mutex::new(RegistryState {
                models,
                fetched_at: None,
            }),
            ttl,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client builds"),
        }
    }

    pub fn lookup(&self, model: &str) -> Option<ModelInfo> {
        self.state
            .lock()
            .expect("registry mutex")
            .models
            .get(model)
            .cloned()
    }

    /// Refetches /v1/models when the cache is stale. The inbound client
    /// credential is forwarded for authentication, exactly like the
    /// passthrough proxy does.
    pub async fn ensure_fresh(&self, base_url: &str, auth: &HeaderMap) {
        {
            let mut st = self.state.lock().expect("registry mutex");
            if st.fetched_at.is_some_and(|at| at.elapsed() < self.ttl) {
                return;
            }
            // Mark the attempt now so concurrent/failing requests don't
            // hammer upstream.
            st.fetched_at = Some(Instant::now());
        }

        let mut req = self.client.get(format!("{base_url}/v1/models"));
        for (k, v) in auth {
            req = req.header(k, v);
        }
        let Ok(resp) = req.send().await else {
            return;
        };
        if resp.status() != reqwest::StatusCode::OK {
            return;
        }
        let Ok(body) = resp.bytes().await else {
            return;
        };
        let infos = parse_model_list(&body);
        if infos.is_empty() {
            return;
        }
        let mut st = self.state.lock().expect("registry mutex");
        for m in infos {
            st.models.insert(m.id.clone(), m);
        }
    }
}

/// Tolerates both OpenAI-style {"data": [...]} and bare-array responses, and
/// reads whichever limit fields the upstream provides.
pub fn parse_model_list(body: &[u8]) -> Vec<ModelInfo> {
    let entries: Vec<serde_json::Map<String, Value>> = {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            data: Option<Vec<serde_json::Map<String, Value>>>,
        }
        match serde_json::from_slice::<Wrapper>(body) {
            Ok(w) if w.data.is_some() => w.data.unwrap_or_default(),
            _ => serde_json::from_slice::<Vec<serde_json::Map<String, Value>>>(body)
                .unwrap_or_default(),
        }
    };
    if entries.is_empty() {
        return Vec::new();
    }

    let mut infos = Vec::new();
    for e in &entries {
        let Some(id) = e.get("id").and_then(Value::as_str) else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        infos.push(ModelInfo {
            id: id.to_string(),
            context_window: first_int(
                e,
                &[
                    "context_window",
                    "context_length",
                    "max_context_tokens",
                    "max_input_tokens",
                ],
            ),
            max_output_tokens: first_int(
                e,
                &[
                    "max_output_tokens",
                    "max_tokens",
                    "output_limit",
                    "max_completion_tokens",
                ],
            ),
        });
    }
    infos
}

fn first_int(e: &serde_json::Map<String, Value>, keys: &[&str]) -> i64 {
    for k in keys {
        match e.get(*k) {
            Some(Value::Number(n)) => {
                if let Some(f) = n.as_f64() {
                    if f > 0.0 {
                        return f as i64;
                    }
                }
            }
            Some(Value::String(s)) => {
                if let Some(n) = parse_leading_int(s) {
                    if n > 0 {
                        return n;
                    }
                }
            }
            _ => {}
        }
    }
    0
}

/// Mirrors fmt.Sscanf("%d"): skips leading whitespace, then reads an optional
/// sign and a run of digits.
fn parse_leading_int(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || (i == 0 && (c == '+' || c == '-')) {
            end = i + 1;
        } else {
            break;
        }
    }
    s[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::convert::build_anthropic_request;
    use crate::adapter::test_config;
    use crate::adapter::types::ResponsesRequest;
    use axum::Router;
    use axum::extract::Request;
    use axum::http::HeaderValue;
    use axum::response::Response;

    #[test]
    fn parse_model_list_openai_style() {
        let body = br#"{"object":"list","data":[
            {"id":"k3-256k","object":"model","context_window":262144,"max_output_tokens":65536},
            {"id":"k3","object":"model","context_length":131072,"max_tokens":16384}
        ]}"#;
        let infos = parse_model_list(body);
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].context_window, 262144);
        assert_eq!(infos[0].max_output_tokens, 65536);
        assert_eq!(infos[1].context_window, 131072);
        assert_eq!(infos[1].max_output_tokens, 16384);
    }

    #[test]
    fn parse_model_list_bare_array() {
        let infos = parse_model_list(br#"[{"id":"kimi-for-coding","max_output_tokens":32768}]"#);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].max_output_tokens, 32768);
    }

    #[tokio::test]
    async fn registry_refresh_overrides_builtins() {
        let app = Router::new().fallback(|req: Request| async move {
            assert_eq!(req.uri().path(), "/v1/models");
            assert_eq!(
                req.headers().get("authorization").unwrap(),
                "Bearer client-key"
            );
            Response::new(axum::body::Body::from(
                r#"{"data":[{"id":"k3-256k","context_window":262144,"max_output_tokens":65536}]}"#,
            ))
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let reg = ModelRegistry::new(Duration::from_secs(60));
        let info = reg.lookup("k3-256k").unwrap();
        assert_eq!(info.max_output_tokens, 32768, "builtin seed wrong");

        let mut auth = HeaderMap::new();
        auth.insert(
            "authorization",
            HeaderValue::from_static("Bearer client-key"),
        );
        reg.ensure_fresh(&format!("http://{addr}"), &auth).await;

        let info = reg.lookup("k3-256k").expect("model still present");
        assert_eq!(
            info.max_output_tokens, 65536,
            "refresh did not override builtin"
        );
    }

    #[tokio::test]
    async fn registry_failure_keeps_cache() {
        let app = Router::new().fallback(|| async {
            Response::builder()
                .status(500)
                .body(axum::body::Body::empty())
                .unwrap()
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let reg = ModelRegistry::new(Duration::from_secs(60));
        reg.ensure_fresh(&format!("http://{addr}"), &HeaderMap::new())
            .await;
        let info = reg.lookup("k3").expect("builtin survives failed refresh");
        assert_eq!(info.max_output_tokens, 32768);
    }

    #[test]
    fn max_tokens_precedence() {
        let cfg = test_config();
        let resolver = |model: &str| {
            Some(ModelInfo {
                id: model.to_string(),
                context_window: 0,
                max_output_tokens: 65536,
            })
        };

        // Client value wins over model metadata.
        let req: ResponsesRequest =
            serde_json::from_str(r#"{"model":"k3","input":"hi","max_output_tokens":4096}"#)
                .unwrap();
        let out = build_anthropic_request(&cfg, &req, Some(&resolver)).unwrap();
        assert_eq!(out.max_tokens, 4096, "client max_output_tokens should win");

        // Model metadata wins over the global default.
        let req: ResponsesRequest = serde_json::from_str(r#"{"model":"k3","input":"hi"}"#).unwrap();
        let out = build_anthropic_request(&cfg, &req, Some(&resolver)).unwrap();
        assert_eq!(
            out.max_tokens, 65536,
            "model metadata should win over KIMI_MAX_TOKENS"
        );

        // Global default applies when no metadata exists.
        let out = build_anthropic_request(&cfg, &req, None).unwrap();
        assert_eq!(out.max_tokens, cfg.max_tokens);
    }
}
