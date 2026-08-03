//! Monitoring routes.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use crate::templates_data::{layout_ctx, MonitoringPage};
use askama::Template;
use axum::{extract::State, response::Html};
use pgpanel_core::permissions::Permission;

pub async fn index(State(state): State<AppState>, user: AuthUser) -> AppResult<Html<String>> {
    user.require(Permission::ClustersRead)?;
    let samples = state
        .monitoring
        .dashboard_summary()
        .await
        .unwrap_or_default();
    let ctx = layout_ctx("Monitoring", &user, "monitoring");
    let page = MonitoringPage { ctx: &ctx, samples };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}
