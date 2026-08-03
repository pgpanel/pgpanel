//! Health check endpoints.

use crate::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use pgpanel_core::version::{BUILD_TIME, GIT_COMMIT, VERSION};
use serde::Serialize;

#[derive(Serialize)]
pub struct LiveResponse {
    status: &'static str,
}

#[derive(Serialize)]
pub struct ReadyResponse {
    status: &'static str,
    sqlite: bool,
    helper: bool,
}

#[derive(Serialize)]
pub struct VersionResponse {
    version: &'static str,
    git_commit: &'static str,
    build_time: &'static str,
    slot: String,
}

pub async fn live() -> Json<LiveResponse> {
    Json(LiveResponse { status: "ok" })
}

pub async fn ready(State(state): State<AppState>) -> Response {
    let sqlite_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();
    let helper_ok = state.helper.is_ready().await;
    let is_ready = sqlite_ok && helper_ok;
    let status = if is_ready { "ok" } else { "degraded" };
    let status_code = if is_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status_code,
        Json(ReadyResponse {
            status,
            sqlite: sqlite_ok,
            helper: helper_ok,
        }),
    )
        .into_response()
}

pub async fn version(State(state): State<AppState>) -> Json<VersionResponse> {
    Json(VersionResponse {
        version: VERSION,
        git_commit: GIT_COMMIT,
        build_time: BUILD_TIME,
        slot: state.config.app.slot.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_live_returns_ok() {
        let app = Router::new().route("/health/live", get(live));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_ready_with_db() {
        let config = pgpanel_core::config::Config::dev_default();
        let pool = db::test_pool().await;
        let state = AppState::new(config, pool).unwrap();
        let app = Router::new()
            .route("/health/ready", get(ready))
            .with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
