//! Server-side session auth (no JWT for browser sessions).

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::cookie::CookieJar;
use chrono::{Duration, Utc};
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use pgpanel_core::crypto::{
    generate_csrf_token, generate_session_token, hash_password, verify_password,
};
use pgpanel_core::error::Error;
use pgpanel_core::models::{User, UserRole};

use crate::error::AppError;
use crate::state::AppState;

pub struct AuthUser {
    pub user: User,
    #[allow(dead_code)]
    pub session_id: Uuid,
    pub csrf_token: String,
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub async fn bootstrap_needed(state: &AppState) -> Result<bool, Error> {
    let val: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'bootstrap_completed'")
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(val.as_deref() != Some("true"))
}

pub async fn count_users(state: &AppState) -> Result<i64, Error> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(n)
}

pub async fn create_bootstrap_user(
    state: &AppState,
    username: &str,
    email: &str,
    password: &SecretString,
) -> Result<User, Error> {
    if !bootstrap_needed(state).await? {
        return Err(Error::BootstrapAlreadyDone);
    }
    if count_users(state).await? > 0 {
        return Err(Error::BootstrapAlreadyDone);
    }

    if username.len() < 3 || username.len() > 64 {
        return Err(Error::Validation("username must be 3-64 characters".into()));
    }
    if !is_valid_email(email) {
        return Err(Error::Validation(
            "a valid e-mail address is required".into(),
        ));
    }
    pgpanel_core::crypto::validate_password_strength(password.expose_or_str())?;

    let id = Uuid::new_v4();
    let hash = hash_password(password)?;
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, failed_login_attempts, role, display_name, enabled, created_at) VALUES (?, ?, ?, ?, 0, 'owner', ?, 1, ?)",
    )
    .bind(id.to_string())
    .bind(username)
    .bind(email.trim())
    .bind(&hash)
    .bind(username)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| Error::Internal(e.to_string()))?;

    sqlx::query(
        "UPDATE settings SET value = 'true', updated_at = ? WHERE key = 'bootstrap_completed'",
    )
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| Error::Internal(e.to_string()))?;

    Ok(User {
        id,
        username: username.to_string(),
        email: email.trim().to_string(),
        role: UserRole::Owner,
        display_name: username.to_string(),
        enabled: true,
        created_at: Utc::now(),
        last_login_at: None,
    })
}

fn is_valid_email(email: &str) -> bool {
    let email = email.trim();
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.chars().any(char::is_whitespace)
}

// Helper trait for SecretString expose in validation path without logging
trait ExposeOrStr {
    fn expose_or_str(&self) -> &str;
}
impl ExposeOrStr for SecretString {
    fn expose_or_str(&self) -> &str {
        use secrecy::ExposeSecret;
        self.expose_secret()
    }
}

pub async fn login(
    state: &AppState,
    username: &str,
    password: &SecretString,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> Result<(String, String, User), Error> {
    let row = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, email, password_hash, failed_login_attempts, locked_until, created_at, last_login_at, role, display_name, enabled FROM users WHERE username = ? COLLATE NOCASE",
    )
    .bind(username)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| Error::Internal(e.to_string()))?;

    let Some(row) = row else {
        // Constant-ish work to reduce user enumeration timing (best-effort)
        let _ = hash_password(&SecretString::from(
            "dummy-password-not-real-00".to_string(),
        ));
        return Err(Error::Unauthorized);
    };

    if row.enabled == 0 {
        return Err(Error::Forbidden("account disabled".into()));
    }

    if let Some(locked) = &row.locked_until {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(locked) {
            if dt > Utc::now() {
                return Err(Error::LoginLocked(dt.with_timezone(&Utc)));
            }
        }
    }

    let ok = verify_password(password, &row.password_hash)?;
    if !ok {
        let attempts = row.failed_login_attempts + 1;
        let locked_until = if attempts >= state.config.login_max_attempts as i64 {
            Some((Utc::now() + Duration::minutes(state.config.login_lockout_minutes)).to_rfc3339())
        } else {
            None
        };
        sqlx::query("UPDATE users SET failed_login_attempts = ?, locked_until = ? WHERE id = ?")
            .bind(attempts)
            .bind(&locked_until)
            .bind(&row.id)
            .execute(&state.pool)
            .await
            .ok();
        return Err(Error::Unauthorized);
    }

    // Reset lockout
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE users SET failed_login_attempts = 0, locked_until = NULL, last_login_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(&row.id)
    .execute(&state.pool)
    .await
    .ok();

    let token = generate_session_token();
    let token_hash = hash_token(&token);
    let csrf = generate_csrf_token();
    let session_id = Uuid::new_v4();
    let expires = (Utc::now() + Duration::hours(state.config.session_ttl_hours)).to_rfc3339();

    sqlx::query(
        r#"
        INSERT INTO sessions (id, user_id, token_hash, csrf_token, ip_address, user_agent, expires_at, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(session_id.to_string())
    .bind(&row.id)
    .bind(&token_hash)
    .bind(&csrf)
    .bind(ip)
    .bind(user_agent)
    .bind(&expires)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| Error::Internal(e.to_string()))?;

    let user = User {
        id: Uuid::parse_str(&row.id).unwrap_or_default(),
        username: row.username,
        email: row.email,
        role: UserRole::parse(&row.role),
        display_name: row.display_name,
        enabled: row.enabled != 0,
        created_at: parse_dt(&row.created_at),
        last_login_at: Some(Utc::now()),
    };

    Ok((token, csrf, user))
}

pub async fn logout(state: &AppState, token: &str) -> Result<(), Error> {
    let th = hash_token(token);
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(&th)
        .execute(&state.pool)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

pub async fn session_from_token(state: &AppState, token: &str) -> Result<AuthUser, Error> {
    let th = hash_token(token);
    let row = sqlx::query_as::<_, SessionRow>(
        r#"
        SELECT s.id AS session_id, s.csrf_token, s.expires_at,
               u.id AS user_id, u.username, u.created_at, u.last_login_at,
               u.email, u.role, u.display_name, u.enabled
        FROM sessions s
        JOIN users u ON u.id = s.user_id
        WHERE s.token_hash = ?
        "#,
    )
    .bind(&th)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
    .ok_or(Error::Unauthorized)?;

    let expires = chrono::DateTime::parse_from_rfc3339(&row.expires_at)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| Error::Unauthorized)?;
    if expires < Utc::now() {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(&row.session_id)
            .execute(&state.pool)
            .await
            .ok();
        return Err(Error::Unauthorized);
    }
    if row.enabled == 0 {
        return Err(Error::Forbidden("account disabled".into()));
    }

    Ok(AuthUser {
        user: User {
            id: Uuid::parse_str(&row.user_id).unwrap_or_default(),
            username: row.username,
            email: row.email,
            role: UserRole::parse(&row.role),
            display_name: row.display_name,
            enabled: true,
            created_at: parse_dt(&row.created_at),
            last_login_at: row.last_login_at.as_ref().map(|s| parse_dt(s)),
        },
        session_id: Uuid::parse_str(&row.session_id).unwrap_or_default(),
        csrf_token: row.csrf_token,
    })
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let cookie_name = &state.config.cookie_name;
        let token = jar
            .get(cookie_name)
            .map(|c| c.value().to_string())
            .ok_or(AppError(Error::Unauthorized))?;

        let auth = session_from_token(state, &token).await?;

        // CSRF check for unsafe methods
        let method = &parts.method;
        if !matches!(
            *method,
            axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
        ) {
            let csrf_header = parts
                .headers
                .get(&state.config.csrf_header)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if csrf_header.is_empty() || csrf_header != auth.csrf_token {
                return Err(AppError(Error::Forbidden("CSRF token mismatch".into())));
            }
        }

        Ok(auth)
    }
}

/// Reject if the authenticated user lacks admin (or owner) privileges.
pub fn require_admin(auth: &AuthUser) -> Result<(), AppError> {
    if !auth.user.role.can_admin() {
        return Err(AppError(Error::Forbidden("admin role required".into())));
    }
    Ok(())
}

pub fn require_write(auth: &AuthUser) -> Result<(), AppError> {
    if !auth.user.role.can_write() {
        return Err(AppError(Error::Forbidden("write role required".into())));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn write_audit(
    state: &AppState,
    actor: Option<&User>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    details: serde_json::Value,
    ip: Option<&str>,
    correlation_id: Option<&str>,
) {
    let _ = sqlx::query(
        r#"
        INSERT INTO audit_logs (actor_id, actor_username, action, resource_type, resource_id, details, ip_address, correlation_id, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(actor.map(|u| u.id.to_string()))
    .bind(actor.map(|u| u.username.as_str()))
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(details.to_string())
    .bind(ip)
    .bind(correlation_id)
    .bind(Utc::now().to_rfc3339())
    .execute(&state.pool)
    .await;
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    username: String,
    email: String,
    password_hash: String,
    failed_login_attempts: i64,
    locked_until: Option<String>,
    created_at: String,
    #[allow(dead_code)]
    last_login_at: Option<String>,
    role: String,
    display_name: String,
    enabled: i64,
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    session_id: String,
    csrf_token: String,
    expires_at: String,
    user_id: String,
    username: String,
    email: String,
    created_at: String,
    last_login_at: Option<String>,
    role: String,
    display_name: String,
    enabled: i64,
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
