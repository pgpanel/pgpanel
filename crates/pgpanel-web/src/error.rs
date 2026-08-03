//! Application error types.

use askama::Template;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use pgpanel_core::CoreError;
use serde::Serialize;
use thiserror::Error;

/// Application result alias.
pub type AppResult<T> = Result<T, AppError>;

/// Structured application errors.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Unauthorized(String),

    #[error("{0}")]
    Forbidden(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Conflict(String),

    #[error("{0}")]
    RateLimited(String),

    #[error("internal error")]
    Internal(String),

    #[error(transparent)]
    Core(#[from] CoreError),

    #[error(transparent)]
    Db(#[from] sqlx::Error),

    #[error(transparent)]
    Helper(#[from] pgpanel_protocol::ProtocolError),

    #[error(transparent)]
    Postgres(#[from] tokio_postgres::Error),

    #[error(transparent)]
    Updater(#[from] pgpanel_updater::UpdaterError),
}

impl AppError {
    /// User-safe message.
    pub fn user_message(&self) -> String {
        match self {
            Self::Internal(_) => "An internal error occurred".into(),
            Self::Core(e) => e.user_message(),
            Self::Db(_) => "Database error".into(),
            Self::Helper(_) => "Helper communication error".into(),
            Self::Postgres(e) => format!("PostgreSQL error: {}", e),
            Self::Updater(e) => e.to_string(),
            other => other.to_string(),
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) | Self::Core(CoreError::InvalidInput(_)) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) | Self::Core(CoreError::Auth(_)) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) | Self::Core(CoreError::Forbidden(_)) => StatusCode::FORBIDDEN,
            Self::NotFound(_) | Self::Core(CoreError::NotFound(_)) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::RateLimited(_) | Self::Core(CoreError::RateLimited(_)) => {
                StatusCode::TOO_MANY_REQUESTS
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Convert from core error.
    pub fn from_core(err: CoreError) -> Self {
        match &err {
            CoreError::InvalidInput(_) => Self::BadRequest(err.user_message()),
            CoreError::Auth(_) => Self::Unauthorized(err.user_message()),
            CoreError::Forbidden(_) => Self::Forbidden(err.user_message()),
            CoreError::NotFound(_) => Self::NotFound(err.user_message()),
            CoreError::RateLimited(_) => Self::RateLimited(err.user_message()),
            CoreError::Config(_)
            | CoreError::Internal(_)
            | CoreError::Io(_)
            | CoreError::Serde(_)
            | CoreError::Toml(_) => Self::Internal(err.user_message()),
        }
    }
}

/// JSON error body.
#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ErrorBody {
            error: self.user_message(),
        };
        (status, Json(body)).into_response()
    }
}

/// HTML error from template.
pub struct HtmlError {
    pub status: StatusCode,
    pub message: String,
}

impl<T: Template> From<(StatusCode, T)> for HtmlError {
    fn from((status, _): (StatusCode, T)) -> Self {
        Self {
            status,
            message: "An error occurred".into(),
        }
    }
}

impl IntoResponse for HtmlError {
    fn into_response(self) -> Response {
        let body = format!(
            r#"<!DOCTYPE html><html><head><title>Error</title></head><body><h1>{}</h1><p>{}</p></body></html>"#,
            self.status.as_u16(),
            html_escape::encode_text(&self.message)
        );
        (
            self.status,
            [(axum::http::header::CONTENT_TYPE, "text/html")],
            body,
        )
            .into_response()
    }
}

mod html_escape {
    pub fn encode_text(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                '<' => "&lt;".into(),
                '>' => "&gt;".into(),
                '&' => "&amp;".into(),
                '"' => "&quot;".into(),
                _ => c.to_string(),
            })
            .collect()
    }
}
