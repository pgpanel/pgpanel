use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use pgpanel_core::error::Error;
use tracing::{error, info, warn};

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
    sqlx::query_scalar("SELECT value FROM settings WHERE key=?")
        .bind(k)
        .fetch_optional(&s.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))
}

async fn put(s: &AppState, k: &str, v: &str) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO settings(key,value,updated_at) VALUES(?,?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
    )
    .bind(k)
    .bind(v)
    .bind(Utc::now().to_rfc3339())
    .execute(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(())
}

/// Version baked into the running image — never trust `.env` PGPANEL_VERSION alone
/// (host update can bump .env before the container image actually changes).
fn current_image_version() -> String {
    // 1) File shipped inside the image (authoritative)
    for path in ["/app/VERSION", "./VERSION"] {
        if let Ok(v) = std::fs::read_to_string(path) {
            let v = v.trim().trim_start_matches('v').to_string();
            if !v.is_empty() {
                return v;
            }
        }
    }
    // 2) Env only as fallback (may be stale/wrong from host .env)
    if let Ok(v) = std::env::var("PGPANEL_VERSION") {
        let v = v.trim().trim_start_matches('v').to_string();
        if !v.is_empty() {
            return v;
        }
    }
    env!("CARGO_PKG_VERSION").into()
}

fn normalize_ver(s: &str) -> String {
    s.trim().trim_start_matches('v').trim().to_string()
}

fn newer(a: &str, b: &str) -> bool {
    let p = |s: &str| {
        normalize_ver(s)
            .split('.')
            .map(|x| x.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let (a, b) = (p(a), p(b));
    (0..3)
        .map(|i| (a.get(i).unwrap_or(&0), b.get(i).unwrap_or(&0)))
        .find(|(x, y)| x != y)
        .map(|(x, y)| y > x)
        .unwrap_or(false)
}

fn versions_equal(a: &str, b: &str) -> bool {
    normalize_ver(a) == normalize_ver(b)
}

async fn fetch_latest() -> Result<(String, String), AppError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("pgpanel-updater/1.0")
        .build()
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    // 1) VERSION on main (primary)
    let url = "https://raw.githubusercontent.com/pgpanel/pgpanel/main/VERSION";
    match client.get(url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(text) = resp.text().await {
                let v = normalize_ver(&text);
                if !v.is_empty() && v != "unknown" {
                    return Ok((
                        v.clone(),
                        format!("https://github.com/pgpanel/pgpanel/releases/tag/v{v}"),
                    ));
                }
            }
        }
        Ok(resp) => {
            warn!(status = %resp.status(), "VERSION fetch non-success");
        }
        Err(e) => {
            warn!(error = %e, "VERSION fetch failed — trying GitHub releases API");
        }
    }

    // 2) GitHub latest release tag
    let rel = client
        .get("https://api.github.com/repos/pgpanel/pgpanel/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError(Error::Internal(format!("releases API failed: {e}"))))?;
    let body: serde_json::Value = rel
        .json()
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    let tag = body
        .get("tag_name")
        .and_then(|t| t.as_str())
        .map(normalize_ver)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError(Error::Internal("no tag_name in releases/latest".into())))?;
    Ok((
        tag.clone(),
        format!("https://github.com/pgpanel/pgpanel/releases/tag/v{tag}"),
    ))
}

async fn refresh_latest_cache(s: &AppState) -> Result<String, AppError> {
    let (v, u) = fetch_latest().await?;
    put(s, "update.latest_version", &v).await?;
    put(s, "update.latest_url", &u).await?;
    put(s, "update.last_checked_at", &Utc::now().to_rfc3339()).await?;
    info!(latest = %v, "update latest cache refreshed");
    Ok(v)
}

fn cache_is_stale(checked: &str) -> bool {
    if checked.is_empty() {
        return true;
    }
    let Ok(ts) = DateTime::parse_from_rfc3339(checked) else {
        return true;
    };
    let age = Utc::now().signed_duration_since(ts.with_timezone(&Utc));
    age > Duration::minutes(5)
}

/// Best-effort: image tag of the running compose `panel` container.
async fn running_panel_image_tag(s: &AppState) -> Option<String> {
    match s.provisioner.docker().running_panel_image().await {
        Ok(Some(img)) => {
            let tag = img.rsplit_once(':').map(|(_, t)| t).unwrap_or(img.as_str());
            let tag = normalize_ver(tag);
            if tag.is_empty() || tag == "latest" {
                None
            } else {
                Some(tag)
            }
        }
        Ok(None) => None,
        Err(e) => {
            warn!(error = %e, "could not inspect running panel image");
            None
        }
    }
}

async fn status_json(s: &AppState, force_refresh: bool) -> Result<serde_json::Value, AppError> {
    let mut latest = setting(s, "update.latest_version")
        .await?
        .unwrap_or_default();
    let mut url = setting(s, "update.latest_url").await?.unwrap_or_default();
    let channel = setting(s, "update.channel")
        .await?
        .unwrap_or_else(|| "stable".into());
    let mut checked = setting(s, "update.last_checked_at")
        .await?
        .unwrap_or_default();

    let cv = current_image_version();
    let running_tag = running_panel_image_tag(s).await;

    if force_refresh || latest.is_empty() || cache_is_stale(&checked) {
        match refresh_latest_cache(s).await {
            Ok(v) => {
                latest = v;
                url = setting(s, "update.latest_url")
                    .await?
                    .unwrap_or(url);
                checked = setting(s, "update.last_checked_at")
                    .await?
                    .unwrap_or(checked);
            }
            Err(e) => {
                warn!(error = %e.0, "refresh latest failed — using cache if any");
            }
        }
    }

    // Effective installed version: prefer running container image tag when it
    // disagrees with /app/VERSION or env (detects .env ahead of image).
    let effective = running_tag
        .clone()
        .filter(|t| !versions_equal(t, &cv))
        .map(|t| {
            warn!(
                file_version = %cv,
                running_image_tag = %t,
                "panel version mismatch — using running image tag for update detection"
            );
            t
        })
        .unwrap_or_else(|| cv.clone());

    let available = !latest.is_empty() && newer(&effective, &latest);
    Ok(serde_json::json!({
        "current_version": effective,
        "image_version": cv,
        "running_image_tag": running_tag,
        "latest_version": if latest.is_empty() { serde_json::Value::Null } else { latest.into() },
        "latest_url": url,
        "update_available": available,
        "channel": channel,
        "last_checked_at": if checked.is_empty() { serde_json::Value::Null } else { checked.into() },
        "can_apply": available
    }))
}

async fn status(State(s): State<AppState>, _a: AuthUser) -> ApiResult<Json<serde_json::Value>> {
    // Soft refresh when cache is stale so the UI does not stick on an old latest.
    Ok(Json(status_json(&s, false).await?))
}

async fn check(State(s): State<AppState>, a: AuthUser) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    let st = status_json(&s, true).await?;

    // Pre-pull the target image in the background so Apply is mostly cutover.
    if st["update_available"].as_bool().unwrap_or(false) {
        if let Some(v) = st["latest_version"].as_str().map(str::to_string) {
            let image = format!("ghcr.io/pgpanel/pgpanel:{v}");
            let docker = s.provisioner.docker().clone();
            tokio::spawn(async move {
                if let Err(e) = docker.pull_panel_image(&image).await {
                    error!(error = %e, %image, "background pre-pull failed");
                } else {
                    info!(%image, "background pre-pull completed");
                }
            });
        }
    }

    Ok(Json(st))
}

async fn apply(State(s): State<AppState>, a: AuthUser) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    // Always re-fetch latest before applying — never trust a stale cache.
    let st = status_json(&s, true).await?;
    if !st["update_available"].as_bool().unwrap_or(false) {
        return Ok(Json(serde_json::json!({
            "status": "up_to_date",
            "message": "Already on the latest version",
            "current_version": st.get("current_version"),
            "latest_version": st.get("latest_version"),
            "running_image_tag": st.get("running_image_tag"),
        })));
    }
    let v = st["latest_version"]
        .as_str()
        .unwrap_or("latest")
        .to_string();
    put(&s, "update.pending", &v).await?;
    let image = format!("ghcr.io/pgpanel/pgpanel:{v}");
    let image_for_task = image.clone();
    let version_for_task = v.clone();

    let docker = s.provisioner.docker().clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if let Err(e) = docker
            .schedule_detached_panel_upgrade(&image_for_task, &version_for_task)
            .await
        {
            error!(
                error = %e,
                image = %image_for_task,
                "panel detached upgrade failed — run: sudo pgpanel update"
            );
        }
    });

    Ok(Json(serde_json::json!({
        "status": "started",
        "message": "Near-zero update started: image pre-pulled if possible, then a short cutover while Caddy stays up. Keep this tab open.",
        "version": v,
        "image": image,
        "poll_health": true,
    })))
}
