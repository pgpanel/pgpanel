use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use pgpanel_core::error::Error;

use crate::auth::AuthUser;
use crate::error::{ApiResult, AppError};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings/wizard", get(get_wizard).post(set_wizard))
        .route("/api/settings", get(list_settings))
}

async fn get_wizard(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    let v: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'wizard_completed'")
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    let completed = matches!(v.as_deref(), Some("true") | Some("1") | Some("yes"));
    Ok(Json(serde_json::json!({ "wizard_completed": completed })))
}

#[derive(Deserialize)]
struct WizardBody {
    completed: bool,
}

async fn set_wizard(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(body): Json<WizardBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let now = chrono::Utc::now().to_rfc3339();
    let val = if body.completed { "true" } else { "false" };
    sqlx::query(
        r#"
        INSERT INTO settings (key, value, updated_at) VALUES ('wizard_completed', ?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
        "#,
    )
    .bind(val)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(serde_json::json!({ "wizard_completed": body.completed })))
}

async fn list_settings(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM settings ORDER BY key")
            .fetch_all(&state.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    let map: serde_json::Map<String, serde_json::Value> = rows
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    Ok(Json(serde_json::Value::Object(map)))
}
