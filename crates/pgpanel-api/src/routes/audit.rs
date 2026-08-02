use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use pgpanel_core::error::Error;
use pgpanel_core::models::AuditLog;

use crate::auth::AuthUser;
use crate::error::{ApiResult, AppError};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/audit-logs", get(list_audit))
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
}

async fn list_audit(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Json<Vec<AuditLog>>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query_as::<_, AuditRow>(
        r#"
        SELECT id, actor_id, actor_username, action, resource_type, resource_id,
               details, ip_address, correlation_id, created_at
        FROM audit_logs ORDER BY id DESC LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    Ok(Json(
        rows.into_iter().filter_map(|r| r.into_log().ok()).collect(),
    ))
}

#[derive(sqlx::FromRow)]
struct AuditRow {
    id: i64,
    actor_id: Option<String>,
    actor_username: Option<String>,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    details: String,
    ip_address: Option<String>,
    correlation_id: Option<String>,
    created_at: String,
}

impl AuditRow {
    fn into_log(self) -> Result<AuditLog, Error> {
        Ok(AuditLog {
            id: self.id,
            actor_id: self.actor_id.and_then(|s| Uuid::parse_str(&s).ok()),
            actor_username: self.actor_username,
            action: self.action,
            resource_type: self.resource_type,
            resource_id: self.resource_id,
            details: serde_json::from_str(&self.details).unwrap_or_default(),
            ip_address: self.ip_address,
            correlation_id: self.correlation_id,
            created_at: chrono::DateTime::parse_from_rfc3339(&self.created_at)
                .map(|d| d.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
        })
    }
}
