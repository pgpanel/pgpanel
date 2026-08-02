use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
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
        .route("/api/clusters/{id}/backup/enable", post(enable_backup))
        .route("/api/clusters/{id}/backup/register", post(enable_backup)) // compat
        .route("/api/clusters/{id}/backup/trigger", post(trigger))
        .route("/api/clusters/{id}/backup/verify", post(verify))
        .route("/api/clusters/{id}/backup/history", get(history))
        .route("/api/clusters/{id}/metrics", get(metrics))
}

#[derive(Deserialize)]
struct TriggerBody {
    #[serde(default = "default_db")]
    database: String,
    #[serde(default)]
    schema_only: bool,
}

fn default_db() -> String {
    "postgres".into()
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
        return Ok(Json(BackupStatus {
            integration_status: DatabasusIntegrationStatus::parse(&row.status),
            last_successful_backup: row
                .last_successful_backup
                .as_ref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&chrono::Utc)),
            last_backup_status: row.last_backup_status,
            backup_lag_seconds: row.backup_lag_seconds,
            wal_status: row.wal_status.or(Some(format!(
                "engine=native storage={}",
                state.backup.storage_kind()
            ))),
            failed_backups: row.failed_backups as u32,
            message: row.message.or(Some(format!(
                "Native backup · storage={} · retention={}d",
                state.backup.storage_kind(),
                state.config.backup_retention_days
            ))),
            manual_setup_info: Some(serde_json::json!({
                "engine": "native",
                "storage": state.backup.storage_kind(),
                "retention_days": state.config.backup_retention_days,
                "encrypt": state.config.backup_encrypt,
            })),
        }));
    }

    Ok(Json(BackupStatus {
        integration_status: cluster.databasus_status,
        last_successful_backup: None,
        last_backup_status: None,
        backup_lag_seconds: None,
        wal_status: Some(format!("engine=native storage={}", state.backup.storage_kind())),
        failed_backups: 0,
        message: Some("Backup not enabled for this cluster yet".into()),
        manual_setup_info: Some(serde_json::json!({
            "engine": "native",
            "hint": "Click Enable backup or create cluster with backup on",
        })),
    }))
}

async fn enable_backup(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let op = state
        .queue
        .enqueue(
            JobType::EnableBackup,
            Some(id),
            serde_json::json!({}),
            Some(&format!("enable-backup-{id}")),
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"action": "enable_native"}),
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
    body: Option<Json<TriggerBody>>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let body = body.map(|j| j.0).unwrap_or(TriggerBody {
        database: "postgres".into(),
        schema_only: false,
    });

    let op = state
        .queue
        .enqueue(
            JobType::RunBackup,
            Some(id),
            serde_json::json!({
                "database": body.database,
                "schema_only": body.schema_only,
            }),
            None,
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_TRIGGER,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"action": "run_backup"}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"operation_id": op})))
}

async fn verify(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let op = state
        .queue
        .enqueue(
            JobType::VerifyBackup,
            Some(id),
            serde_json::json!({}),
            None,
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"action": "verify"}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"operation_id": op})))
}

async fn history(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let rows = sqlx::query_as::<_, HistRow>(
        r#"
        SELECT id, kind, status, database_name, storage_key, size_bytes, checksum_sha256,
               error, started_at, finished_at, created_at
        FROM backups WHERE cluster_id = ?
        ORDER BY created_at DESC LIMIT 50
        "#,
    )
    .bind(id.to_string())
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    Ok(Json(serde_json::json!({ "backups": rows })))
}

#[derive(Deserialize)]
struct MetricsQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    60
}

async fn metrics(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<MetricsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let cluster = load_cluster(&state, id).await?;

    // Live sample
    let live = if let Some(ref name) = Some(cluster.docker_container_name.clone()) {
        state
            .provisioner
            .docker()
            .container_stats(name)
            .await
            .ok()
    } else {
        None
    };

    if let Some(ref s) = live {
        let now = chrono::Utc::now().to_rfc3339();
        let _ = sqlx::query(
            "INSERT INTO cluster_metrics (cluster_id, cpu_percent, memory_usage_mb, memory_limit_mb, collected_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(s.cpu_percent)
        .bind(s.memory_usage_mb)
        .bind(s.memory_limit_mb)
        .bind(&now)
        .execute(&state.pool)
        .await;
    }

    let history = sqlx::query_as::<_, MetricRow>(
        r#"
        SELECT cpu_percent, memory_usage_mb, memory_limit_mb, network_rx_bytes, network_tx_bytes, collected_at
        FROM cluster_metrics WHERE cluster_id = ?
        ORDER BY collected_at DESC LIMIT ?
        "#,
    )
    .bind(id.to_string())
    .bind(q.limit)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    Ok(Json(serde_json::json!({
        "live": live.map(|s| serde_json::json!({
            "cpu_percent": s.cpu_percent,
            "memory_usage_mb": s.memory_usage_mb,
            "memory_limit_mb": s.memory_limit_mb,
        })),
        "history": history,
    })))
}

#[derive(sqlx::FromRow, serde::Serialize)]
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

#[derive(sqlx::FromRow, serde::Serialize)]
struct HistRow {
    id: String,
    kind: String,
    status: String,
    database_name: String,
    storage_key: Option<String>,
    size_bytes: Option<i64>,
    checksum_sha256: Option<String>,
    error: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
    created_at: String,
}

#[derive(sqlx::FromRow, serde::Serialize)]
struct MetricRow {
    cpu_percent: f64,
    memory_usage_mb: f64,
    memory_limit_mb: f64,
    network_rx_bytes: Option<i64>,
    network_tx_bytes: Option<i64>,
    collected_at: String,
}
