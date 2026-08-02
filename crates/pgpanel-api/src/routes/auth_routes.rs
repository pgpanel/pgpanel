use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use cookie::time::Duration as TimeDuration;
use secrecy::SecretString;
use std::net::SocketAddr;

use pgpanel_core::audit;
use pgpanel_core::models::{BootstrapRequest, LoginRequest, MeResponse};

use crate::auth::{bootstrap_needed, create_bootstrap_user, login, logout, write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::state::AppState;
use axum::extract::State;
use pgpanel_core::error::Error;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/bootstrap", post(bootstrap))
        .route("/api/auth/login", post(do_login))
        .route("/api/auth/logout", post(do_logout))
        .route("/api/me", get(me))
        .route("/api/auth/status", get(auth_status))
}

#[derive(serde::Serialize)]
struct AuthStatus {
    bootstrap_required: bool,
}

async fn auth_status(State(state): State<AppState>) -> ApiResult<Json<AuthStatus>> {
    Ok(Json(AuthStatus {
        bootstrap_required: bootstrap_needed(&state).await?,
    }))
}

async fn bootstrap(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<BootstrapRequest>,
) -> ApiResult<Json<MeResponse>> {
    // Optional bootstrap token from install.sh
    if let Some(expected) = &state.config.bootstrap_token {
        match &body.bootstrap_token {
            Some(t) if t == expected => {}
            _ => {
                return Err(AppError(Error::Forbidden(
                    "invalid or missing bootstrap token".into(),
                )));
            }
        }
    }

    let password = SecretString::from(body.password);
    let user = create_bootstrap_user(&state, &body.username, &password).await?;

    write_audit(
        &state,
        Some(&user),
        audit::AUTH_BOOTSTRAP,
        "user",
        Some(&user.id.to_string()),
        serde_json::json!({"username": user.username}),
        Some(&addr.ip().to_string()),
        None,
    )
    .await;

    // Auto-login after bootstrap
    let (token, csrf, user) = login(
        &state,
        &body.username,
        &password,
        Some(&addr.ip().to_string()),
        None,
    )
    .await?;

    // Note: cookie set via separate response builder would be cleaner;
    // for bootstrap we return CSRF and expect client to login, or set cookie here.
    let _ = token; // client should call login; still return me-like payload

    Ok(Json(MeResponse {
        user,
        csrf_token: csrf,
    }))
}

async fn do_login(
    State(state): State<AppState>,
    jar: CookieJar,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> ApiResult<(CookieJar, Json<MeResponse>)> {
    let password = SecretString::from(body.password);
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());

    let (token, csrf, user) = login(
        &state,
        &body.username,
        &password,
        Some(&addr.ip().to_string()),
        ua,
    )
    .await?;

    write_audit(
        &state,
        Some(&user),
        audit::AUTH_LOGIN,
        "user",
        Some(&user.id.to_string()),
        serde_json::json!({}),
        Some(&addr.ip().to_string()),
        None,
    )
    .await;

    let mut cookie = Cookie::new(state.config.cookie_name.clone(), token);
    cookie.set_http_only(true);
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);
    cookie.set_secure(state.config.cookie_secure);
    cookie.set_max_age(TimeDuration::hours(state.config.session_ttl_hours));

    Ok((
        jar.add(cookie),
        Json(MeResponse {
            user,
            csrf_token: csrf,
        }),
    ))
}

async fn do_logout(
    State(state): State<AppState>,
    jar: CookieJar,
    auth: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> ApiResult<(CookieJar, Json<serde_json::Value>)> {
    if let Some(c) = jar.get(&state.config.cookie_name) {
        logout(&state, c.value()).await?;
    }

    write_audit(
        &state,
        Some(&auth.user),
        audit::AUTH_LOGOUT,
        "user",
        Some(&auth.user.id.to_string()),
        serde_json::json!({}),
        Some(&addr.ip().to_string()),
        None,
    )
    .await;

    let mut cookie = Cookie::new(state.config.cookie_name.clone(), "");
    cookie.set_http_only(true);
    cookie.set_path("/");
    cookie.set_max_age(TimeDuration::seconds(0));

    Ok((jar.add(cookie), Json(serde_json::json!({"ok": true}))))
}

async fn me(auth: AuthUser) -> ApiResult<Json<MeResponse>> {
    Ok(Json(MeResponse {
        user: auth.user,
        csrf_token: auth.csrf_token,
    }))
}
