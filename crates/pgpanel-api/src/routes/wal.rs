use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::DateTime;
use pgpanel_core::audit;
use pgpanel_core::error::Error;
use pgpanel_core::models::JobType;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{require_write, write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::routes::clusters::load_cluster;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/clusters/{id}/wal", get(get_wal))
        .route("/api/clusters/{id}/wal/enable", post(enable_wal))
        .route("/api/clusters/{id}/wal/disable", post(disable_wal))
        .route("/api/clusters/{id}/wal/sync", post(sync_wal))
        .route("/api/clusters/{id}/wal/switch", post(switch_wal))
        .route("/api/clusters/{id}/wal/segments", get(list_segments))
        .route(
            "/api/clusters/{id}/wal/segments/{filename}/download",
            get(download_segment),
        )
        .route(
            "/api/clusters/{id}/wal/segments/{filename}",
            get(get_segment).delete(delete_segment),
        )
        .route("/api/clusters/{id}/wal/base-backup", post(base_backup))
        .route("/api/clusters/{id}/wal/pitr", post(pitr_restore))
}

#[derive(Debug, Deserialize)]
struct EnableWalRequest {
    retention_days: Option<i64>,
    compress: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SegmentQuery {
    q: Option<String>,
    from: Option<String>,
    to: Option<String>,
    timeline: Option<i64>,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

fn default_limit() -> i64 {
    100
}

#[derive(Debug, Deserialize)]
struct PitrRequest {
    target_time: String,
    confirm_cluster_name: String,
}

#[derive(Debug, sqlx::FromRow, Serialize)]
struct WalRow {
    cluster_id: String,
    enabled: bool,
    archive_dir: String,
    compress: bool,
    retention_days: i64,
    status: String,
    last_segment: Option<String>,
    last_synced_at: Option<String>,
    last_error: Option<String>,
    timeline: Option<i64>,
    segment_count: i64,
    total_bytes: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, sqlx::FromRow, Serialize)]
struct SegmentRow {
    id: String,
    cluster_id: String,
    filename: String,
    timeline: i64,
    size_bytes: i64,
    archived_at: Option<String>,
    synced_at: String,
    storage_key: String,
    checksum_sha256: Option<String>,
}

async fn get_wal(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let stream = sqlx::query_as::<_, WalRow>(
        r#"
        SELECT cluster_id, enabled, archive_dir, compress, retention_days, status,
               last_segment, last_synced_at, last_error, timeline, segment_count,
               total_bytes, created_at, updated_at
        FROM wal_streams WHERE cluster_id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    let enabled = stream.as_ref().map(|s| s.enabled).unwrap_or(false);
    let stream_json = stream.map(|s| {
        serde_json::json!({
            "cluster_id": s.cluster_id,
            "enabled": s.enabled,
            "archive_dir": s.archive_dir,
            "compress": s.compress,
            "retention_days": s.retention_days,
            "status": s.status,
            "last_segment": s.last_segment,
            "last_synced_at": s.last_synced_at,
            "last_error": s.last_error,
            "timeline": s.timeline,
            "segment_count": s.segment_count,
            "total_bytes": s.total_bytes,
            "created_at": s.created_at,
            "updated_at": s.updated_at,
        })
    });
    Ok(Json(serde_json::json!({
        "stream": stream_json,
        "enabled": enabled,
    })))
}

async fn enable_wal(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    body: Option<Json<EnableWalRequest>>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    let _ = load_cluster(&state, id).await?;
    let body = body.map(|v| v.0).unwrap_or(EnableWalRequest {
        retention_days: None,
        compress: None,
    });
    let retention = body.retention_days.unwrap_or(14);
    if !(1..=3650).contains(&retention) {
        return Err(AppError(Error::Validation(
            "retention_days must be between 1 and 3650".into(),
        )));
    }
    let compress = body.compress.unwrap_or(true);
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO wal_streams (
            cluster_id, enabled, archive_dir, compress, retention_days, status, created_at, updated_at
        ) VALUES (?, 1, '/var/lib/postgresql/wal_archive', ?, ?, 'configuring', ?, ?)
        ON CONFLICT(cluster_id) DO UPDATE SET
            enabled = 1, compress = excluded.compress, retention_days = excluded.retention_days,
            status = 'configuring', last_error = NULL, updated_at = excluded.updated_at
        "#,
    )
    .bind(id.to_string())
    .bind(if compress { 1 } else { 0 })
    .bind(retention)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    update_integration_wal_status(&state, id, "configuring").await?;
    let operation_id = state
        .queue
        .enqueue(JobType::SyncWal, Some(id), serde_json::json!({}), None)
        .await
        .map_err(AppError)?;
    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"action": "enable_wal", "retention_days": retention, "compress": compress}),
        None,
        None,
    )
    .await;
    Ok(Json(serde_json::json!({
        "enabled": true,
        "status": "configuring",
        "operation_id": operation_id,
    })))
}

async fn disable_wal(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    let _ = load_cluster(&state, id).await?;
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE wal_streams SET enabled = 0, status = 'disabled', last_error = NULL, updated_at = ? WHERE cluster_id = ?",
    )
    .bind(&now)
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if result.rows_affected() == 0 {
        return Err(AppError(Error::NotFound("WAL stream".into())));
    }
    update_integration_wal_status(&state, id, "disabled").await?;
    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"action": "disable_wal"}),
        None,
        None,
    )
    .await;
    Ok(Json(
        serde_json::json!({"enabled": false, "status": "disabled"}),
    ))
}

async fn sync_wal(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    enqueue_wal_job(
        &state,
        &auth,
        id,
        JobType::SyncWal,
        serde_json::json!({}),
        "sync_wal",
    )
    .await
}

async fn switch_wal(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    enqueue_wal_job(
        &state,
        &auth,
        id,
        JobType::SyncWal,
        serde_json::json!({"switch": true}),
        "switch_wal",
    )
    .await
}

async fn base_backup(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    enqueue_wal_job(
        &state,
        &auth,
        id,
        JobType::WalBaseBackup,
        serde_json::json!({}),
        "wal_base_backup",
    )
    .await
}

async fn pitr_restore(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PitrRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    let cluster = load_cluster(&state, id).await?;
    if body.confirm_cluster_name != cluster.name {
        return Err(AppError(Error::ConfirmationRequired(
            "confirm_cluster_name must match the cluster name exactly".into(),
        )));
    }
    DateTime::parse_from_rfc3339(&body.target_time)
        .map_err(|_| AppError(Error::Validation("target_time must be RFC3339".into())))?;
    enqueue_wal_job(
        &state,
        &auth,
        id,
        JobType::WalPitrRestore,
        serde_json::json!({"target_time": body.target_time}),
        "wal_pitr_restore",
    )
    .await
}

async fn enqueue_wal_job(
    state: &AppState,
    auth: &AuthUser,
    id: Uuid,
    job_type: JobType,
    payload: serde_json::Value,
    action: &str,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(state, id).await?;
    let operation_id = state
        .queue
        .enqueue(job_type, Some(id), payload, None)
        .await
        .map_err(AppError)?;
    write_audit(
        state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"action": action, "operation_id": operation_id}),
        None,
        None,
    )
    .await;
    Ok(Json(serde_json::json!({"operation_id": operation_id})))
}

async fn list_segments(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<SegmentQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let limit = query.limit.clamp(1, 1000);
    let offset = query.offset.max(0);
    let q = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| format!("%{v}%"));
    let from = query.from.clone();
    let to = query.to.clone();
    let timeline = query.timeline;

    let mut sql = String::from(
        "SELECT id, cluster_id, filename, timeline, size_bytes, archived_at, synced_at, storage_key, checksum_sha256 FROM wal_segments WHERE cluster_id = ?",
    );
    let mut count_sql =
        String::from("SELECT COUNT(*) FROM wal_segments WHERE cluster_id = ?");
    if q.is_some() {
        sql.push_str(" AND filename LIKE ?");
        count_sql.push_str(" AND filename LIKE ?");
    }
    if from.is_some() {
        sql.push_str(" AND COALESCE(archived_at, synced_at) >= ?");
        count_sql.push_str(" AND COALESCE(archived_at, synced_at) >= ?");
    }
    if to.is_some() {
        sql.push_str(" AND COALESCE(archived_at, synced_at) <= ?");
        count_sql.push_str(" AND COALESCE(archived_at, synced_at) <= ?");
    }
    if timeline.is_some() {
        sql.push_str(" AND timeline = ?");
        count_sql.push_str(" AND timeline = ?");
    }
    sql.push_str(" ORDER BY COALESCE(archived_at, synced_at) DESC, filename DESC LIMIT ? OFFSET ?");

    let mut request = sqlx::query_as::<_, SegmentRow>(&sql).bind(id.to_string());
    let mut count_req = sqlx::query_scalar::<_, i64>(&count_sql).bind(id.to_string());
    if let Some(ref q) = q {
        request = request.bind(q);
        count_req = count_req.bind(q);
    }
    if let Some(ref from) = from {
        request = request.bind(from);
        count_req = count_req.bind(from);
    }
    if let Some(ref to) = to {
        request = request.bind(to);
        count_req = count_req.bind(to);
    }
    if let Some(timeline) = timeline {
        request = request.bind(timeline);
        count_req = count_req.bind(timeline);
    }

    let rows = request
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    let total = count_req
        .fetch_one(&state.pool)
        .await
        .unwrap_or(rows.len() as i64);

    Ok(Json(serde_json::json!({
        "segments": rows,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

async fn get_segment(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path((id, filename)): Path<(Uuid, String)>,
) -> ApiResult<Json<SegmentRow>> {
    let _ = load_cluster(&state, id).await?;
    pgpanel_backup::parse_wal_filename(&filename).map_err(AppError)?;
    let row = sqlx::query_as::<_, SegmentRow>(
        "SELECT id, cluster_id, filename, timeline, size_bytes, archived_at, synced_at, storage_key, checksum_sha256 FROM wal_segments WHERE cluster_id = ? AND filename = ?",
    )
    .bind(id.to_string())
    .bind(&filename)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?
    .ok_or_else(|| AppError(Error::NotFound("WAL segment".into())))?;
    Ok(Json(row))
}

async fn download_segment(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path((id, filename)): Path<(Uuid, String)>,
) -> ApiResult<Response> {
    let _ = load_cluster(&state, id).await?;
    pgpanel_backup::parse_wal_filename(&filename).map_err(AppError)?;
    let key: String = sqlx::query_scalar(
        "SELECT storage_key FROM wal_segments WHERE cluster_id = ? AND filename = ?",
    )
    .bind(id.to_string())
    .bind(&filename)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?
    .ok_or_else(|| AppError(Error::NotFound("WAL segment".into())))?;
    let bytes = state.backup.get_object(&key).await.map_err(AppError)?;
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(bytes))
        .map_err(|e| AppError(Error::Internal(format!("download response: {e}"))))
}

async fn delete_segment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, filename)): Path<(Uuid, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    let _ = load_cluster(&state, id).await?;
    pgpanel_backup::parse_wal_filename(&filename).map_err(AppError)?;
    let key: Option<String> = sqlx::query_scalar(
        "SELECT storage_key FROM wal_segments WHERE cluster_id = ? AND filename = ?",
    )
    .bind(id.to_string())
    .bind(&filename)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    let Some(key) = key else {
        return Err(AppError(Error::NotFound("WAL segment".into())));
    };
    state.backup.delete_object(&key).await.map_err(AppError)?;
    sqlx::query("DELETE FROM wal_segments WHERE cluster_id = ? AND filename = ?")
        .bind(id.to_string())
        .bind(&filename)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    sqlx::query(
        "UPDATE wal_streams SET segment_count = (SELECT COUNT(*) FROM wal_segments WHERE cluster_id = ?), total_bytes = (SELECT COALESCE(SUM(size_bytes), 0) FROM wal_segments WHERE cluster_id = ?), updated_at = ? WHERE cluster_id = ?",
    )
    .bind(id.to_string())
    .bind(id.to_string())
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .ok();
    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "wal_segment",
        Some(&filename),
        serde_json::json!({"action": "delete", "cluster_id": id}),
        None,
        None,
    )
    .await;
    Ok(Json(
        serde_json::json!({"deleted": true, "filename": filename}),
    ))
}

async fn update_integration_wal_status(
    state: &AppState,
    cluster_id: Uuid,
    status: &str,
) -> ApiResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO backup_integrations (
            id, cluster_id, status, external_id, wal_status, failed_backups, created_at, updated_at
        ) VALUES (?, ?, 'registered', 'native', ?, 0, ?, ?)
        ON CONFLICT(cluster_id) DO UPDATE SET wal_status = excluded.wal_status, updated_at = excluded.updated_at
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(cluster_id.to_string())
    .bind(status)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(())
}
