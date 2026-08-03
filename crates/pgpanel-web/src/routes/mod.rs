//! Route handlers and router assembly.

mod audit;
mod auth;
mod backups;
mod clusters;
mod dashboard;
mod databases;
mod health;
mod logs;
mod monitoring;
mod roles;
mod settings;
mod setup;
mod sql;
mod static_files;
mod updates;
mod users;

use crate::middleware::{auth as auth_middleware, csrf, rate_limit, request_id};
use crate::state::AppState;
use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

/// Build the application router.
pub fn router(state: AppState) -> Router {
    let public = Router::new()
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .route("/health/version", get(health::version))
        .route("/setup", get(setup::setup_form).post(setup::setup_submit))
        .route(
            "/auth/login",
            get(auth::login_form).post(auth::login_submit),
        )
        .route("/auth/logout", post(auth::logout))
        .route("/auth/totp", get(auth::totp_form).post(auth::totp_submit))
        .merge(static_files::router());

    let protected = Router::new()
        .route("/", get(dashboard::index))
        .route("/dashboard", get(dashboard::index))
        .nest("/clusters", clusters::router())
        .nest("/databases", databases::router())
        .nest("/roles", roles::router())
        .nest("/sql", sql::router())
        .route("/monitoring", get(monitoring::index))
        .route("/logs", get(logs::index))
        .route("/backups", get(backups::index))
        .route("/audit", get(audit::index))
        .route("/audit/export", get(audit::export_csv))
        .route(
            "/auth/reauth",
            get(auth::reauth_form).post(auth::reauth_submit),
        )
        .nest("/users", users::router())
        .route("/settings", get(settings::index).post(settings::update))
        .route("/settings/security", get(auth::security_form))
        .route("/settings/security/totp/enable", post(auth::enable_totp))
        .route(
            "/settings/security/totp/regenerate",
            post(auth::regenerate_backup_codes),
        )
        .route("/settings/security/totp/disable", post(auth::disable_totp))
        .nest("/updates", updates::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            csrf::csrf_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::require_auth_redirect,
        ));

    Router::new()
        .merge(public)
        .merge(protected)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_id::request_id_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .with_state(state)
}
