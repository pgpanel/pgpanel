use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use uuid::Uuid;

use pgpanel_core::error::Error;
use pgpanel_core::models::Operation;

use crate::auth::AuthUser;
use crate::error::{ApiResult, AppError};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/operations", get(list_ops))
        .route("/api/operations/{id}", get(get_op))
        .route("/api/operations/{id}/events", get(op_events))
}

async fn list_ops(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<Operation>>> {
    let ops = state.queue.list(100).await.map_err(AppError)?;
    Ok(Json(ops))
}

async fn get_op(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Operation>> {
    let op = state
        .queue
        .get(id)
        .await
        .map_err(AppError)?
        .ok_or_else(|| AppError(Error::NotFound("operation".into())))?;
    Ok(Json(op))
}

async fn op_events(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut last_log_id: i64 = 0;
        for _ in 0..600 {
            tokio::time::sleep(Duration::from_secs(1)).await;

            if let Ok(Some(op)) = state.queue.get(id).await {
                let logs = state.queue.logs(id, last_log_id).await.unwrap_or_default();
                if let Some(log) = logs.last() {
                    last_log_id = log.id;
                }
                let data = serde_json::json!({
                    "operation": op,
                    "logs": logs,
                });
                yield Ok(Event::default().event("update").data(data.to_string()));

                if op.status.is_terminal() {
                    yield Ok(Event::default().event("done").data("{}"));
                    break;
                }
            } else {
                yield Ok(Event::default().event("error").data("{\"error\":\"not found\"}"));
                break;
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
