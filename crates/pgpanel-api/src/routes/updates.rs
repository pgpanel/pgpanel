use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use pgpanel_core::error::Error;
use crate::auth::{require_admin, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/updates/status", get(status))
        .route("/api/updates/check", post(check))
        .route("/api/updates/apply", post(apply))
}
async fn setting(s: &AppState, k: &str) -> Result<Option<String>, AppError> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key=?").bind(k).fetch_optional(&s.pool).await.map_err(|e|AppError(Error::Internal(e.to_string())))
}
async fn put(s: &AppState, k: &str, v: &str) -> Result<(), AppError> {
    sqlx::query("INSERT INTO settings(key,value,updated_at) VALUES(?,?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at").bind(k).bind(v).bind(Utc::now().to_rfc3339()).execute(&s.pool).await.map_err(|e|AppError(Error::Internal(e.to_string())))?; Ok(())
}
fn current() -> String { std::env::var("PGPANEL_VERSION").ok().filter(|x|!x.is_empty()).unwrap_or_else(||env!("CARGO_PKG_VERSION").into()) }
fn newer(a: &str, b: &str) -> bool {
    let p=|s:&str| s.trim_start_matches('v').split('.').map(|x|x.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (a,b)=(p(a),p(b)); (0..3).map(|i|(a.get(i).unwrap_or(&0),b.get(i).unwrap_or(&0))).find(|(x,y)|x!=y).map(|(x,y)|y>x).unwrap_or(false)
}
async fn fetch_latest() -> Result<(String,String), AppError> {
    let client=reqwest::Client::new(); let url="https://raw.githubusercontent.com/pgpanel/pgpanel/main/VERSION";
    let v=client.get(url).send().await.map_err(|e|AppError(Error::Internal(e.to_string())))?.text().await.map_err(|e|AppError(Error::Internal(e.to_string())))?.trim().to_string();
    if v.is_empty(){return Err(AppError(Error::Internal("empty VERSION response".into())))} Ok((v.clone(), format!("https://github.com/pgpanel/pgpanel/releases/tag/v{v}")))
}
async fn status(State(s): State<AppState>, _a: AuthUser) -> ApiResult<Json<serde_json::Value>> {
    let latest=setting(&s,"update.latest_version").await?.unwrap_or_default(); let url=setting(&s,"update.latest_url").await?.unwrap_or_default(); let channel=setting(&s,"update.channel").await?.unwrap_or_else(||"stable".into()); let checked=setting(&s,"update.last_checked_at").await?.unwrap_or_default(); let cv=current();
    let available = !latest.is_empty() && newer(&cv, &latest);
    Ok(Json(serde_json::json!({
        "current_version": cv,
        "latest_version": if latest.is_empty() { serde_json::Value::Null } else { latest.into() },
        "latest_url": url,
        "update_available": available,
        "channel": channel,
        "last_checked_at": if checked.is_empty() { serde_json::Value::Null } else { checked.into() },
        "can_apply": available
    })))
}
async fn check(State(s): State<AppState>, a: AuthUser) -> ApiResult<Json<serde_json::Value>> { require_admin(&a)?; let (v,u)=fetch_latest().await?; put(&s,"update.latest_version",&v).await?; put(&s,"update.latest_url",&u).await?; put(&s,"update.last_checked_at",&Utc::now().to_rfc3339()).await?; status(State(s),a).await }
async fn apply(State(s): State<AppState>, a: AuthUser) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    let st = status(State(s.clone()), a).await?.0;
    if !st["update_available"].as_bool().unwrap_or(false) {
        return Ok(Json(serde_json::json!({"status":"up_to_date"})));
    }
    let v = st["latest_version"].as_str().unwrap_or("latest").to_string();
    put(&s, "update.pending", &v).await?;
    let image = format!("ghcr.io/pgpanel/pgpanel:{v}");
    s.provisioner
        .docker()
        .pull_panel_image(&image)
        .await
        .map_err(AppError)?;
    // Best-effort: also pull :latest so compose recreate picks it up.
    let _ = s
        .provisioner
        .docker()
        .pull_panel_image("ghcr.io/pgpanel/pgpanel:latest")
        .await;
    let restarted = s
        .provisioner
        .docker()
        .restart_panel_container()
        .await
        .ok();
    Ok(Json(serde_json::json!({
        "status": if restarted.unwrap_or(false) { "restarting" } else { "pulled" },
        "message": if restarted.unwrap_or(false) {
            "Image pulled and panel restart requested. If the version does not change, run: pgpanel update"
        } else {
            "Image pulled. Finish with: pgpanel update (or restart the panel container on the new tag)"
        },
        "version": v,
        "image": image,
    })))
}
