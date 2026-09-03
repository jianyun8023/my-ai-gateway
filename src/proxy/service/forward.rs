async fn forward_account(
    _config: &GatewayConfig,
    secrets: &secrets::SecretResolver,
    http: &SourceHttpClient,
    route: &ResolvedRoute,
    provider: &config::ProviderConfig,
    account: &config::AccountConfig,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    let credential = resolve_credential(secrets, account);
    if route.mode == "adapter" {
        if route.adapter.as_deref() == Some("kimi_responses_adapter") {
            return embedded_kimi_adapter(
                http,
                provider,
                account,
                credential.as_deref(),
                headers,
                body,
                stream_config,
                request_started,
            )
            .await;
        }
        Err(transport::TransportError::Request)
    } else {
        transport::forward_url_with_config(
            http,
            &route.upstream_endpoint,
            account,
            credential.as_deref(),
            route.protocol_upstream,
            headers,
            body,
            stream_config,
            request_started,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn embedded_kimi_adapter(
    http: &SourceHttpClient,
    provider: &config::ProviderConfig,
    _account: &config::AccountConfig,
    credential: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
    stream_config: &stream::StreamConfig,
    request_started: Instant,
) -> Result<Response<Body>, transport::TransportError> {
    http.validate_base_url(&provider.base_url)?;
    // The embedded adapter does not pass through the native transport helper,
    // so retain the original request and explicitly attach the same usage
    // report for completed JSON responses.
    let request_body = body.clone();
    let is_streaming = serde_json::from_slice::<Value>(&request_body)
        .ok()
        .and_then(|value| value.get("stream").and_then(Value::as_bool))
        .unwrap_or(false);
    let cfg = kimi_responses_adapter::adapter::config::Config {
        listen_addr: String::new(),
        kimi_base_url: provider.base_url.trim_end_matches('/').to_string(),
        anthropic_beta: String::new(),
        model_map: Default::default(),
        client_source: String::new(),
        models: provider.models.clone(),
        max_tokens: 32768,
        thinking_budgets: [
            ("low".into(), 4096),
            ("medium".into(), 16384),
            ("high".into(), 32768),
        ]
        .into_iter()
        .collect(),
        search_status_prefix: "Search results for query:".into(),
        stream_config: kimi_responses_adapter::adapter::config::StreamConfig::from_durations(
            stream_config.heartbeat_interval,
            stream_config.connection_timeout,
            stream_config.first_event_timeout,
            stream_config.idle_timeout,
            stream_config.total_timeout,
        ),
    };
    let adapter =
        kimi_responses_adapter::adapter::server::router_with_client(cfg, http.raw_client());
    let mut request = Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .body(Body::from(body))
        .map_err(|_| transport::TransportError::Request)?;
    request
        .extensions_mut()
        .insert(kimi_responses_adapter::adapter::server::StreamRequestStart(
            request_started,
        ));
    let request_headers = request.headers_mut();
    for (name, value) in headers {
        if !matches!(
            name.as_str(),
            "host" | "content-length" | "authorization" | "x-api-key"
        ) {
            request_headers.insert(name, value.clone());
        }
    }
    if let Some(value) = credential {
        if headers.contains_key("x-api-key") {
            if let Ok(value) = HeaderValue::from_str(value) {
                request_headers.insert("x-api-key", value);
            }
        } else if let Ok(value) = HeaderValue::from_str(&format!("Bearer {value}")) {
            request_headers.insert("authorization", value);
        }
    }
    let response = adapter
        .oneshot(request)
        .await
        .map_err(|_| transport::TransportError::Request)?;
    if is_streaming || is_event_stream(&response) {
        return Ok(response);
    }
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 16 * 1024 * 1024)
        .await
        .map_err(|_| transport::TransportError::Request)?;
    let report = usage::usage_for_json_response(parts.status.is_success(), &request_body, &bytes);
    let mut response = Response::from_parts(parts, Body::from(bytes));
    response.extensions_mut().insert(report);
    Ok(response)
}

struct FallbackCandidate<'a> {
    account: &'a config::AccountConfig,
    provider: &'a config::ProviderConfig,
    provider_id: String,
    source_id: String,
    upstream_model: String,
    protocol_upstream: Protocol,
    mode: String,
    adapter: Option<String>,
    upstream_endpoint: Option<String>,
    degraded_features: Vec<String>,
}
