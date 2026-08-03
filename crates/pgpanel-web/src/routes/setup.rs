//! First-run setup.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::user_count;
use crate::middleware::request_id::ClientIp;
use crate::state::AppState;
use crate::templates_data::{auth_layout, SetupPage};
use askama::Template;
use axum::{
    extract::{Extension, State},
    response::{Html, IntoResponse, Redirect},
    Form,
};
use pgpanel_core::auth::hash_password;
use pgpanel_core::permissions::Role;
use serde::Deserialize;

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
pub struct SetupForm {
    pub csrf_token: String,
    pub username: String,
    pub password: String,
    pub password_confirm: String,
}

pub async fn setup_form(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
    if user_count(&state).await? > 0 {
        return Ok(Redirect::to("/auth/login").into_response());
    }
    let ctx = auth_layout("Setup PgPanel");
    let page = SetupPage {
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

pub async fn setup_submit(
    State(state): State<AppState>,
    Extension(_ip): Extension<ClientIp>,
    Form(form): Form<SetupForm>,
) -> AppResult<impl IntoResponse> {
    if user_count(&state).await? > 0 {
        return Err(AppError::Conflict("setup already completed".into()));
    }
    if form.password != form.password_confirm {
        let ctx = auth_layout("Setup PgPanel");
        let page = SetupPage {
            ctx: &ctx,
            csrf_token: &form.csrf_token,
            error: Some("Passwords do not match"),
        };
        return Ok(Html(
            page.render()
                .map_err(|e| AppError::Internal(e.to_string()))?,
        )
        .into_response());
    }

    let hash = hash_password(&form.password).map_err(AppError::from_core)?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO users (username, password_hash, role, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&form.username)
    .bind(&hash)
    .bind(Role::Owner.as_str())
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    Ok(Redirect::to("/auth/login").into_response())
}
