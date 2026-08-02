use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;

use pgpanel_core::audit;
use pgpanel_core::error::Error;
use pgpanel_core::models::*;

use crate::auth::{write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::routes::clusters::load_cluster;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/clusters/{id}/backup", get(backup_status))
        .route("/api/clusters/{id}/backup/register", post(register))
        .route("/api/clusters/{id}/backup/trigger", post(trigger))
}

async fn backup_status(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<BackupStatus>> {
    let cluster = load_cluster(&state, id).await?;

    let row = sqlx::query_as::<_, BackupRow>(
        r#"
        SELECT status, external_id, last_successful_backup, last_backup_status,
               backup_lag_seconds, wal_status, failed_backups, message, manual_setup_info
        FROM backup_integrations WHERE cluster_id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    if let Some(row) = row {
        if let Some(ext) = &row.external_id {
            if let Ok(live) = state.databasus.get_backup_status(ext).await {
                return Ok(Json(live));
            }
        }
        return Ok(Json(BackupStatus {
            integration_status: DatabasusIntegrationStatus::parse(&row.status),
            last_successful_backup: row
                .last_successful_backup
                .as_ref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&chrono::Utc)),
            last_backup_status: row.last_backup_status,
            backup_lag_seconds: row.backup_lag_seconds,
            wal_status: row.wal_status,
            failed_backups: row.failed_backups as u32,
            message: row.message,
            manual_setup_info: row
                .manual_setup_info
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
        }));
    }

    Ok(Json(BackupStatus {
        integration_status: cluster.databasus_status,
        last_successful_backup: None,
        last_backup_status: None,
        backup_lag_seconds: None,
        wal_status: None,
        failed_backups: 0,
        message: Some("not registered".into()),
        manual_setup_info: None,
    }))
}

async fn register(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let op = state
        .queue
        .enqueue(
            JobType::RegisterDatabasus,
            Some(id),
            serde_json::json!({}),
            Some(&format!("register-databasus-{id}")),
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"action": "register"}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"operation_id": op})))
}

async fn trigger(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let ext: Option<String> =
        sqlx::query_scalar("SELECT external_id FROM backup_integrations WHERE cluster_id = ?")
            .bind(id.to_string())
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?
            .flatten();

    let Some(ext) = ext else {
        return Err(AppError(Error::Databasus(
            "cluster not registered with Databasus".into(),
        )));
    };

    let info = state
        .databasus
        .trigger_backup(&ext)
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_TRIGGER,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"external_id": ext}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"backup": info})))
}

#[derive(sqlx::FromRow)]
struct BackupRow {
    status: String,
    external_id: Option<String>,
    last_successful_backup: Option<String>,
    last_backup_status: Option<String>,
    backup_lag_seconds: Option<i64>,
    wal_status: Option<String>,
    failed_backups: i64,
    message: Option<String>,
    manual_setup_info: Option<String>,
}
