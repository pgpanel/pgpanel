use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use pgpanel_core::models::{HealthResponse, ReadyResponse};

use crate::error::ApiResult;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

async fn ready(State(state): State<AppState>) -> ApiResult<Json<ReadyResponse>> {
    let database = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();

    let docker = state.provisioner.docker().ping().await.is_ok();

    Ok(Json(ReadyResponse {
        ready: database,
        database,
        docker,
    }))
}
