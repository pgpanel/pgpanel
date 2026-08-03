//! Logs overview route.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use crate::templates_data::{layout_ctx, LogsPage};
use askama::Template;
use axum::{extract::State, response::Html};
use pgpanel_core::permissions::Permission;
use pgpanel_protocol::{HelperOk, HelperOp, HelperResult};

pub async fn index(State(state): State<AppState>, user: AuthUser) -> AppResult<Html<String>> {
    user.require(Permission::LogsRead)?;
    let clusters = match state
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
        Ok(resp) => match resp.result {
            HelperResult::Ok { data } => match *data {
                HelperOk::ClusterList { clusters } => clusters,
                _ => vec![],
            },
            _ => vec![],
        },
        Err(_) => vec![],
    };
    let ctx = layout_ctx("Logs", &user, "logs");
    let page = LogsPage {
        ctx: &ctx,
        clusters,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}
