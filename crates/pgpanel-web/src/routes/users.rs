//! Panel user management.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use crate::templates_data::{layout_ctx, UserCreatePage, UserRow, UsersListPage};
use askama::Template;
use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect},
    routing::get,
    Form, Router,
};
use pgpanel_core::auth::hash_password;
use pgpanel_core::permissions::{Permission, Role};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/create", get(create_form).post(create_submit))
}

async fn list(State(state): State<AppState>, user: AuthUser) -> AppResult<Html<String>> {
    user.require(Permission::UsersManage)?;
    let users: Vec<UserRow> = sqlx::query_as(
        "SELECT id, username, role, totp_enabled, created_at, last_login_at, disabled FROM users ORDER BY username",
    )
    .fetch_all(&state.db)
    .await?;
    let ctx = layout_ctx("Users", &user, "users");
    let page = UsersListPage { ctx: &ctx, users };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

async fn create_form(user: AuthUser) -> AppResult<Html<String>> {
    user.require(Permission::UsersManage)?;
    let ctx = layout_ctx("Create User", &user, "users");
    let page = UserCreatePage {
        ctx: &ctx,
        roles: vec!["administrator", "operator", "viewer"],
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
struct CreateUserForm {
    csrf_token: String,
    username: String,
    password: String,
    role: String,
}

async fn create_submit(
    State(state): State<AppState>,
    user: AuthUser,
    Form(form): Form<CreateUserForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::UsersManage)?;
    let role =
        Role::parse(&form.role).ok_or_else(|| AppError::BadRequest("invalid role".into()))?;
    if role == Role::Owner && user.role() != Role::Owner {
        return Err(AppError::Forbidden("only owners can create owners".into()));
    }
    let hash = hash_password(&form.password).map_err(AppError::from_core)?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO users (username, password_hash, role, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&form.username)
    .bind(&hash)
    .bind(role.as_str())
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db) = &e {
            if db.is_unique_violation() {
                return AppError::Conflict("username already exists".into());
            }
        }
        AppError::Db(e)
    })?;
    Ok(Redirect::to("/users").into_response())
}
