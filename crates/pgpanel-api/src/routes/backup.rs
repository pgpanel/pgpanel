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
        .route("/api/clusters/{id}/backup/restore", post(restore))
        .route("/api/clusters/{id}/backup/prune", post(prune))
        .route(
            "/api/clusters/{id}/backup/schedules",
            get(list_schedules).post(upsert_schedule),
        )
        .route(
            "/api/clusters/{id}/backup/schedules/{sid}",
            axum::routing::delete(delete_schedule),
        )
        .route("/api/clusters/{id}/backup/history", get(history))
        .route("/api/clusters/{id}/metrics", get(metrics))
        .route("/api/settings/backup-policy", get(get_policy).post(set_policy))
}

#[derive(Deserialize)]
struct TriggerBody {
    #[serde(default = "default_db")]
    database: String,
    #[serde(default)]
    schema_only: bool,
    #[serde(default)]
    exclude_schemas: String,
    #[serde(default)]
    exclude_tables: String,
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
        wal_status: Some(format!(
            "engine=native storage={}",
            state.backup.storage_kind()
        )),
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
        exclude_schemas: String::new(),
        exclude_tables: String::new(),
    });

    let op = state
        .queue
        .enqueue(
            JobType::RunBackup,
            Some(id),
            serde_json::json!({
                "database": body.database,
                "schema_only": body.schema_only,
                "exclude_schemas": body.exclude_schemas,
                "exclude_tables": body.exclude_tables,
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
        .enqueue(JobType::VerifyBackup, Some(id), serde_json::json!({}), None)
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

async fn restore(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<RestoreBackupRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let cluster = load_cluster(&state, id).await?;
    if body.confirm_cluster_name != cluster.name {
        return Err(AppError(Error::Validation(
            "confirm_cluster_name must match the cluster name exactly".into(),
        )));
    }
    if body.target_database.trim().is_empty() {
        return Err(AppError(Error::Validation(
            "target_database is required".into(),
        )));
    }

    let op = state
        .queue
        .enqueue(
            JobType::RestoreBackup,
            Some(id),
            serde_json::json!({
                "backup_id": body.backup_id,
                "target_database": body.target_database,
                "clean": body.clean,
            }),
            None,
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_RESTORE,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({
            "backup_id": body.backup_id,
            "target_database": body.target_database,
            "clean": body.clean,
        }),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"operation_id": op})))
}

async fn prune(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let op = state
        .queue
        .enqueue(JobType::PruneBackups, Some(id), serde_json::json!({}), None)
        .await
        .map_err(AppError)?;
    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"action": "prune"}),
        None,
        None,
    )
    .await;
    Ok(Json(serde_json::json!({"operation_id": op})))
}

#[derive(sqlx::FromRow)]
struct ScheduleRow {
    id: String,
    cluster_id: String,
    cron: String,
    kind: String,
    database_name: String,
    enabled: i64,
    retention_days: i64,
    keep_count: i64,
    compression_level: i64,
    dump_format: String,
    schema_only: i64,
    exclude_schemas: String,
    exclude_tables: String,
    include_schemas: String,
    jobs: i64,
    notify_on_success: i64,
    notify_on_failure: i64,
    verify_after: i64,
    window_start_hour: Option<i64>,
    window_end_hour: Option<i64>,
    pause_until: Option<String>,
    next_run_at: Option<String>,
    description: Option<String>,
    last_run_at: Option<String>,
    created_at: String,
    updated_at: String,
}

fn parse_opt_dt(s: &Option<String>) -> Option<chrono::DateTime<chrono::Utc>> {
    s.as_ref().and_then(|v| {
        chrono::DateTime::parse_from_rfc3339(v)
            .ok()
            .map(|d| d.with_timezone(&chrono::Utc))
    })
}

impl ScheduleRow {
    fn into_schedule(self) -> Result<BackupSchedule, Error> {
        Ok(BackupSchedule {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            cluster_id: Uuid::parse_str(&self.cluster_id)
                .map_err(|e| Error::Internal(e.to_string()))?,
            cron: self.cron,
            kind: self.kind,
            database_name: self.database_name,
            enabled: self.enabled != 0,
            retention_days: self.retention_days.max(0) as u32,
            keep_count: self.keep_count.max(0) as u32,
            compression_level: self.compression_level.clamp(0, 9) as u8,
            dump_format: self.dump_format,
            schema_only: self.schema_only != 0,
            exclude_schemas: self.exclude_schemas,
            exclude_tables: self.exclude_tables,
            include_schemas: self.include_schemas,
            jobs: self.jobs.clamp(1, 16) as u8,
            notify_on_success: self.notify_on_success != 0,
            notify_on_failure: self.notify_on_failure != 0,
            verify_after: self.verify_after != 0,
            window_start_hour: self.window_start_hour.map(|h| h.clamp(0, 23) as u8),
            window_end_hour: self.window_end_hour.map(|h| h.clamp(0, 23) as u8),
            pause_until: parse_opt_dt(&self.pause_until),
            next_run_at: parse_opt_dt(&self.next_run_at),
            description: self.description,
            last_run_at: parse_opt_dt(&self.last_run_at),
            created_at: parse_opt_dt(&Some(self.created_at)).unwrap_or_else(chrono::Utc::now),
            updated_at: parse_opt_dt(&Some(self.updated_at)).unwrap_or_else(chrono::Utc::now),
        })
    }
}

async fn list_schedules(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<BackupSchedule>>> {
    let _ = load_cluster(&state, id).await?;
    let rows = sqlx::query_as::<_, ScheduleRow>(
        r#"
        SELECT id, cluster_id, cron, kind, database_name, enabled, retention_days, keep_count,
               compression_level, dump_format, schema_only, exclude_schemas, exclude_tables,
               include_schemas, jobs, notify_on_success, notify_on_failure, verify_after,
               window_start_hour, window_end_hour, pause_until, next_run_at, description,
               last_run_at, created_at, updated_at
        FROM backup_schedules WHERE cluster_id = ? ORDER BY database_name
        "#,
    )
    .bind(id.to_string())
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_schedule().ok())
            .collect(),
    ))
}

async fn upsert_schedule(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpsertBackupScheduleRequest>,
) -> ApiResult<Json<BackupSchedule>> {
    let _ = load_cluster(&state, id).await?;
    if body.retention_days == 0 || body.retention_days > 3650 || body.keep_count == 0 {
        return Err(AppError(Error::Validation(
            "retention_days / keep_count out of range".into(),
        )));
    }
    if body.cron.split_whitespace().count() < 2 {
        return Err(AppError(Error::Validation(
            "cron must have at least minute and hour fields".into(),
        )));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let sid = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO backup_schedules (
            id, cluster_id, cron, kind, database_name, enabled, retention_days, keep_count,
            compression_level, dump_format, schema_only, exclude_schemas, exclude_tables,
            include_schemas, jobs, notify_on_success, notify_on_failure, verify_after,
            window_start_hour, window_end_hour, description, created_at, updated_at
        ) VALUES (?, ?, ?, 'logical_full', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(cluster_id, database_name, kind) DO UPDATE SET
            cron = excluded.cron,
            enabled = excluded.enabled,
            retention_days = excluded.retention_days,
            keep_count = excluded.keep_count,
            compression_level = excluded.compression_level,
            dump_format = excluded.dump_format,
            schema_only = excluded.schema_only,
            exclude_schemas = excluded.exclude_schemas,
            exclude_tables = excluded.exclude_tables,
            include_schemas = excluded.include_schemas,
            jobs = excluded.jobs,
            notify_on_success = excluded.notify_on_success,
            notify_on_failure = excluded.notify_on_failure,
            verify_after = excluded.verify_after,
            window_start_hour = excluded.window_start_hour,
            window_end_hour = excluded.window_end_hour,
            description = excluded.description,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(sid.to_string())
    .bind(id.to_string())
    .bind(&body.cron)
    .bind(&body.database_name)
    .bind(if body.enabled { 1 } else { 0 })
    .bind(body.retention_days as i64)
    .bind(body.keep_count as i64)
    .bind(body.compression_level as i64)
    .bind(&body.dump_format)
    .bind(if body.schema_only { 1 } else { 0 })
    .bind(&body.exclude_schemas)
    .bind(&body.exclude_tables)
    .bind(&body.include_schemas)
    .bind(body.jobs as i64)
    .bind(if body.notify_on_success { 1 } else { 0 })
    .bind(if body.notify_on_failure { 1 } else { 0 })
    .bind(if body.verify_after { 1 } else { 0 })
    .bind(body.window_start_hour.map(|h| h as i64))
    .bind(body.window_end_hour.map(|h| h as i64))
    .bind(body.description.as_deref())
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"action": "upsert_schedule", "database": body.database_name, "cron": body.cron}),
        None,
        None,
    )
    .await;

    let db_name = body.database_name.clone();
    let schedules = list_schedules(State(state), auth, Path(id)).await?;
    schedules
        .0
        .into_iter()
        .find(|s| s.database_name == db_name)
        .map(Json)
        .ok_or_else(|| AppError(Error::Internal("schedule not found after upsert".into())))
}

async fn delete_schedule(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, sid)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let res = sqlx::query("DELETE FROM backup_schedules WHERE id = ? AND cluster_id = ?")
        .bind(sid.to_string())
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if res.rows_affected() == 0 {
        return Err(AppError(Error::NotFound("schedule".into())));
    }
    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"action": "delete_schedule", "schedule_id": sid}),
        None,
        None,
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn setting_val(state: &AppState, key: &str) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
}

async fn get_policy(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<GlobalBackupPolicy>> {
    let parse_bool = |v: Option<String>, d: bool| {
        v.map(|x| matches!(x.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(d)
    };
    let parse_u32 = |v: Option<String>, d: u32| v.and_then(|x| x.parse().ok()).unwrap_or(d);
    Ok(Json(GlobalBackupPolicy {
        retention_days: parse_u32(
            setting_val(&state, "backup.retention_days").await,
            state.config.backup_retention_days,
        ),
        keep_count: parse_u32(
            setting_val(&state, "backup.keep_count").await,
            state.config.backup_keep_count,
        ),
        schedule_hour: parse_u32(setting_val(&state, "backup.schedule_hour").await, 3) as u8,
        cron_default: setting_val(&state, "backup.cron_default")
            .await
            .unwrap_or_else(|| "0 3 * * *".into()),
        compression_level: parse_u32(setting_val(&state, "backup.compression_level").await, 6)
            as u8,
        dump_format: setting_val(&state, "backup.dump_format")
            .await
            .unwrap_or_else(|| "custom".into()),
        verify_after: parse_bool(setting_val(&state, "backup.verify_after").await, false),
        keep_local_copy: parse_bool(setting_val(&state, "backup.keep_local_copy").await, false),
        notify_webhook: setting_val(&state, "backup.notify_webhook")
            .await
            .unwrap_or_default(),
        notify_on_success: parse_bool(setting_val(&state, "backup.notify_on_success").await, false),
        notify_on_failure: parse_bool(setting_val(&state, "backup.notify_on_failure").await, true),
        exclude_schemas_default: setting_val(&state, "backup.exclude_schemas_default")
            .await
            .unwrap_or_default(),
        wal_archiving_default: parse_bool(
            setting_val(&state, "backup.wal_archiving_default").await,
            false,
        ),
        parallel_jobs: parse_u32(setting_val(&state, "backup.parallel_jobs").await, 1) as u8,
        encrypt: parse_bool(
            setting_val(&state, "backup.encrypt").await,
            state.config.backup_encrypt,
        ),
    }))
}

async fn set_policy(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<GlobalBackupPolicy>,
) -> ApiResult<Json<GlobalBackupPolicy>> {
    if body.retention_days == 0
        || body.retention_days > 3650
        || body.keep_count == 0
        || body.compression_level > 9
        || body.parallel_jobs == 0
        || body.parallel_jobs > 16
    {
        return Err(AppError(Error::Validation(
            "backup policy values out of range".into(),
        )));
    }
    let now = chrono::Utc::now().to_rfc3339();
    async fn put(state: &AppState, key: &str, value: &str, now: &str) -> ApiResult<()> {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .bind(now)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
        Ok(())
    }
    put(
        &state,
        "backup.retention_days",
        &body.retention_days.to_string(),
        &now,
    )
    .await?;
    put(&state, "backup.keep_count", &body.keep_count.to_string(), &now).await?;
    put(
        &state,
        "backup.schedule_hour",
        &body.schedule_hour.to_string(),
        &now,
    )
    .await?;
    put(&state, "backup.cron_default", &body.cron_default, &now).await?;
    put(
        &state,
        "backup.compression_level",
        &body.compression_level.to_string(),
        &now,
    )
    .await?;
    put(&state, "backup.dump_format", &body.dump_format, &now).await?;
    put(
        &state,
        "backup.verify_after",
        &body.verify_after.to_string(),
        &now,
    )
    .await?;
    put(
        &state,
        "backup.keep_local_copy",
        &body.keep_local_copy.to_string(),
        &now,
    )
    .await?;
    put(&state, "backup.notify_webhook", &body.notify_webhook, &now).await?;
    put(
        &state,
        "backup.notify_on_success",
        &body.notify_on_success.to_string(),
        &now,
    )
    .await?;
    put(
        &state,
        "backup.notify_on_failure",
        &body.notify_on_failure.to_string(),
        &now,
    )
    .await?;
    put(
        &state,
        "backup.exclude_schemas_default",
        &body.exclude_schemas_default,
        &now,
    )
    .await?;
    put(
        &state,
        "backup.wal_archiving_default",
        &body.wal_archiving_default.to_string(),
        &now,
    )
    .await?;
    put(
        &state,
        "backup.parallel_jobs",
        &body.parallel_jobs.to_string(),
        &now,
    )
    .await?;
    put(&state, "backup.encrypt", &body.encrypt.to_string(), &now).await?;
    if body.keep_local_copy {
        std::env::set_var("BACKUP_KEEP_LOCAL", "true");
    } else {
        std::env::set_var("BACKUP_KEEP_LOCAL", "false");
    }
    if !body.notify_webhook.is_empty() {
        std::env::set_var("WEBHOOK_URL", &body.notify_webhook);
    }

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "backup_policy",
        None,
        serde_json::json!({"retention_days": body.retention_days, "cron": body.cron_default}),
        None,
        None,
    )
    .await;

    get_policy(State(state), auth).await
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
        state.provisioner.docker().container_stats(name).await.ok()
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
