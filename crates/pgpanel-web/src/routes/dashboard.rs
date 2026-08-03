//! Dashboard route.

use crate::error::AppResult;
use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use crate::templates_data::{layout_ctx, DashboardPage};
use askama::Template;
use axum::{extract::State, response::Html};
use pgpanel_core::permissions::Permission;
use pgpanel_protocol::{HelperOp, HelperResult};

pub async fn index(State(state): State<AppState>, user: AuthUser) -> AppResult<Html<String>> {
    user.require(Permission::ClustersRead)?;

    let mut cluster_count = 0usize;
    let mut online_count = 0usize;

    if let Ok(resp) = state
        .helper
        .call(
            HelperOp::ClusterList,
            Some(user.user_id()),
            Some(user.username().to_string()),
            None,
            None,
        )
        .await
    {
        if let HelperResult::Ok { data } = resp.result {
            if let pgpanel_protocol::HelperOk::ClusterList { clusters } = *data {
                cluster_count = clusters.len();
                online_count = clusters
                    .iter()
                    .filter(|c| c.status.to_lowercase().contains("online"))
                    .count();
            }
        }
    }

    let samples = state
        .monitoring
        .dashboard_summary()
        .await
        .unwrap_or_default();
    let recent_audit = state.audit.list(None, 10, 0).await.unwrap_or_default();

    let ctx = layout_ctx("Dashboard", &user, "dashboard");
    let page = DashboardPage {
        ctx: &ctx,
        cluster_count,
        online_count,
        samples,
        recent_audit,
    };
    Ok(Html(page.render().map_err(|e| {
        crate::error::AppError::Internal(e.to_string())
    })?))
}
