use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use pgpanel_core::error::Error as CoreError;
use pgpanel_core::models::ApiErrorBody;

pub struct AppError(pub CoreError);

impl From<CoreError> for AppError {
    fn from(e: CoreError) -> Self {
        Self(e)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        Self(CoreError::Internal(e.to_string()))
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = ApiErrorBody {
            error: self.0.to_string(),
            code: self.0.error_code().to_string(),
            details: None,
        };
        // Never leak internal crypto/stack details for 500 in production-ish way:
        let body = if status.is_server_error() {
            match &self.0 {
                CoreError::Internal(_) | CoreError::Other(_) => ApiErrorBody {
                    error: "internal server error".into(),
                    code: body.code,
                    details: None,
                },
                _ => body,
            }
        } else {
            body
        };
        tracing::error!(code = %body.code, error = %self.0, "request error");
        (status, Json(body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, AppError>;
