use crate::source_url::{SourceUrlPolicy, SourceUrlPolicyError};
use reqwest::{Client, Method, RequestBuilder, Url};
use std::sync::Arc;

/// HTTP client shared by the control-plane discovery flow and the data plane.
///
/// Keeping the client beside the URL policy makes the network boundary usable
/// by both layers without making either one depend on the proxy module.
#[derive(Clone)]
pub struct SourceHttpClient {
    inner: Client,
    policy: Arc<SourceUrlPolicy>,
}

impl SourceHttpClient {
    pub fn request(
        &self,
        method: Method,
        url: Url,
    ) -> Result<RequestBuilder, SourceUrlPolicyError> {
        self.policy.validate_request_url(&url)?;
        Ok(self.inner.request(method, url))
    }

    pub fn post(&self, url: &str) -> Result<RequestBuilder, SourceUrlPolicyError> {
        let url = self.policy.parse_request_url(url)?;
        Ok(self.inner.post(url))
    }

    #[cfg(test)]
    pub fn get(&self, url: &str) -> Result<RequestBuilder, SourceUrlPolicyError> {
        let url = self.policy.parse_request_url(url)?;
        Ok(self.inner.get(url))
    }

    pub fn validate_base_url(&self, value: &str) -> Result<Url, SourceUrlPolicyError> {
        self.policy.validate_base_url(value)
    }

    pub fn raw_client(&self) -> Client {
        self.inner.clone()
    }
}

pub fn client(policy: Arc<SourceUrlPolicy>) -> Result<SourceHttpClient, reqwest::Error> {
    let redirect = policy.redirect_policy();
    let resolver = Arc::new(policy.dns_resolver());
    let inner = Client::builder()
        // The stream contract owns connect/idle/total deadlines. Reqwest's
        // defaults are already unlimited, so no client-wide timeout is set;
        // this keeps the phases distinguishable.
        .no_proxy()
        .redirect(redirect)
        .dns_resolver(resolver)
        .build()?;
    Ok(SourceHttpClient { inner, policy })
}

#[cfg(test)]
pub fn test_client() -> Result<SourceHttpClient, reqwest::Error> {
    client(crate::source_url::test_policy())
}
