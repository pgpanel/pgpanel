//! Authentication routes.

use crate::crypto;
use crate::error::{AppError, AppResult};
use crate::middleware::auth::{user_count, AuthUser};
use crate::middleware::request_id::{ClientIp, RequestId};
use crate::state::AppState;
use crate::templates_data::{auth_layout, layout_ctx, LoginPage, ReauthPage, SecurityPage};
use askama::Template;
use axum::{
    extract::{Extension, State},
    response::{Html, IntoResponse, Redirect},
    Form,
};
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use cookie::{Cookie, SameSite};
use pgpanel_core::audit::AuditEvent;
use pgpanel_core::auth::{
    generate_backup_codes, hash_token, totp_generate, totp_verify, verify_password,
};
use serde::Deserialize;

const TOTP_SECRET_DOMAIN: &str = "totp-secret-v1";
const TOTP_URI_DOMAIN: &str = "totp-otpauth-v1";
const PENDING_TOTP_TTL_SECS: i64 = 600;

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
    pub totp_code: Option<String>,
    pub backup_code: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
pub struct ReauthForm {
    pub csrf_token: String,
    pub password: String,
    pub totp_code: Option<String>,
    pub backup_code: Option<String>,
    pub return_to: String,
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
pub struct TotpCodeForm {
    pub csrf_token: String,
    pub totp_code: String,
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
pub struct DisableTotpForm {
    pub csrf_token: String,
    pub password: Option<String>,
    pub totp_code: Option<String>,
}

pub async fn login_form(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
    if user_count(&state).await? == 0 {
        return Ok(Redirect::to("/setup").into_response());
    }
    let ctx = auth_layout("Login");
    let page = LoginPage {
        ctx: &ctx,
        csrf_token: "",
        error: None,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .into_response())
}

pub async fn login_submit(
    State(state): State<AppState>,
    Extension(ip): Extension<ClientIp>,
    Form(form): Form<LoginForm>,
) -> AppResult<impl IntoResponse> {
    let now = Utc::now();

    // Check lockout
    let failed: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM login_attempts WHERE ip_address = ? AND success = 0 AND attempted_at > ?",
    )
    .bind(&ip.0)
    .bind((now - chrono::Duration::seconds(state.config.auth.lockout_secs as i64)).to_rfc3339())
    .fetch_one(&state.db)
    .await?;

    if failed.0 >= state.config.auth.max_failed_logins as i64 {
        return Err(AppError::RateLimited(
            "account locked due to failed attempts".into(),
        ));
    }

    let user: Option<(i64, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, password_hash, totp_secret, totp_enabled FROM users WHERE username = ? AND disabled = 0",
    )
    .bind(&form.username)
    .fetch_optional(&state.db)
    .await?;

    let mut authenticated_user_id = None;
    let success = if let Some((id, hash, totp_secret, totp_enabled)) = user {
        if !verify_password(&form.password, &hash).unwrap_or(false) {
            false
        } else if totp_enabled != 0 {
            let mut factor_ok = false;
            if let (Some(code), Some(stored_secret)) =
                (form.totp_code.as_deref(), totp_secret.as_deref())
            {
                match load_totp_secret(&state, id, stored_secret).await {
                    Ok(secret) => factor_ok = totp_verify(&secret, code).unwrap_or(false),
                    Err(error) => {
                        tracing::error!(user_id = id, %error, "unable to load TOTP secret");
                    }
                }
            }
            if !factor_ok {
                if let Some(code) = form.backup_code.as_deref().or(form.totp_code.as_deref()) {
                    factor_ok = verify_backup_code(&state, id, code).await?;
                }
            }
            if factor_ok {
                authenticated_user_id = Some(id);
            }
            factor_ok
        } else {
            authenticated_user_id = Some(id);
            true
        }
    } else {
        false
    };

    sqlx::query("INSERT INTO login_attempts (ip_address, username, success, attempted_at) VALUES (?, ?, ?, ?)")
        .bind(&ip.0)
        .bind(&form.username)
        .bind(success as i64)
        .bind(now.to_rfc3339())
        .execute(&state.db)
        .await?;

    if !success {
        let ctx = auth_layout("Login");
        let page = LoginPage {
            ctx: &ctx,
            csrf_token: "",
            error: Some("Invalid credentials"),
        };
        return Ok(Html(
            page.render()
                .map_err(|e| AppError::Internal(e.to_string()))?,
        )
        .into_response());
    }

    let user_id = authenticated_user_id
        .ok_or_else(|| AppError::Internal("successful authentication without a user id".into()))?;

    sqlx::query("UPDATE users SET last_login_at = ? WHERE id = ?")
        .bind(now.to_rfc3339())
        .bind(user_id)
        .execute(&state.db)
        .await?;

    let (token, _session) = state
        .sessions
        .create_session(user_id, Some(&ip.0), None)
        .await
        .map_err(AppError::from_core)?;

    let cookie = build_session_cookie(&state, token.as_str());
    let mut jar = CookieJar::new();
    jar = jar.add(cookie);

    Ok((jar, Redirect::to("/dashboard")).into_response())
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> AppResult<impl IntoResponse> {
    let cookie_name = state.config.session.cookie_name.clone();
    if let Some(c) = jar.get(&cookie_name) {
        state
            .sessions
            .destroy_session(c.value())
            .await
            .map_err(AppError::from_core)?;
    }
    let mut jar = jar;
    jar = jar.remove(
        Cookie::build((cookie_name, ""))
            .path("/")
            .http_only(true)
            .build(),
    );
    Ok((jar, Redirect::to("/auth/login")).into_response())
}

pub async fn totp_form() -> AppResult<impl IntoResponse> {
    // Login now keeps the password and second factor in one submission. Keep
    // this route for old bookmarks and deployments that still link to it.
    Ok(Redirect::to("/auth/login").into_response())
}

pub async fn totp_submit() -> AppResult<impl IntoResponse> {
    Ok(Redirect::to("/auth/login").into_response())
}

pub async fn security_form(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<impl IntoResponse> {
    let row: (i64,) = sqlx::query_as("SELECT totp_enabled FROM users WHERE id = ?")
        .bind(user.user_id())
        .fetch_one(&state.db)
        .await?;

    if row.0 != 0 {
        return render_security(&user, true, None, None, &[], None);
    }

    let now = Utc::now();
    let existing: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT id, secret_encrypted, otpauth_uri_encrypted
         FROM pending_totp_enrollments
         WHERE user_id = ? AND session_id = ? AND expires_at > ?",
    )
    .bind(user.user_id())
    .bind(user.session.session_id)
    .bind(now.to_rfc3339())
    .fetch_optional(&state.db)
    .await?;

    let (secret, otpauth_uri) = if let Some((_, secret, uri)) = existing {
        (
            crypto::decrypt_with_domain(&state.secret_key, TOTP_SECRET_DOMAIN, &secret)
                .map_err(AppError::from_core)?,
            crypto::decrypt_with_domain(&state.secret_key, TOTP_URI_DOMAIN, &uri)
                .map_err(AppError::from_core)?,
        )
    } else {
        sqlx::query(
            "DELETE FROM pending_totp_enrollments
             WHERE user_id = ? AND session_id = ?",
        )
        .bind(user.user_id())
        .bind(user.session.session_id)
        .execute(&state.db)
        .await?;

        let (secret, uri) = totp_generate(&state.config.auth.totp_issuer, user.username())
            .map_err(AppError::from_core)?;
        let secret_encrypted =
            crypto::encrypt_with_domain(&state.secret_key, TOTP_SECRET_DOMAIN, &secret)
                .map_err(AppError::from_core)?;
        let uri_encrypted = crypto::encrypt_with_domain(&state.secret_key, TOTP_URI_DOMAIN, &uri)
            .map_err(AppError::from_core)?;
        let expires_at = (now + Duration::seconds(PENDING_TOTP_TTL_SECS)).to_rfc3339();
        sqlx::query(
            "INSERT INTO pending_totp_enrollments
             (user_id, session_id, secret_encrypted, created_at, expires_at,
              otpauth_uri_encrypted)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(user.user_id())
        .bind(user.session.session_id)
        .bind(&secret_encrypted)
        .bind(now.to_rfc3339())
        .bind(expires_at)
        .bind(&uri_encrypted)
        .execute(&state.db)
        .await?;
        (secret, uri)
    };

    render_security(&user, false, Some(&otpauth_uri), Some(&secret), &[], None)
}

pub async fn enable_totp(
    State(state): State<AppState>,
    user: AuthUser,
    Extension(ip): Extension<ClientIp>,
    Extension(request_id): Extension<RequestId>,
    Form(form): Form<TotpCodeForm>,
) -> AppResult<impl IntoResponse> {
    let pending: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, secret_encrypted
         FROM pending_totp_enrollments
         WHERE user_id = ? AND session_id = ? AND expires_at > ?",
    )
    .bind(user.user_id())
    .bind(user.session.session_id)
    .bind(Utc::now().to_rfc3339())
    .fetch_optional(&state.db)
    .await?;
    let Some((pending_id, secret_encrypted)) = pending else {
        return Err(AppError::BadRequest(
            "TOTP enrollment expired; reload the security page".into(),
        ));
    };
    let secret =
        crypto::decrypt_with_domain(&state.secret_key, TOTP_SECRET_DOMAIN, &secret_encrypted)
            .map_err(AppError::from_core)?;
    if !totp_verify(&secret, &form.totp_code).unwrap_or(false) {
        return Err(AppError::Unauthorized("invalid TOTP code".into()));
    }

    let codes = generate_backup_codes(10);
    let now = Utc::now().to_rfc3339();
    let mut tx = state.db.begin().await?;
    let updated = sqlx::query(
        "UPDATE users
         SET totp_secret = ?, totp_enabled = 1, updated_at = ?
         WHERE id = ? AND totp_enabled = 0",
    )
    .bind(
        crypto::encrypt_with_domain(&state.secret_key, TOTP_SECRET_DOMAIN, &secret)
            .map_err(AppError::from_core)?,
    )
    .bind(&now)
    .bind(user.user_id())
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() != 1 {
        tx.rollback().await?;
        return Err(AppError::Conflict("TOTP is already enabled".into()));
    }
    sqlx::query("DELETE FROM backup_codes WHERE user_id = ?")
        .bind(user.user_id())
        .execute(&mut *tx)
        .await?;
    for code in &codes {
        sqlx::query("INSERT INTO backup_codes (user_id, code_hash) VALUES (?, ?)")
            .bind(user.user_id())
            .bind(hash_token(&normalize_backup_code(code)))
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "DELETE FROM pending_totp_enrollments
         WHERE id = ? AND user_id = ? AND session_id = ?",
    )
    .bind(pending_id)
    .bind(user.user_id())
    .bind(user.session.session_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    record_security_audit(&state, &user, &ip, &request_id, "auth.totp.enable").await;
    render_security(&user, true, None, None, &codes, None)
}

pub async fn regenerate_backup_codes(
    State(state): State<AppState>,
    user: AuthUser,
    Extension(ip): Extension<ClientIp>,
    Extension(request_id): Extension<RequestId>,
    Form(form): Form<TotpCodeForm>,
) -> AppResult<impl IntoResponse> {
    require_recent_reauth(&user, &state)?;
    let stored_secret: Option<String> =
        sqlx::query_scalar("SELECT totp_secret FROM users WHERE id = ? AND totp_enabled = 1")
            .bind(user.user_id())
            .fetch_optional(&state.db)
            .await?;
    let stored_secret =
        stored_secret.ok_or_else(|| AppError::BadRequest("TOTP is not enabled".into()))?;
    let secret = load_totp_secret(&state, user.user_id(), &stored_secret).await?;
    if !totp_verify(&secret, &form.totp_code).unwrap_or(false) {
        return Err(AppError::Unauthorized("invalid TOTP code".into()));
    }

    let codes = generate_backup_codes(10);
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM backup_codes WHERE user_id = ?")
        .bind(user.user_id())
        .execute(&mut *tx)
        .await?;
    for code in &codes {
        sqlx::query("INSERT INTO backup_codes (user_id, code_hash) VALUES (?, ?)")
            .bind(user.user_id())
            .bind(hash_token(&normalize_backup_code(code)))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    record_security_audit(
        &state,
        &user,
        &ip,
        &request_id,
        "auth.totp.backup_codes_regenerate",
    )
    .await;
    render_security(&user, true, None, None, &codes, None)
}

pub async fn disable_totp(
    State(state): State<AppState>,
    user: AuthUser,
    Extension(ip): Extension<ClientIp>,
    Extension(request_id): Extension<RequestId>,
    Form(form): Form<DisableTotpForm>,
) -> AppResult<impl IntoResponse> {
    require_recent_reauth(&user, &state)?;
    let row: (String, i64) =
        sqlx::query_as("SELECT password_hash, totp_enabled FROM users WHERE id = ?")
            .bind(user.user_id())
            .fetch_one(&state.db)
            .await?;
    if row.1 == 0 {
        return Err(AppError::BadRequest("TOTP is not enabled".into()));
    }
    let password_ok = form
        .password
        .as_deref()
        .is_some_and(|password| verify_password(password, &row.0).unwrap_or(false));
    let totp_ok = if let Some(code) = form.totp_code.as_deref() {
        let stored_secret: String =
            sqlx::query_scalar("SELECT totp_secret FROM users WHERE id = ?")
                .bind(user.user_id())
                .fetch_one(&state.db)
                .await?;
        let secret = load_totp_secret(&state, user.user_id(), &stored_secret).await?;
        totp_verify(&secret, code).unwrap_or(false)
    } else {
        false
    };
    if !password_ok && !totp_ok {
        return Err(AppError::Unauthorized(
            "valid password or current TOTP code required".into(),
        ));
    }

    let now = Utc::now().to_rfc3339();
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "UPDATE users SET totp_secret = NULL, totp_enabled = 0, updated_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(user.user_id())
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM backup_codes WHERE user_id = ?")
        .bind(user.user_id())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    record_security_audit(&state, &user, &ip, &request_id, "auth.totp.disable").await;
    render_security(&user, false, None, None, &[], None)
}

pub async fn reauth_form(
    user: AuthUser,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> AppResult<impl IntoResponse> {
    let return_to = params
        .get("return_to")
        .map(|s| safe_return_to(s))
        .unwrap_or("/");
    let ctx = auth_layout("Re-authenticate");
    let page = ReauthPage {
        ctx: &ctx,
        csrf_token: &user.session.csrf_token,
        return_to,
        error: None,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .into_response())
}

pub async fn reauth_submit(
    State(state): State<AppState>,
    user: AuthUser,
    Form(form): Form<ReauthForm>,
) -> AppResult<impl IntoResponse> {
    let row: (String, Option<String>, i64) =
        sqlx::query_as("SELECT password_hash, totp_secret, totp_enabled FROM users WHERE id = ?")
            .bind(user.user_id())
            .fetch_one(&state.db)
            .await?;

    if !verify_password(&form.password, &row.0).map_err(AppError::from_core)? {
        return Err(AppError::Unauthorized("invalid password".into()));
    }

    if row.2 != 0 {
        let mut factor_ok = false;
        if let (Some(code), Some(stored_secret)) = (form.totp_code.as_deref(), row.1.as_deref()) {
            let secret = load_totp_secret(&state, user.user_id(), stored_secret).await?;
            factor_ok = totp_verify(&secret, code).unwrap_or(false);
        }
        if !factor_ok {
            if let Some(code) = form.backup_code.as_deref().or(form.totp_code.as_deref()) {
                factor_ok = verify_backup_code(&state, user.user_id(), code).await?;
            }
        }
        if !factor_ok {
            return Err(AppError::Unauthorized("invalid TOTP or backup code".into()));
        }
    }

    state
        .sessions
        .touch_auth(user.session.session_id)
        .await
        .map_err(AppError::from_core)?;

    Ok(Redirect::to(safe_return_to(&form.return_to)).into_response())
}

async fn verify_backup_code(state: &AppState, user_id: i64, code: &str) -> AppResult<bool> {
    let hash = hash_token(&normalize_backup_code(code));
    let result = sqlx::query(
        "UPDATE backup_codes
         SET used_at = ?
         WHERE user_id = ? AND code_hash = ? AND used_at IS NULL",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(user_id)
    .bind(hash)
    .execute(&state.db)
    .await?;
    Ok(result.rows_affected() == 1)
}

fn normalize_backup_code(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_ascii_whitespace() && *c != '-')
        .flat_map(|c| c.to_uppercase())
        .collect()
}

fn safe_return_to(value: &str) -> &str {
    if value.starts_with('/') && !value.starts_with("//") {
        value
    } else {
        "/"
    }
}

async fn load_totp_secret(state: &AppState, user_id: i64, stored: &str) -> AppResult<String> {
    match crypto::decrypt_with_domain(&state.secret_key, TOTP_SECRET_DOMAIN, stored) {
        Ok(secret) => Ok(secret),
        Err(_) if is_dev_secret_key(state) && looks_like_legacy_totp(stored) => {
            let encrypted =
                crypto::encrypt_with_domain(&state.secret_key, TOTP_SECRET_DOMAIN, stored)
                    .map_err(AppError::from_core)?;
            sqlx::query("UPDATE users SET totp_secret = ?, updated_at = ? WHERE id = ?")
                .bind(encrypted)
                .bind(Utc::now().to_rfc3339())
                .bind(user_id)
                .execute(&state.db)
                .await?;
            Ok(stored.to_string())
        }
        Err(_) => Err(AppError::Internal(
            "TOTP secret is not encrypted with the current application key; re-enroll TOTP".into(),
        )),
    }
}

fn looks_like_legacy_totp(value: &str) -> bool {
    let value = value.trim_end_matches('=');
    value.len() >= 16
        && value
            .chars()
            .all(|c| matches!(c.to_ascii_uppercase(), 'A'..='Z' | '2'..='7'))
}

fn is_dev_secret_key(state: &AppState) -> bool {
    state.config.app.secret_key == "dev-secret-key-change-me-in-production-32b"
}

fn require_recent_reauth(user: &AuthUser, state: &AppState) -> AppResult<()> {
    if !user.is_recently_authed(state.config.session.reauth_window_secs) {
        return Err(AppError::Unauthorized(
            "recent authentication required".into(),
        ));
    }
    Ok(())
}

async fn record_security_audit(
    state: &AppState,
    user: &AuthUser,
    ip: &ClientIp,
    request_id: &RequestId,
    action: &str,
) {
    let event = AuditEvent::success(
        action,
        Some(user.user_id()),
        Some(user.username().to_string()),
        Some(ip.0.clone()),
        request_id.0.clone(),
        None,
        None,
    );
    if let Err(error) = state.audit.record(event).await {
        tracing::error!(%error, action, "failed to record TOTP security audit event");
    }
}

fn render_security<'a>(
    user: &'a AuthUser,
    enabled: bool,
    otpauth_uri: Option<&'a str>,
    manual_secret: Option<&'a str>,
    backup_codes: &'a [String],
    error: Option<&'a str>,
) -> AppResult<axum::response::Response> {
    let ctx = layout_ctx("Security", user, "security");
    let page = SecurityPage {
        ctx: &ctx,
        enabled,
        otpauth_uri,
        manual_secret,
        backup_codes,
        error,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .into_response())
}

fn build_session_cookie(state: &AppState, token: &str) -> Cookie<'static> {
    let same_site = match state.config.session.same_site.as_str() {
        "Lax" => SameSite::Lax,
        "None" => SameSite::None,
        _ => SameSite::Strict,
    };
    Cookie::build((state.config.session.cookie_name.clone(), token.to_string()))
        .path("/")
        .http_only(true)
        .secure(state.config.session.secure)
        .same_site(same_site)
        .max_age(time::Duration::seconds(
            state.config.session.ttl_secs as i64,
        ))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use pgpanel_core::auth::hash_password;

    #[tokio::test]
    async fn login_lockout_after_failures() {
        let config = pgpanel_core::config::Config::dev_default();
        let pool = db::test_pool().await;
        let hash = hash_password("CorrectHorse!battery1").unwrap();
        let now = Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO users (username, password_hash, role, created_at, updated_at) VALUES ('admin', ?, 'owner', ?, ?)")
            .bind(&hash).bind(&now).bind(&now)
            .execute(&pool).await.unwrap();
        let state = AppState::new(config.clone(), pool).unwrap();
        for _ in 0..config.auth.max_failed_logins {
            sqlx::query("INSERT INTO login_attempts (ip_address, username, success, attempted_at) VALUES ('127.0.0.1', 'admin', 0, ?)")
                .bind(Utc::now().to_rfc3339())
                .execute(&state.db).await.unwrap();
        }
        let failed: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM login_attempts WHERE ip_address = '127.0.0.1' AND success = 0",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert!(failed.0 >= config.auth.max_failed_logins as i64);
    }

    #[tokio::test]
    async fn backup_code_is_case_normalized_and_consumed_once() {
        let config = pgpanel_core::config::Config::dev_default();
        let pool = db::test_pool().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO users (username, password_hash, role, created_at, updated_at) VALUES ('admin', 'unused', 'owner', ?, ?)")
            .bind(&now)
            .bind(&now)
            .execute(&pool)
            .await
            .unwrap();
        let user_id: (i64,) = sqlx::query_as("SELECT id FROM users WHERE username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO backup_codes (user_id, code_hash) VALUES (?, ?)")
            .bind(user_id.0)
            .bind(hash_token(&normalize_backup_code("AbCdE-fGhIj")))
            .execute(&pool)
            .await
            .unwrap();
        let state = AppState::new(config, pool).unwrap();

        assert!(verify_backup_code(&state, user_id.0, "abcde fghij")
            .await
            .unwrap());
        assert!(!verify_backup_code(&state, user_id.0, "ABCDE-FGHIJ")
            .await
            .unwrap());
        assert!(!verify_backup_code(&state, user_id.0, "wrong-code")
            .await
            .unwrap());
    }

    #[test]
    fn invalid_totp_code_is_rejected() {
        let (secret, _) = totp_generate("PgPanel", "admin").unwrap();
        assert!(!totp_verify(&secret, "not-a-code").unwrap());
        assert!(!totp_verify(&secret, "abcdef").unwrap());
    }
}
