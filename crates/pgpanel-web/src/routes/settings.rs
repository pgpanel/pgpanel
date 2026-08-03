//! Settings routes.

use crate::error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use crate::templates_data::{layout_ctx, SettingRow, SettingsPage};
use askama::Template;
use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect},
    Form,
};
use pgpanel_core::permissions::Permission;
use serde::Deserialize;

pub async fn index(State(state): State<AppState>, user: AuthUser) -> AppResult<Html<String>> {
    user.require(Permission::SettingsManage)?;
    let settings = build_settings_display(&state);
    let ctx = layout_ctx("Settings", &user, "settings");
    let page = SettingsPage {
        ctx: &ctx,
        settings,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
pub struct SettingsForm {
    csrf_token: String,
    key: String,
    value: String,
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Form(form): Form<SettingsForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::SettingsManage)?;
    if form.key.contains("secret") || form.key.contains("password") || form.key.contains("api_key")
    {
        return Err(AppError::Forbidden("cannot modify secrets via UI".into()));
    }
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(&form.key)
    .bind(&form.value)
    .bind(&now)
    .execute(&state.db)
    .await?;
    Ok(Redirect::to("/settings").into_response())
}

fn build_settings_display(state: &AppState) -> Vec<SettingRow> {
    vec![
        SettingRow {
            key: "server.listen".into(),
            value: state.config.server.listen.to_string(),
            is_secret: false,
        },
        SettingRow {
            key: "app.slot".into(),
            value: state.config.app.slot.clone(),
            is_secret: false,
        },
        SettingRow {
            key: "app.active_slot".into(),
            value: state.config.app.active_slot.clone(),
            is_secret: false,
        },
        SettingRow {
            key: "monitoring.retention_hours".into(),
            value: state.config.monitoring.retention_hours.to_string(),
            is_secret: false,
        },
        SettingRow {
            key: "updates.channel".into(),
            value: state.config.updates.channel.clone(),
            is_secret: false,
        },
        SettingRow {
            key: "app.secret_key".into(),
            value: "********".into(),
            is_secret: true,
        },
        SettingRow {
            key: "databasus.api_key".into(),
            value: if state.config.databasus.api_key.is_some() {
                "********".into()
            } else {
                "(not set)".into()
            },
            is_secret: true,
        },
    ]
}
