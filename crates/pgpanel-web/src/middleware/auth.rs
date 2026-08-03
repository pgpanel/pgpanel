//! Session extraction and authentication middleware.

use crate::error::{AppError, AppResult};
use crate::services::session::SessionData;
use crate::state::AppState;
use axum::{
    extract::{FromRequestParts, State},
    http::request::Parts,
};
use axum_extra::extract::CookieJar;
use pgpanel_core::permissions::{role_has, Permission, Role};

/// Authenticated user context.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub session: SessionData,
}

impl AuthUser {
    /// User ID.
    pub fn user_id(&self) -> i64 {
        self.session.user_id
    }

    /// Username.
    pub fn username(&self) -> &str {
        &self.session.username
    }

    /// Role.
    pub fn role(&self) -> Role {
        self.session.role
    }

    /// Check permission.
    pub fn require(&self, perm: Permission) -> AppResult<()> {
        if role_has(self.role(), perm) {
            Ok(())
        } else {
            Err(AppError::Forbidden(format!(
                "missing permission: {}",
                perm.as_str()
            )))
        }
    }

    /// Whether recent reauthentication is valid.
    pub fn is_recently_authed(&self, reauth_window_secs: u64) -> bool {
        self.session.is_recently_authed(reauth_window_secs)
    }
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::Unauthorized("invalid cookies".into()))?;
        let cookie_name = state.config.session.cookie_name.clone();
        let token = jar
            .get(&cookie_name)
            .map(|c| c.value().to_string())
            .ok_or_else(|| AppError::Unauthorized("not authenticated".into()))?;

        let ip = parts
            .extensions
            .get::<super::request_id::ClientIp>()
            .map(|c| c.0.clone());

        let session = state
            .sessions
            .validate_session(&token, ip.as_deref())
            .await
            .map_err(AppError::from_core)?;

        Ok(AuthUser { session })
    }
}

/// Optional auth — returns None if not logged in.
pub struct OptionalAuth(pub Option<AuthUser>);

#[axum::async_trait]
impl FromRequestParts<AppState> for OptionalAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(OptionalAuth(Some(user))),
            Err(AppError::Unauthorized(_)) => Ok(OptionalAuth(None)),
            Err(e) => Err(e),
        }
    }
}

/// Extension to require setup not complete redirect logic.
pub async fn user_count(state: &AppState) -> AppResult<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    Ok(row.0)
}

/// Redirect unauthenticated users to login.
pub async fn require_auth_redirect(
    State(state): State<AppState>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, AppError> {
    use axum::http::header;
    use axum::response::IntoResponse;

    let path = request.uri().path().to_string();
    let count = user_count(&state).await?;
    if count == 0
        && !path.starts_with("/setup")
        && !path.starts_with("/static")
        && !path.starts_with("/health")
    {
        return Ok(axum::response::Redirect::to("/setup").into_response());
    }

    let cookie_name = state.config.session.cookie_name.clone();
    let token = request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| parse_cookie(cookies, &cookie_name));

    let Some(token) = token else {
        return Ok(axum::response::Redirect::to("/auth/login").into_response());
    };

    let ip = request
        .extensions()
        .get::<super::request_id::ClientIp>()
        .map(|c| c.0.clone());

    match state.sessions.validate_session(&token, ip.as_deref()).await {
        Ok(session) => {
            request.extensions_mut().insert(AuthUser { session });
            Ok(next.run(request).await)
        }
        Err(_) => Ok(axum::response::Redirect::to("/auth/login").into_response()),
    }
}

fn parse_cookie(cookies: &str, name: &str) -> Option<String> {
    for part in cookies.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k == name {
                return Some(v.to_string());
            }
        }
    }
    None
}
