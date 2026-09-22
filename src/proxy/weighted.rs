//! Preferred account first, then one weighted fallback or short Retry-After retry.
use super::attempt::AttemptContext;
use super::completion::{FinalUpstream, RequestCompletion};
use super::fallback::{select_fallback_candidate, FallbackCandidate};
use super::policy::{
    is_retryable, primary_unavailable_reason, retry_after_delay, transport_error_status,
    warn_degraded_features,
};
use super::transport;
use crate::domain::config::GatewayConfig;
use crate::http::response::data_plane_error_response;
use axum::body::Body;
use axum::http::{Response, StatusCode};

pub(super) async fn proxy_weighted(
    config: &GatewayConfig,
    context: &AttemptContext<'_>,
    completion: RequestCompletion<'_>,
) -> Response<Body> {
    let route = completion.route;
    let error_response = |status: StatusCode, code: &str, message: &str| {
        data_plane_error_response(context.protocol, status, code, message, context.request_id)
    };
    let Some(provider) = config.provider(&route.source_id) else {
        return super::service::finish_proxy(
            context.protocol,
            context.model,
            context.started,
            completion.is_streamed,
            error_response(
                StatusCode::BAD_GATEWAY,
                "provider_not_found",
                "route references an unknown provider",
            ),
        );
    };
    let Some(account) = config.account(&route.primary_account_id) else {
        return super::service::finish_proxy(
            context.protocol,
            context.model,
            context.started,
            completion.is_streamed,
            error_response(
                StatusCode::BAD_GATEWAY,
                "account_not_found",
                "route references an unknown account",
            ),
        );
    };
    let health = context.health.get_health(&account.id).await;
    let unavailable_reason = if !account.enabled {
        Some("account_disabled".to_owned())
    } else if !health.available {
        Some(primary_unavailable_reason(&health))
    } else {
        None
    };
    let unavailable_response = || {
        error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            if account.enabled {
                "account_cooling_down"
            } else {
                "account_disabled"
            },
            "primary account is unavailable and no fallback succeeded",
        )
    };
    if let Some(reason) = unavailable_reason {
        let Some(candidate) = select_fallback_candidate(
            config,
            context.health,
            route,
            context.model,
            context.protocol,
        )
        .await
        else {
            return completion
                .finish(
                    unavailable_response(),
                    FinalUpstream::route(route),
                    vec![],
                    context.body.clone(),
                    Some(reason),
                    None,
                )
                .await;
        };
        if !route.is_degraded() && !candidate.degraded_features.is_empty() {
            warn_degraded_features(
                context.request_id,
                &route.route_id,
                &candidate.degraded_features,
            );
        }
        let attempted = context.execute(&candidate, 0, true).await;
        let response = attempted.result.unwrap_or_else(|error| {
            if matches!(error, transport::TransportError::Timeout(_)) {
                error_response(
                    transport_error_status(&error),
                    "upstream_request_failed",
                    error.message(),
                )
            } else {
                unavailable_response()
            }
        });
        return completion
            .finish(
                response,
                FinalUpstream::candidate(&candidate),
                vec![attempted.usage],
                attempted.request_body,
                Some(reason),
                None,
            )
            .await;
    }
    let primary = FallbackCandidate::primary(route, account, provider, context.model);
    let attempted = context.execute(&primary, 0, false).await;
    let mut attempts = vec![attempted.usage];
    let request_body = context.body.clone();
    let mut final_candidate = &primary;
    let fallback;
    let mut fallback_reason = None;
    let response = match attempted.result {
        Ok(response) if is_retryable(response.status()) => {
            fallback_reason = Some(format!("upstream_http_{}", response.status().as_u16()));
            let retry_after = (response.status() == StatusCode::TOO_MANY_REQUESTS)
                .then(|| retry_after_delay(response.headers()))
                .flatten();
            let fallback_available = select_fallback_candidate(
                config,
                context.health,
                route,
                context.model,
                context.protocol,
            )
            .await
            .is_some();
            if let Some(delay) = retry_after.filter(|_| !fallback_available) {
                tokio::time::sleep(delay).await;
                let retried = context.execute(&primary, attempts.len(), false).await;
                attempts.push(retried.usage);
                retried.result.unwrap_or_else(|error| {
                    error_response(
                        transport_error_status(&error),
                        "upstream_request_failed",
                        error.message(),
                    )
                })
            } else if let Some(candidate) = select_fallback_candidate(
                config,
                context.health,
                route,
                context.model,
                context.protocol,
            )
            .await
            {
                fallback = candidate;
                final_candidate = &fallback;
                let attempted = context.execute(&fallback, attempts.len(), true).await;
                attempts.push(attempted.usage);
                // An HTTP primary failure remains the client response if the fallback transport fails.
                attempted.result.unwrap_or(response)
            } else {
                response
            }
        }
        Ok(response) => response,
        Err(error) => {
            fallback_reason = Some("upstream_transport_error".into());
            if let Some(candidate) = select_fallback_candidate(
                config,
                context.health,
                route,
                context.model,
                context.protocol,
            )
            .await
            {
                fallback = candidate;
                final_candidate = &fallback;
                let attempted = context.execute(&fallback, attempts.len(), true).await;
                attempts.push(attempted.usage);
                attempted.result.unwrap_or_else(|error| {
                    error_response(
                        transport_error_status(&error),
                        "upstream_request_failed",
                        error.message(),
                    )
                })
            } else {
                error_response(
                    transport_error_status(&error),
                    "upstream_request_failed",
                    error.message(),
                )
            }
        }
    };
    if attempts.len() > 1 && !route.is_degraded() && !final_candidate.degraded_features.is_empty() {
        warn_degraded_features(
            context.request_id,
            &route.route_id,
            &final_candidate.degraded_features,
        );
    }
    let fallback_reason = (attempts.len() > 1).then_some(fallback_reason).flatten();
    completion
        .finish(
            response,
            FinalUpstream::candidate(final_candidate),
            attempts,
            request_body,
            fallback_reason,
            None,
        )
        .await
}
