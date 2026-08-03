//! Update management routes (daemon client only).

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::middleware::request_id::{ClientIp, RequestId};
use crate::state::AppState;
use crate::templates_data::{layout_ctx, UpdatesPage, UpdatesStatusFragment};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Extension, Form, Router,
};
use pgpanel_core::audit::AuditEvent;
use pgpanel_core::permissions::Permission;
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/status-fragment", get(status_fragment))
        .route("/check", post(check))
        .route("/start", post(start))
        .route("/rollback", post(rollback))
}

#[derive(Debug, Default, Deserialize)]
struct UpdatesQuery {
    notice: Option<String>,
    op: Option<String>,
}

async fn index(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<UpdatesQuery>,
) -> AppResult<Html<String>> {
    user.require(Permission::UpdatesManage)?;
    let status = state.updates.status().await;
    let ctx = layout_ctx("Updates", &user, "updates");
    let notice = notice_message(q.notice.as_deref(), q.op.as_deref());
    let page = UpdatesPage {
        ctx: &ctx,
        status: &status,
        notice: notice.as_deref(),
        csrf_token: &user.session.csrf_token,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

fn notice_message(notice: Option<&str>, op: Option<&str>) -> Option<String> {
    match notice {
        Some("check_ok") => Some("Update check completed.".into()),
        Some("install_accepted") => Some(format!(
            "Install accepted{}. The web UI may restart while the update continues.",
            op.map(|id| format!(" (operation {id})"))
                .unwrap_or_default()
        )),
        Some("rollback_accepted") => Some(format!(
            "Rollback accepted{}. The web UI may restart while rollback continues.",
            op.map(|id| format!(" (operation {id})"))
                .unwrap_or_default()
        )),
        _ => None,
    }
}

async fn status_fragment(State(state): State<AppState>, user: AuthUser) -> AppResult<Html<String>> {
    user.require(Permission::UpdatesManage)?;
    let status = state.updates.status().await;
    let fragment = UpdatesStatusFragment {
        status: &status,
        csrf_token: &user.session.csrf_token,
    };
    Ok(Html(
        fragment
            .render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

pub async fn check(
    State(state): State<AppState>,
    user: AuthUser,
    Extension(ip): Extension<ClientIp>,
    Extension(req_id): Extension<RequestId>,
    Form(form): Form<CsrfOnlyForm>,
) -> AppResult<impl IntoResponse> {
    let _ = form;
    user.require(Permission::UpdatesManage)?;

    match state.updates.check().await {
        Ok(status) => {
            let latest = status
                .latest
                .as_ref()
                .map(|r| r.version.clone())
                .unwrap_or_else(|| "none".into());
            state
                .audit
                .record(AuditEvent::success(
                    "updates.check",
                    Some(user.user_id()),
                    Some(user.username().to_string()),
                    Some(ip.0.clone()),
                    req_id.0.clone(),
                    Some(latest),
                    None,
                ))
                .await
                .ok();
            Ok(Redirect::to("/updates?notice=check_ok").into_response())
        }
        Err(msg) => {
            state
                .audit
                .record(AuditEvent::failure(
                    "updates.check",
                    Some(user.user_id()),
                    Some(user.username().to_string()),
                    Some(ip.0.clone()),
                    req_id.0.clone(),
                    None,
                    msg.clone(),
                ))
                .await
                .ok();
            Err(AppError::BadRequest(msg))
        }
    }
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
pub(crate) struct CsrfOnlyForm {
    csrf_token: String,
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
pub(crate) struct UpdateForm {
    csrf_token: String,
    version: Option<String>,
    confirm: Option<String>,
    confirmation: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
pub(crate) struct RollbackForm {
    csrf_token: String,
    confirm: Option<String>,
    confirmation: Option<String>,
}

pub async fn start(
    State(state): State<AppState>,
    user: AuthUser,
    Extension(ip): Extension<ClientIp>,
    Extension(req_id): Extension<RequestId>,
    Form(form): Form<UpdateForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::UpdatesManage)?;
    require_reauth(&user, &state)?;
    require_confirm(
        form.confirm.as_deref(),
        form.confirmation.as_deref(),
        "INSTALL",
    )?;

    let version = form
        .version
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());

    match state.updates.install(version).await {
        Ok(operation_id) => {
            state
                .audit
                .record(AuditEvent::success(
                    "updates.install",
                    Some(user.user_id()),
                    Some(user.username().to_string()),
                    Some(ip.0.clone()),
                    req_id.0.clone(),
                    Some(version.unwrap_or("latest").to_string()),
                    Some(serde_json::json!({ "operation_id": operation_id })),
                ))
                .await
                .ok();
            Ok(Redirect::to(&format!(
                "/updates?notice=install_accepted&op={operation_id}"
            ))
            .into_response())
        }
        Err(msg) => {
            state
                .audit
                .record(AuditEvent::failure(
                    "updates.install",
                    Some(user.user_id()),
                    Some(user.username().to_string()),
                    Some(ip.0.clone()),
                    req_id.0.clone(),
                    Some(version.unwrap_or("latest").to_string()),
                    msg.clone(),
                ))
                .await
                .ok();
            Err(AppError::BadRequest(msg))
        }
    }
}

pub async fn rollback(
    State(state): State<AppState>,
    user: AuthUser,
    Extension(ip): Extension<ClientIp>,
    Extension(req_id): Extension<RequestId>,
    Form(form): Form<RollbackForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::UpdatesManage)?;
    require_reauth(&user, &state)?;
    require_confirm(
        form.confirm.as_deref(),
        form.confirmation.as_deref(),
        "ROLLBACK",
    )?;

    match state.updates.rollback().await {
        Ok(operation_id) => {
            state
                .audit
                .record(AuditEvent::success(
                    "updates.rollback",
                    Some(user.user_id()),
                    Some(user.username().to_string()),
                    Some(ip.0.clone()),
                    req_id.0.clone(),
                    Some("previous".into()),
                    Some(serde_json::json!({ "operation_id": operation_id })),
                ))
                .await
                .ok();
            Ok(Redirect::to(&format!(
                "/updates?notice=rollback_accepted&op={operation_id}"
            ))
            .into_response())
        }
        Err(msg) => {
            state
                .audit
                .record(AuditEvent::failure(
                    "updates.rollback",
                    Some(user.user_id()),
                    Some(user.username().to_string()),
                    Some(ip.0.clone()),
                    req_id.0.clone(),
                    Some("previous".into()),
                    msg.clone(),
                ))
                .await
                .ok();
            Err(AppError::BadRequest(msg))
        }
    }
}

fn require_reauth(user: &AuthUser, state: &AppState) -> AppResult<()> {
    if !user.is_recently_authed(state.config.session.reauth_window_secs) {
        return Err(AppError::Unauthorized(
            "recent authentication required".into(),
        ));
    }
    Ok(())
}

fn require_confirm(
    confirm: Option<&str>,
    confirmation: Option<&str>,
    expected: &str,
) -> AppResult<()> {
    let checked = confirm.map(|c| c == "1" || c.eq_ignore_ascii_case("on") || c == "true");
    if checked != Some(true) {
        return Err(AppError::BadRequest(
            "confirmation checkbox is required".into(),
        ));
    }
    let phrase = confirmation.map(str::trim).unwrap_or("");
    if phrase != expected {
        return Err(AppError::BadRequest(format!(
            "type {expected} to confirm this operation"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::session::SessionData;
    use chrono::Utc;
    use pgpanel_core::permissions::Role;

    fn auth_user(role: Role, recent: bool) -> AuthUser {
        let last_auth_at = if recent {
            Utc::now()
        } else {
            Utc::now() - chrono::Duration::hours(2)
        };
        AuthUser {
            session: SessionData {
                session_id: 1,
                user_id: 1,
                username: "admin".into(),
                role,
                csrf_token: "csrf".into(),
                last_auth_at,
                expires_at: Utc::now() + chrono::Duration::hours(1),
                ip_address: Some("127.0.0.1".into()),
            },
        }
    }

    #[test]
    fn viewer_lacks_updates_manage() {
        let user = auth_user(Role::Viewer, true);
        assert!(user.require(Permission::UpdatesManage).is_err());
    }

    #[test]
    fn administrator_has_updates_manage() {
        let user = auth_user(Role::Administrator, true);
        assert!(user.require(Permission::UpdatesManage).is_ok());
    }

    #[test]
    fn install_requires_recent_reauth() {
        let user = auth_user(Role::Administrator, false);
        let config = pgpanel_core::config::Config::dev_default();
        // Mirror require_reauth window check without full AppState.
        assert!(!user.is_recently_authed(config.session.reauth_window_secs));
        let fresh = auth_user(Role::Administrator, true);
        assert!(fresh.is_recently_authed(config.session.reauth_window_secs));
    }

    #[test]
    fn confirm_phrase_enforced() {
        assert!(require_confirm(Some("1"), Some("INSTALL"), "INSTALL").is_ok());
        assert!(require_confirm(None, Some("INSTALL"), "INSTALL").is_err());
        assert!(require_confirm(Some("1"), Some("install"), "INSTALL").is_err());
        assert!(require_confirm(Some("1"), Some("ROLLBACK"), "ROLLBACK").is_ok());
    }
}
