//! Shared HTTP error envelopes for management and protocol ingress.
use axum::{
    body::Body,
    http::{HeaderValue, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::json;

use crate::domain::protocol::Protocol;

pub(crate) fn error_response(status: StatusCode, code: &str, message: &str) -> Response<Body> {
    (
        status,
        Json(json!({"error":{"code":code,"type":code,"message":message}})),
    )
        .into_response()
}

/// Build a protocol-aware JSON error envelope for the data-plane proxy
/// (`/v1/chat/completions`, `/v1/responses`, `/v1/messages`).
///
/// The Anthropic Messages envelope follows the public Anthropic API contract
/// (`{"type":"error","error":{"type":<sdk_standard>,"code":<internal>,
/// "message":<...>},"request_id":<uuid>}`).  The OpenAI envelopes keep the
/// existing `error.code/type/message` shape for backward compatibility and
/// only attach the optional top-level `request_id` plus the `x-request-id`
/// response header so OpenAI clients ignore unknown fields safely.
///
/// `x-request-id` is always set on the response so SDKs that read it from
/// headers (Anthropic SDK) get a stable correlation id.
pub(crate) fn data_plane_error_response(
    protocol: Protocol,
    status: StatusCode,
    gateway_code: &str,
    message: &str,
    request_id: &str,
) -> Response<Body> {
    let body = match protocol {
        Protocol::AnthropicMessages => {
            let standard_type = anthropic_standard_error_type(gateway_code);
            json!({
                "type": "error",
                "error": {
                    "type": standard_type,
                    "code": gateway_code,
                    "message": message,
                },
                "request_id": request_id,
            })
        }
        Protocol::OpenAiChatCompletions | Protocol::OpenAiResponses => {
            // The existing `error.{code,type,message}` shape is preserved so
            // OpenAI SDKs and clients keep working.  `request_id` is added
            // at the top level; OpenAI clients ignore unknown fields.
            json!({
                "error": {
                    "code": gateway_code,
                    "type": gateway_code,
                    "message": message,
                },
                "request_id": request_id,
            })
        }
    };
    let mut response = (status, Json(body)).into_response();
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

/// Map a gateway internal error code to an Anthropic SDK standard error
/// `type`.  See https://docs.anthropic.com/en/api/errors for the full enum.
fn anthropic_standard_error_type(gateway_code: &str) -> &'static str {
    match gateway_code {
        "invalid_json"
        | "missing_required_parameter"
        | "unsupported_protocol"
        | "lossy_conversion_not_allowed" => "invalid_request_error",
        "unauthorized" => "authentication_error",
        "route_not_found" => "not_found_error",
        "account_cooling_down" => "overloaded_error",
        "upstream_request_failed" => "api_error",
        "provider_not_found"
        | "account_not_found"
        | "fallback_account_not_found"
        | "fallback_provider_mismatch"
        | "account_provider_mismatch"
        | "adapter_missing"
        | "adapter_unknown"
        | "adapter_direction_mismatch"
        | "adapter_capability_mismatch"
        | "adapter_source_unsupported"
        | "invalid_route_mode"
        | "endpoint_missing"
        | "route_unavailable"
        | "account_disabled" => "api_error",
        _ => "api_error",
    }
}
