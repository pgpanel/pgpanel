//! Core error types.

use thiserror::Error;

/// Result alias.
pub type CoreResult<T> = Result<T, CoreError>;

/// Structured core errors.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Invalid user input.
    #[error("{0}")]
    InvalidInput(String),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),

    /// Authentication failure (safe message).
    #[error("{0}")]
    Auth(String),

    /// Authorization failure.
    #[error("permission denied: {0}")]
    Forbidden(String),

    /// Not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Rate limited.
    #[error("rate limited: {0}")]
    RateLimited(String),

    /// Internal error (details for logs only via Debug).
    #[error("internal error")]
    Internal(String),

    /// I/O.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// TOML.
    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),
}

impl CoreError {
    /// User-safe message (never secrets).
    pub fn user_message(&self) -> String {
        match self {
            Self::Internal(_) => "An internal error occurred".into(),
            other => other.to_string(),
        }
    }
}
