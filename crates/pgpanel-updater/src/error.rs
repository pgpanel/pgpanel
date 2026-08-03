//! Updater error types.

use thiserror::Error;

/// Result alias for updater operations.
pub type UpdaterResult<T> = Result<T, UpdaterError>;

/// Structured updater errors.
#[derive(Debug, Error)]
pub enum UpdaterError {
    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),

    /// Release / GitHub API error.
    #[error("release error: {0}")]
    Release(String),

    /// Network / download error.
    #[error("network error: {0}")]
    Network(String),

    /// Download size / integrity error.
    #[error("download error: {0}")]
    Download(String),

    /// Checksum / signature verification error.
    #[error("verification error: {0}")]
    Verify(String),

    /// Manifest validation error.
    #[error("manifest error: {0}")]
    Manifest(String),

    /// Archive extraction error.
    #[error("archive error: {0}")]
    Archive(String),

    /// Deployment error.
    #[error("deployment error: {0}")]
    Deploy(String),

    /// Health check error.
    #[error("health check error: {0}")]
    Health(String),

    /// Caddy control error.
    #[error("caddy error: {0}")]
    Caddy(String),

    /// Update lock error.
    #[error("lock error: {0}")]
    Lock(String),

    /// Version policy error.
    #[error("version error: {0}")]
    Version(String),

    /// Rollback error.
    #[error("rollback error: {0}")]
    Rollback(String),

    /// Not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Unix-socket protocol or daemon response error.
    #[error("protocol error: {0}")]
    Protocol(String),
}
