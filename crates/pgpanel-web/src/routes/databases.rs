//! Database management routes.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use crate::templates_data::{
    layout_ctx, DatabaseCreatePage, DatabaseDetailPage, DatabasesListPage,
};
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
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
        .route("/:version/:cluster/:db", get(detail))
        .route("/:version/:cluster/:db/delete", post(delete_db))
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
    user.require(Permission::DatabasesRead)?;
    let port = q.port.unwrap_or(5432);
    let databases = state
        .postgres
        .list_databases(&version, &cluster, port)
        .await
        .map_err(AppError::from_core)?;
    let ctx = layout_ctx("Databases", &user, "databases");
    let page = DatabasesListPage {
        ctx: &ctx,
        version,
        cluster,
        port,
        databases,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

async fn create_form(
    State(_state): State<AppState>,
    user: AuthUser,
    Path((version, cluster)): Path<(String, String)>,
    Query(q): Query<PortQuery>,
) -> AppResult<Html<String>> {
    user.require(Permission::DatabasesCreate)?;
    let ctx = layout_ctx("Create Database", &user, "databases");
    let page = DatabaseCreatePage {
        ctx: &ctx,
        version,
        cluster,
        port: q.port.unwrap_or(5432),
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
struct CreateDbForm {
    csrf_token: String,
    name: String,
    owner: String,
    port: u16,
}

async fn create_submit(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, cluster)): Path<(String, String)>,
    Form(form): Form<CreateDbForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::DatabasesCreate)?;
    validate_pg_identifier(&form.name).map_err(AppError::from_core)?;
    validate_pg_identifier(&form.owner).map_err(AppError::from_core)?;
    state
        .postgres
        .create_database(&version, &cluster, form.port, &form.name, &form.owner)
        .await
        .map_err(AppError::from_core)?;
    Ok(Redirect::to(&format!(
        "/databases/{version}/{cluster}?port={}",
        form.port
    ))
    .into_response())
}

async fn detail(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, cluster, db)): Path<(String, String, String)>,
    Query(q): Query<PortQuery>,
) -> AppResult<Html<String>> {
    user.require(Permission::DatabasesRead)?;
    let port = q.port.unwrap_or(5432);
    let databases = state
        .postgres
        .list_databases(&version, &cluster, port)
        .await
        .map_err(AppError::from_core)?;
    let database = databases
        .into_iter()
        .find(|d| d.name == db)
        .ok_or_else(|| AppError::NotFound("database not found".into()))?;
    let ctx = layout_ctx(&db, &user, "databases");
    let page = DatabaseDetailPage {
        ctx: &ctx,
        version,
        cluster,
        database,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
struct DeleteDbForm {
    csrf_token: String,
    port: u16,
}

async fn delete_db(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, cluster, db)): Path<(String, String, String)>,
    Form(form): Form<DeleteDbForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::DatabasesDelete)?;
    state
        .postgres
        .drop_database(&version, &cluster, form.port, &db)
        .await
        .map_err(AppError::from_core)?;
    Ok(Redirect::to(&format!(
        "/databases/{version}/{cluster}?port={}",
        form.port
    ))
    .into_response())
}
