//! Backup status page (Databasus health visibility).

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::services::databasus::{DatabasusClient, HealthCheckResult, HealthStatus};
use crate::state::AppState;
use crate::templates_data::{layout_ctx, BackupsPage};
use askama::Template;
use axum::{extract::State, response::Html};
use pgpanel_core::permissions::Permission;

pub async fn index(State(state): State<AppState>, user: AuthUser) -> AppResult<Html<String>> {
    user.require(Permission::ClustersRead)?;

    let enabled = state.config.databasus.enabled;
    let configured = enabled
        && state
            .config
            .databasus
            .base_url
            .as_deref()
            .map(str::trim)
            .is_some_and(|u| !u.is_empty());

    let health = if !enabled {
        HealthCheckResult {
            status: HealthStatus::Unavailable,
            message: Some("Databasus integration is disabled".into()),
            body: None,
        }
    } else if !configured {
        HealthCheckResult {
            status: HealthStatus::Unavailable,
            message: Some("Databasus is enabled but base_url is not configured".into()),
            body: None,
        }
    } else {
        match DatabasusClient::new(&state.config.databasus) {
            Ok(client) => client.check_health().await,
            Err(e) => HealthCheckResult {
                status: HealthStatus::Unavailable,
                message: Some(e.user_message()),
                body: None,
            },
        }
    };

    let ctx = layout_ctx("Backups", &user, "backups");
    let page = BackupsPage {
        ctx: &ctx,
        enabled,
        configured,
        health_status: health.status.as_str(),
        health_message: health.message,
        // Public/configured Databasus API does not expose these without a
        // future explicit adapter (requires database_id mapping).
        job_listing_supported: false,
        trigger_supported: false,
        restore_verification_supported: false,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}
