//! Admin and data-plane identities. Database access is explicit; authentication
//! does not depend on the HTTP router or mutable runtime configuration.
use axum::http::HeaderMap;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::infra::db;

#[derive(Clone)]
pub(crate) struct AdminAuth {
    key_digest: Option<[u8; 32]>,
}

impl AdminAuth {
    pub(crate) fn from_env() -> Self {
        Self::from_key(
            std::env::var("GATEWAY_ADMIN_KEY")
                .ok()
                .filter(|key| !key.is_empty())
                .as_deref(),
        )
    }

    pub(crate) fn from_key(key: Option<&str>) -> Self {
        Self {
            key_digest: key.map(key_digest),
        }
    }

    pub(crate) fn is_configured(&self) -> bool {
        self.key_digest.is_some()
    }

    pub(crate) fn authorized(&self, headers: &HeaderMap) -> bool {
        let (Some(expected), Some(supplied)) = (self.key_digest, supplied_key(headers)) else {
            return false;
        };
        key_matches_digest(&expected, supplied)
    }

    #[cfg(test)]
    pub(crate) fn test() -> Self {
        Self::from_key(Some(crate::test_helpers::TEST_ADMIN_KEY))
    }
}

pub(crate) fn key_digest(key: &str) -> [u8; 32] {
    Sha256::digest(key.as_bytes()).into()
}

pub(crate) fn key_matches_digest(expected: &[u8; 32], supplied: &str) -> bool {
    bool::from(expected.ct_eq(&key_digest(supplied)))
}

/// Authentication identity resolved during data-plane authorization.
#[derive(Clone, Debug)]
pub(crate) enum AuthIdentity {
    /// Matched the static `GATEWAY_API_KEY` environment variable.
    StaticApiKey,
    /// Matched a database-backed Virtual Key.
    VirtualKey {
        id: i64,
        name: String,
        prefix: String,
    },
}

impl AuthIdentity {
    pub(crate) fn virtual_key_id(&self) -> Option<i64> {
        match self {
            Self::VirtualKey { id, .. } => Some(*id),
            _ => None,
        }
    }

    /// Default client_source value derived from auth identity.
    pub(crate) fn default_client_source(&self) -> String {
        match self {
            Self::StaticApiKey => "static_api_key".to_owned(),
            Self::VirtualKey { name, prefix, .. } => {
                if name.is_empty() {
                    prefix.clone()
                } else {
                    name.clone()
                }
            }
        }
    }
}

pub(crate) async fn authorized_with_db(
    database: Option<&db::Database>,
    headers: &HeaderMap,
    model: Option<&str>,
) -> Option<AuthIdentity> {
    if let Ok(expected) = std::env::var("GATEWAY_API_KEY") {
        if supplied_key(headers)
            .is_some_and(|supplied| key_matches_digest(&key_digest(&expected), supplied))
        {
            return Some(AuthIdentity::StaticApiKey);
        }
    }
    let Some(database) = database else {
        return std::env::var("GATEWAY_API_KEY")
            .is_err()
            .then_some(AuthIdentity::StaticApiKey);
    };
    let key = supplied_key(headers)?;
    database
        .authenticate_virtual_key_with_identity(key, model)
        .await
        .ok()
        .flatten()
        .map(|(id, name, prefix)| AuthIdentity::VirtualKey { id, name, prefix })
}

pub(crate) fn supplied_key(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
        })
}
