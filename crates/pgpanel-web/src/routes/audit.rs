//! Audit log routes.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use crate::templates_data::{layout_ctx, AuditPage, Pagination};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use pgpanel_core::permissions::Permission;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AuditQuery {
    pub page: Option<i64>,
    pub action: Option<String>,
}

pub async fn index(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<AuditQuery>,
) -> AppResult<Html<String>> {
    user.require(Permission::AuditRead)?;
    let page = q.page.unwrap_or(1).max(1);
    let per_page = 50i64;
    let offset = (page - 1) * per_page;
    let action_filter = q.action.unwrap_or_default();
    let filter = if action_filter.is_empty() {
        None
    } else {
        Some(action_filter.as_str())
    };
    let total = state
        .audit
        .count(filter)
        .await
        .map_err(AppError::from_core)?;
    let entries = state
        .audit
        .list(filter, per_page, offset)
        .await
        .map_err(AppError::from_core)?;
    let ctx = layout_ctx("Audit Log", &user, "audit");
    let page_view = AuditPage {
        ctx: &ctx,
        entries,
        pagination: Pagination::new(page, per_page, total),
        action_filter,
    };
    Ok(Html(
        page_view
            .render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

pub async fn export_csv(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::AuditRead)?;
    let csv = state
        .audit
        .export_csv(10_000)
        .await
        .map_err(AppError::from_core)?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/csv"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"audit.csv\"",
            ),
        ],
        csv,
    ))
}
