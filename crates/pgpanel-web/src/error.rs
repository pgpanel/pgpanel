//! Application error types.

use askama::Template;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use pgpanel_core::CoreError;
use pgpanel_protocol::HelperErrorBody;
use serde::Serialize;
use thiserror::Error;

/// Application result alias.
pub type AppResult<T> = Result<T, AppError>;

/// HTTP status category for structured user-facing errors.
#[derive(Debug, Clone, Copy)]
enum DetailedErrorKind {
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Internal,
}

impl DetailedErrorKind {
    fn status_code(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// Application errors.
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

    #[error("{message}")]
    Detailed {
        message: String,
        details: Option<String>,
        status: StatusCode,
    },

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
            Self::Detailed {
                message, status, ..
            } => {
                if *status == StatusCode::INTERNAL_SERVER_ERROR {
                    "An internal error occurred".into()
                } else {
                    message.clone()
                }
            }
            Self::Core(e) => e.user_message(),
            Self::Db(_) => "Database error".into(),
            Self::Helper(_) => "Helper communication error".into(),
            Self::Postgres(e) => format!("PostgreSQL error: {}", e),
            Self::Updater(e) => e.to_string(),
            other => other.to_string(),
        }
    }

    /// Optional diagnostic details safe for authenticated administrators.
    pub fn details(&self) -> Option<&str> {
        match self {
            Self::Detailed { details, .. } => details.as_deref().filter(|d| !d.trim().is_empty()),
            _ => None,
        }
    }

    /// Map a helper operation failure to an HTTP-facing error.
    pub fn from_helper_error(error: HelperErrorBody) -> Self {
        use pgpanel_protocol::HelperErrorCode;

        let details = error
            .details
            .map(|d| sanitize_diagnostic_details(&d))
            .filter(|d| !d.trim().is_empty());

        let (kind, message) = match error.code {
            HelperErrorCode::InvalidInput
            | HelperErrorCode::VersionNotInstalled
            | HelperErrorCode::ConfigInvalid
            | HelperErrorCode::ConfirmationFailed
            | HelperErrorCode::InsufficientDisk
            | HelperErrorCode::CommandFailed => (DetailedErrorKind::BadRequest, error.message),
            HelperErrorCode::NotFound => (DetailedErrorKind::NotFound, error.message),
            HelperErrorCode::AlreadyExists | HelperErrorCode::PortInUse => {
                (DetailedErrorKind::Conflict, error.message)
            }
            HelperErrorCode::Forbidden => (DetailedErrorKind::Forbidden, error.message),
            HelperErrorCode::Unauthorized => (DetailedErrorKind::Unauthorized, error.message),
            HelperErrorCode::Timeout | HelperErrorCode::Internal | HelperErrorCode::Protocol => {
                (DetailedErrorKind::Internal, error.message)
            }
        };

        Self::Detailed {
            message,
            details,
            status: kind.status_code(),
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
            Self::Detailed { status, .. } => *status,
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

fn sanitize_diagnostic_details(raw: &str) -> String {
    const MAX_LEN: usize = 500;
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        }
        out.push(ch);
        if out.len() >= MAX_LEN {
            out.push('…');
            break;
        }
    }
    out.trim().to_string()
}

/// JSON error body.
#[derive(Serialize)]
struct ErrorBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ErrorBody {
            error: self.user_message(),
            details: self.details().map(str::to_string),
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;
    use pgpanel_protocol::HelperErrorCode;

    #[test]
    fn from_helper_error_maps_port_in_use_to_conflict() {
        let err = AppError::from_helper_error(HelperErrorBody {
            code: HelperErrorCode::PortInUse,
            message: "port 5432 is already in use".into(),
            details: Some("Error: port 5432 already in use".into()),
        });
        assert_eq!(err.user_message(), "port 5432 is already in use");
        assert_eq!(err.details(), Some("Error: port 5432 already in use"));
        assert_eq!(err.into_response().status(), StatusCode::CONFLICT);
    }

    #[test]
    fn from_helper_error_maps_version_not_installed_to_bad_request() {
        let err = AppError::from_helper_error(HelperErrorBody {
            code: HelperErrorCode::VersionNotInstalled,
            message: "PostgreSQL 99 is not installed".into(),
            details: Some("could not find version 99".into()),
        });
        assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn sanitize_diagnostic_details_strips_control_chars() {
        assert_eq!(sanitize_diagnostic_details("bad\x07news"), "badnews");
    }

    #[tokio::test]
    async fn json_error_body_includes_details() {
        let err = AppError::from_helper_error(HelperErrorBody {
            code: HelperErrorCode::CommandFailed,
            message: "initdb: permission denied".into(),
            details: Some("initdb: permission denied".into()),
        });
        let response = err.into_response();
        let body = response.into_body();
        let bytes = body.collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "initdb: permission denied");
        assert_eq!(json["details"], "initdb: permission denied");
    }
}
