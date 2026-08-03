//! Role management routes.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use crate::templates_data::{layout_ctx, RoleCreatePage, RoleDetailPage, RolesListPage};
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::get,
    Form, Router,
};
use pgpanel_core::permissions::Permission;
use pgpanel_core::validation::validate_pg_identifier;
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/:version/:cluster", get(list))
        .route(
            "/:version/:cluster/create",
            get(create_form).post(create_submit),
        )
        .route("/:version/:cluster/:role", get(detail))
}

#[derive(Deserialize)]
struct PortQuery {
    port: Option<u16>,
}

async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, cluster)): Path<(String, String)>,
    Query(q): Query<PortQuery>,
) -> AppResult<Html<String>> {
    user.require(Permission::RolesRead)?;
    let port = q.port.unwrap_or(5432);
    let roles = state
        .postgres
        .list_roles(&version, &cluster, port)
        .await
        .map_err(AppError::from_core)?;
    let ctx = layout_ctx("Roles", &user, "roles");
    let page = RolesListPage {
        ctx: &ctx,
        version,
        cluster,
        port,
        roles,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

async fn create_form(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, cluster)): Path<(String, String)>,
    Query(q): Query<PortQuery>,
) -> AppResult<Html<String>> {
    user.require(Permission::RolesManage)?;
    let ctx = layout_ctx("Create Role", &user, "roles");
    let page = RoleCreatePage {
        ctx: &ctx,
        version,
        cluster,
        port: q.port.unwrap_or(5432),
        allow_superuser: state.config.app.allow_superuser_creation,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
struct CreateRoleForm {
    csrf_token: String,
    name: String,
    password: Option<String>,
    can_login: Option<String>,
    can_create_db: Option<String>,
    is_superuser: Option<String>,
    port: u16,
}

async fn create_submit(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, cluster)): Path<(String, String)>,
    Form(form): Form<CreateRoleForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::RolesManage)?;
    validate_pg_identifier(&form.name).map_err(AppError::from_core)?;
    state
        .postgres
        .create_role(
            &version,
            &cluster,
            form.port,
            &form.name,
            form.password.as_deref(),
            form.can_login.is_some(),
            form.can_create_db.is_some(),
            form.is_superuser.is_some(),
        )
        .await
        .map_err(AppError::from_core)?;
    Ok(Redirect::to(&format!("/roles/{version}/{cluster}?port={}", form.port)).into_response())
}

async fn detail(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, cluster, role_name)): Path<(String, String, String)>,
    Query(q): Query<PortQuery>,
) -> AppResult<Html<String>> {
    user.require(Permission::RolesRead)?;
    let port = q.port.unwrap_or(5432);
    let roles = state
        .postgres
        .list_roles(&version, &cluster, port)
        .await
        .map_err(AppError::from_core)?;
    let role = roles
        .into_iter()
        .find(|r| r.name == role_name)
        .ok_or_else(|| AppError::NotFound("role not found".into()))?;
    let ctx = layout_ctx(&role_name, &user, "roles");
    let page = RoleDetailPage {
        ctx: &ctx,
        version,
        cluster,
        role,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}
