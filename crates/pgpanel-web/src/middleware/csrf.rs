//! CSRF protection middleware.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use pgpanel_core::auth::tokens_equal;

/// Validate CSRF on POST/PUT/PATCH/DELETE requests.
pub async fn csrf_middleware(
    State(_state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let method = request.method().clone();
    if method == axum::http::Method::GET
        || method == axum::http::Method::HEAD
        || method == axum::http::Method::OPTIONS
    {
        return Ok(next.run(request).await);
    }

    let path = request.uri().path();
    if path.starts_with("/health") || path.starts_with("/static") {
        return Ok(next.run(request).await);
    }

    // Setup and login don't require CSRF (no session yet)
    if path == "/setup" || path == "/auth/login" {
        return Ok(next.run(request).await);
    }

    let user = request.extensions().get::<AuthUser>().cloned();

    let Some(user) = user else {
        return Ok(next.run(request).await);
    };

    let content_type = request
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if content_type.starts_with("application/x-www-form-urlencoded") {
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, 1024 * 64)
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        let body_str = String::from_utf8_lossy(&bytes);

        let token = extract_csrf_from_form(&body_str, &content_type);
        validate_csrf(&user, token.as_deref())?;

        let request = Request::from_parts(parts, Body::from(bytes));
        return Ok(next.run(request).await);
    }

    // Check header for JSON/API requests
    if let Some(header) = request
        .headers()
        .get("X-CSRF-Token")
        .and_then(|v| v.to_str().ok())
    {
        validate_csrf(&user, Some(header))?;
        return Ok(next.run(request).await);
    }

    // HTMX sends header
    if let Some(header) = request
        .headers()
        .get("HX-CSRF-Token")
        .and_then(|v| v.to_str().ok())
    {
        validate_csrf(&user, Some(header))?;
        return Ok(next.run(request).await);
    }

    Err(AppError::Forbidden("CSRF token missing or invalid".into()))
}

fn extract_csrf_from_form(body: &str, content_type: &str) -> Option<String> {
    if content_type.starts_with("application/x-www-form-urlencoded") {
        return url::form_urlencoded::parse(body.as_bytes())
            .find(|(key, _)| key == "csrf_token")
            .map(|(_, value)| value.into_owned());
    }
    None
}

fn validate_csrf(user: &AuthUser, token: Option<&str>) -> AppResult<()> {
    let Some(token) = token else {
        return Err(AppError::Forbidden("CSRF token missing".into()));
    };
    if tokens_equal(token, &user.session.csrf_token) {
        Ok(())
    } else {
        Err(AppError::Forbidden("CSRF token invalid".into()))
    }
}
