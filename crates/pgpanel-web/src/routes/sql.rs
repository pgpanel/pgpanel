//! SQL editor routes.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use crate::templates_data::{layout_ctx, SqlEditorPage};
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::Html,
    routing::get,
    Form, Router,
};
use pgpanel_core::permissions::{queries_read_only, Permission};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new().route("/:version/:cluster", get(editor).post(execute))
}

#[derive(Deserialize)]
struct SqlQuery {
    port: Option<u16>,
    database: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
struct SqlForm {
    csrf_token: String,
    query: String,
    port: u16,
    database: String,
}

async fn editor(
    State(_state): State<AppState>,
    user: AuthUser,
    Path((version, cluster)): Path<(String, String)>,
    Query(q): Query<SqlQuery>,
) -> AppResult<Html<String>> {
    user.require(Permission::QueriesExecute)?;
    let ctx = layout_ctx("SQL Editor", &user, "sql");
    let page = SqlEditorPage {
        ctx: &ctx,
        version,
        cluster,
        port: q.port.unwrap_or(5432),
        database: q.database.unwrap_or_else(|| "postgres".into()),
        result: None,
        query: String::new(),
        read_only: queries_read_only(user.role()),
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

async fn execute(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, cluster)): Path<(String, String)>,
    Form(form): Form<SqlForm>,
) -> AppResult<Html<String>> {
    user.require(Permission::QueriesExecute)?;
    let read_only = queries_read_only(user.role());
    let result = state
        .postgres
        .execute_query(
            &version,
            &cluster,
            form.port,
            &form.database,
            &form.query,
            state.config.postgres.default_max_rows,
            state.config.postgres.default_statement_timeout_ms,
            read_only,
        )
        .await
        .ok();

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO query_history (user_id, version, cluster_name, database, query_text, row_count, duration_ms, success, executed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user.user_id())
    .bind(&version)
    .bind(&cluster)
    .bind(&form.database)
    .bind(&form.query)
    .bind(result.as_ref().map(|r| r.row_count as i64))
    .bind(result.as_ref().map(|r| r.duration_ms as i64))
    .bind(result.is_some() as i64)
    .bind(&now)
    .execute(&state.db)
    .await?;

    let ctx = layout_ctx("SQL Editor", &user, "sql");
    let page = SqlEditorPage {
        ctx: &ctx,
        version,
        cluster,
        port: form.port,
        database: form.database,
        result,
        query: form.query,
        read_only,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}
