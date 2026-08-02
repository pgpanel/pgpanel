use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("validation error: {0}")]
    Validation(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("rate limited")]
    RateLimited,

    #[error("login locked out until {0}")]
    LoginLocked(chrono::DateTime<chrono::Utc>),

    #[error("bootstrap already completed")]
    BootstrapAlreadyDone,

    #[error("cluster state error: {0}")]
    ClusterState(String),

    #[error("docker error: {0}")]
    Docker(String),

    #[error("postgres error: {0}")]
    Postgres(String),

    #[error("backup error: {0}")]
    Backup(String),

    /// Legacy alias used during Databasus removal — maps to Backup.
    #[error("backup error: {0}")]
    Databasus(String),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("job error: {0}")]
    Job(String),

    #[error("sql console: {0}")]
    SqlConsole(String),

    #[error("delete protection enabled for cluster")]
    DeleteProtection,

    #[error("confirmation required: {0}")]
    ConfirmationRequired(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Error {
    pub fn status_code(&self) -> u16 {
        match self {
            Self::Validation(_) | Self::ConfirmationRequired(_) => 400,
            Self::Unauthorized | Self::LoginLocked(_) => 401,
            Self::Forbidden(_) | Self::DeleteProtection => 403,
            Self::NotFound(_) => 404,
            Self::Conflict(_) | Self::BootstrapAlreadyDone | Self::ClusterState(_) => 409,
            Self::RateLimited => 429,
            Self::SqlConsole(_) => 422,
            _ => 500,
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "validation_error",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::RateLimited => "rate_limited",
            Self::LoginLocked(_) => "login_locked",
            Self::BootstrapAlreadyDone => "bootstrap_already_done",
            Self::ClusterState(_) => "cluster_state_error",
            Self::Docker(_) => "docker_error",
            Self::Postgres(_) => "postgres_error",
            Self::Backup(_) | Self::Databasus(_) => "backup_error",
            Self::Crypto(_) => "crypto_error",
            Self::Job(_) => "job_error",
            Self::SqlConsole(_) => "sql_console_error",
            Self::DeleteProtection => "delete_protection",
            Self::ConfirmationRequired(_) => "confirmation_required",
            Self::Internal(_) => "internal_error",
            Self::Other(_) => "internal_error",
        }
    }
}
