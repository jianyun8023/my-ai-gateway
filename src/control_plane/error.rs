use super::model_catalog;
use crate::infra::secrets::SecretResolverError;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub(crate) enum ControlPlaneError {
    Database(sqlx::Error),
    Json(serde_json::Error),
    NotFound(String),
    Conflict(String),
    Validation(Vec<String>),
    Credential(SecretResolverError),
    NoCiphertext,
}

impl ControlPlaneError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Database(_) => "database_error",
            Self::Json(_) | Self::Validation(_) => "validation_failed",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Credential(error) => error.code(),
            Self::NoCiphertext => "no_ciphertext",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Validation(errors) => errors.join("; "),
            _ => self.to_string(),
        }
    }
}

impl fmt::Display for ControlPlaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::NotFound(message) | Self::Conflict(message) => f.write_str(message),
            Self::Validation(errors) => f.write_str(&errors.join("; ")),
            Self::Credential(error) => f.write_str(error.public_message()),
            Self::NoCiphertext => f.write_str("account does not have an encrypted credential"),
        }
    }
}

impl Error for ControlPlaneError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Credential(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for ControlPlaneError {
    fn from(value: sqlx::Error) -> Self {
        if let sqlx::Error::Database(database) = &value {
            match database.code().as_deref() {
                Some("23505" | "40001") => return Self::Conflict(database.message().to_owned()),
                Some("23503" | "23514" | "22P02") => {
                    return Self::Validation(vec![database.message().to_owned()])
                }
                _ => {}
            }
        }
        Self::Database(value)
    }
}

impl From<serde_json::Error> for ControlPlaneError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<SecretResolverError> for ControlPlaneError {
    fn from(error: SecretResolverError) -> Self {
        Self::Credential(error)
    }
}

impl From<model_catalog::CatalogError> for ControlPlaneError {
    fn from(error: model_catalog::CatalogError) -> Self {
        match error {
            model_catalog::CatalogError::Database(error) => ControlPlaneError::Database(error),
            model_catalog::CatalogError::Json(error) => ControlPlaneError::Json(error),
            model_catalog::CatalogError::NotFound(message) => ControlPlaneError::NotFound(message),
            model_catalog::CatalogError::InvalidMetadata(message)
            | model_catalog::CatalogError::InvalidState(message)
            | model_catalog::CatalogError::ImmutableVersionConflict(message) => {
                ControlPlaneError::Validation(vec![message])
            }
        }
    }
}
