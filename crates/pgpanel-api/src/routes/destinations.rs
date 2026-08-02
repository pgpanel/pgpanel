//! Backup destination CRUD and per-cluster backup targets.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use secrecy::ExposeSecret;
use uuid::Uuid;

use pgpanel_backup::{LocalStorage, S3Storage, S3StorageConfig, StorageBackend};
use pgpanel_core::audit;
use pgpanel_core::crypto::{decrypt_secret, encrypt_secret};
use pgpanel_core::error::Error;
use pgpanel_core::models::*;
use pgpanel_core::validation::{slugify, validate_display_name, validate_safe_name};

use crate::auth::{require_write, write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::routes::clusters::load_cluster;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/backup-destinations",
            get(list_destinations).post(create_destination),
        )
        .route(
            "/api/backup-destinations/{id}",
            get(get_destination)
                .put(update_destination)
                .delete(delete_destination),
        )
        .route("/api/backup-destinations/{id}/test", post(test_destination))
        .route(
            "/api/clusters/{id}/backup-targets",
            get(list_cluster_targets).post(upsert_cluster_target),
        )
        .route(
            "/api/clusters/{id}/backup-targets/{tid}",
            axum::routing::delete(delete_cluster_target),
        )
}

const DEST_SELECT: &str = r#"
    SELECT id, name, slug, storage_type, endpoint, region, bucket, prefix,
           path_style, tls_verify, encrypt_backups, compression_level, enabled, is_default,
           access_key_encrypted, secret_key_encrypted, notes,
           last_test_at, last_test_ok, last_test_error, created_at, updated_at
    FROM backup_destinations
"#;

#[derive(sqlx::FromRow)]
struct DestRow {
    id: String,
    name: String,
    slug: String,
    storage_type: String,
    endpoint: String,
    region: String,
    bucket: String,
    prefix: String,
    path_style: i64,
    tls_verify: i64,
    encrypt_backups: i64,
    compression_level: i64,
    enabled: i64,
    is_default: i64,
    access_key_encrypted: Option<String>,
    secret_key_encrypted: Option<String>,
    notes: Option<String>,
    last_test_at: Option<String>,
    last_test_ok: Option<i64>,
    last_test_error: Option<String>,
    created_at: String,
    updated_at: String,
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

impl DestRow {
    fn into_destination(self) -> Result<BackupDestination, Error> {
        Ok(BackupDestination {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            name: self.name,
            slug: self.slug,
            storage_type: self.storage_type,
            endpoint: self.endpoint,
            region: self.region,
            bucket: self.bucket,
            prefix: self.prefix,
            path_style: self.path_style != 0,
            tls_verify: self.tls_verify != 0,
            encrypt_backups: self.encrypt_backups != 0,
            compression_level: self.compression_level as u8,
            enabled: self.enabled != 0,
            is_default: self.is_default != 0,
            access_key_set: self.access_key_encrypted.is_some(),
            secret_key_set: self.secret_key_encrypted.is_some(),
            notes: self.notes,
            last_test_at: self.last_test_at.as_ref().map(|s| parse_dt(s)),
            last_test_ok: self.last_test_ok.map(|v| v != 0),
            last_test_error: self.last_test_error,
            created_at: parse_dt(&self.created_at),
            updated_at: parse_dt(&self.updated_at),
        })
    }
}

fn backup_data_dir(state: &AppState) -> std::path::PathBuf {
    state
        .config
        .backup_data_dir
        .clone()
        .unwrap_or_else(|| state.config.data_dir.join("backups"))
}

fn decrypt_credential(state: &AppState, enc: Option<&str>) -> Option<String> {
    enc.and_then(|e| {
        decrypt_secret(&state.config.master_encryption_key, e)
            .ok()
            .map(|s| s.expose_secret().to_string())
    })
}

async fn build_storage_backend(
    state: &AppState,
    row: &DestRow,
    access_key: Option<String>,
    secret_key: Option<String>,
) -> Result<Arc<dyn StorageBackend>, AppError> {
    let storage_type = row.storage_type.trim().to_lowercase();
    if storage_type == "local" {
        return Ok(Arc::new(LocalStorage::new(backup_data_dir(state))));
    }

    let access_key = access_key
        .or(decrypt_credential(
            state,
            row.access_key_encrypted.as_deref(),
        ))
        .ok_or_else(|| {
            AppError(Error::Validation(
                "access key is required for S3 storage".into(),
            ))
        })?;
    let secret_key = secret_key
        .or(decrypt_credential(
            state,
            row.secret_key_encrypted.as_deref(),
        ))
        .ok_or_else(|| {
            AppError(Error::Validation(
                "secret key is required for S3 storage".into(),
            ))
        })?;

    let config = S3StorageConfig {
        endpoint: if row.endpoint.trim().is_empty() {
            None
        } else {
            Some(row.endpoint.trim().to_string())
        },
        region: row.region.trim().to_string(),
        bucket: row.bucket.trim().to_string(),
        access_key,
        secret_key,
        prefix: row.prefix.trim().trim_matches('/').to_string() + "/",
        path_style: row.path_style != 0,
        tls_verify: row.tls_verify != 0,
    };
    let storage = S3Storage::from_config(backup_data_dir(state), config).map_err(AppError)?;
    Ok(Arc::new(storage))
}

async fn test_storage(
    state: &AppState,
    row: &DestRow,
    access_key: Option<String>,
    secret_key: Option<String>,
) -> Result<(), AppError> {
    let backend = build_storage_backend(state, row, access_key, secret_key).await?;
    backend.test().await.map_err(AppError)?;
    Ok(())
}

async fn replace_node_allowlist(
    state: &AppState,
    destination_id: Uuid,
    node_ids: &[Uuid],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM node_backup_destinations WHERE destination_id = ?")
        .bind(destination_id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    for nid in node_ids {
        sqlx::query(
            "INSERT INTO node_backup_destinations (node_id, destination_id, prefer) VALUES (?, ?, 0)",
        )
        .bind(nid.to_string())
        .bind(destination_id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    }
    Ok(())
}

fn validate_storage_type(storage_type: &str) -> Result<(), AppError> {
    if !matches!(
        storage_type,
        "local" | "s3" | "r2" | "b2" | "minio" | "hetzner"
    ) {
        return Err(AppError(Error::Validation(
            "unsupported storage type".into(),
        )));
    }
    Ok(())
}

async fn list_destinations(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<BackupDestination>>> {
    let rows =
        sqlx::query_as::<_, DestRow>(&format!("{DEST_SELECT} ORDER BY is_default DESC, name"))
            .fetch_all(&state.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_destination().ok())
            .collect(),
    ))
}

async fn get_destination(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<BackupDestination>> {
    let row = sqlx::query_as::<_, DestRow>(&format!("{DEST_SELECT} WHERE id = ?"))
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?
        .ok_or_else(|| AppError(Error::NotFound("backup destination".into())))?;
    Ok(Json(row.into_destination().map_err(AppError)?))
}

async fn create_destination(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpsertBackupDestinationRequest>,
) -> ApiResult<Json<BackupDestination>> {
    require_write(&auth)?;
    validate_display_name(&req.name).map_err(AppError)?;
    let slug = slugify(&req.name);
    validate_safe_name(&slug, "destination slug").map_err(AppError)?;

    let storage_type = req.storage_type.trim().to_lowercase();
    validate_storage_type(&storage_type)?;

    let access_key = if req.access_key.trim().is_empty() {
        None
    } else {
        Some(req.access_key.trim().to_string())
    };
    let secret_key = if req.secret_key.trim().is_empty() {
        None
    } else {
        Some(req.secret_key.trim().to_string())
    };

    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let prefix = req.prefix.trim().trim_matches('/').to_string() + "/";

    let probe_row = DestRow {
        id: id.to_string(),
        name: req.name.trim().to_string(),
        slug: slug.clone(),
        storage_type: storage_type.clone(),
        endpoint: req.endpoint.trim().to_string(),
        region: req.region.trim().to_string(),
        bucket: req.bucket.trim().to_string(),
        prefix: prefix.clone(),
        path_style: if req.path_style { 1 } else { 0 },
        tls_verify: if req.tls_verify { 1 } else { 0 },
        encrypt_backups: if req.encrypt_backups { 1 } else { 0 },
        compression_level: req.compression_level as i64,
        enabled: if req.enabled { 1 } else { 0 },
        is_default: if req.set_default { 1 } else { 0 },
        access_key_encrypted: None,
        secret_key_encrypted: None,
        notes: req.notes.clone(),
        last_test_at: None,
        last_test_ok: None,
        last_test_error: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    if storage_type != "local" {
        test_storage(&state, &probe_row, access_key.clone(), secret_key.clone()).await?;
    }

    let access_enc = access_key
        .as_ref()
        .map(|k| {
            encrypt_secret(
                &state.config.master_encryption_key,
                &secrecy::SecretString::from(k.clone()),
            )
            .map_err(AppError)
        })
        .transpose()?;
    let secret_enc = secret_key
        .as_ref()
        .map(|k| {
            encrypt_secret(
                &state.config.master_encryption_key,
                &secrecy::SecretString::from(k.clone()),
            )
            .map_err(AppError)
        })
        .transpose()?;

    if req.set_default {
        sqlx::query("UPDATE backup_destinations SET is_default = 0")
            .execute(&state.pool)
            .await
            .ok();
    }

    sqlx::query(
        r#"
        INSERT INTO backup_destinations (
            id, name, slug, storage_type, endpoint, region, bucket, prefix,
            path_style, tls_verify, access_key_encrypted, secret_key_encrypted,
            encrypt_backups, compression_level, enabled, is_default, notes,
            created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(id.to_string())
    .bind(req.name.trim())
    .bind(&slug)
    .bind(&storage_type)
    .bind(req.endpoint.trim())
    .bind(req.region.trim())
    .bind(req.bucket.trim())
    .bind(&prefix)
    .bind(if req.path_style { 1 } else { 0 })
    .bind(if req.tls_verify { 1 } else { 0 })
    .bind(&access_enc)
    .bind(&secret_enc)
    .bind(if req.encrypt_backups { 1 } else { 0 })
    .bind(req.compression_level as i64)
    .bind(if req.enabled { 1 } else { 0 })
    .bind(if req.set_default { 1 } else { 0 })
    .bind(req.notes.as_deref())
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError(Error::Conflict(
                "destination name or slug already exists".into(),
            ))
        } else {
            AppError(Error::Internal(e.to_string()))
        }
    })?;

    if !req.allowed_node_ids.is_empty() {
        replace_node_allowlist(&state, id, &req.allowed_node_ids).await?;
    }

    write_audit(
        &state,
        Some(&auth.user),
        audit::DEST_CREATE,
        "backup_destination",
        Some(&id.to_string()),
        serde_json::json!({"name": req.name, "slug": slug, "storage_type": storage_type}),
        None,
        None,
    )
    .await;

    get_destination(State(state), auth, Path(id)).await
}

async fn update_destination(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertBackupDestinationRequest>,
) -> ApiResult<Json<BackupDestination>> {
    require_write(&auth)?;

    let existing = sqlx::query_as::<_, DestRow>(&format!("{DEST_SELECT} WHERE id = ?"))
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?
        .ok_or_else(|| AppError(Error::NotFound("backup destination".into())))?;

    validate_display_name(&req.name).map_err(AppError)?;
    let storage_type = req.storage_type.trim().to_lowercase();
    validate_storage_type(&storage_type)?;

    let access_key = if req.access_key.trim().is_empty() {
        None
    } else {
        Some(req.access_key.trim().to_string())
    };
    let secret_key = if req.secret_key.trim().is_empty() {
        None
    } else {
        Some(req.secret_key.trim().to_string())
    };

    let now = Utc::now().to_rfc3339();
    let prefix = req.prefix.trim().trim_matches('/').to_string() + "/";

    let probe_row = DestRow {
        id: existing.id.clone(),
        name: req.name.trim().to_string(),
        slug: existing.slug.clone(),
        storage_type: storage_type.clone(),
        endpoint: req.endpoint.trim().to_string(),
        region: req.region.trim().to_string(),
        bucket: req.bucket.trim().to_string(),
        prefix: prefix.clone(),
        path_style: if req.path_style { 1 } else { 0 },
        tls_verify: if req.tls_verify { 1 } else { 0 },
        encrypt_backups: if req.encrypt_backups { 1 } else { 0 },
        compression_level: req.compression_level as i64,
        enabled: if req.enabled { 1 } else { 0 },
        is_default: if req.set_default { 1 } else { 0 },
        access_key_encrypted: existing.access_key_encrypted.clone(),
        secret_key_encrypted: existing.secret_key_encrypted.clone(),
        notes: req.notes.clone(),
        last_test_at: existing.last_test_at.clone(),
        last_test_ok: existing.last_test_ok,
        last_test_error: existing.last_test_error.clone(),
        created_at: existing.created_at.clone(),
        updated_at: now.clone(),
    };

    if storage_type != "local" {
        test_storage(&state, &probe_row, access_key.clone(), secret_key.clone()).await?;
    }

    let access_enc = if let Some(k) = access_key {
        Some(
            encrypt_secret(
                &state.config.master_encryption_key,
                &secrecy::SecretString::from(k),
            )
            .map_err(AppError)?,
        )
    } else {
        existing.access_key_encrypted.clone()
    };
    let secret_enc = if let Some(k) = secret_key {
        Some(
            encrypt_secret(
                &state.config.master_encryption_key,
                &secrecy::SecretString::from(k),
            )
            .map_err(AppError)?,
        )
    } else {
        existing.secret_key_encrypted.clone()
    };

    if req.set_default {
        sqlx::query("UPDATE backup_destinations SET is_default = 0")
            .execute(&state.pool)
            .await
            .ok();
    }

    sqlx::query(
        r#"
        UPDATE backup_destinations SET
            name = ?, storage_type = ?, endpoint = ?, region = ?, bucket = ?, prefix = ?,
            path_style = ?, tls_verify = ?, access_key_encrypted = ?, secret_key_encrypted = ?,
            encrypt_backups = ?, compression_level = ?, enabled = ?, is_default = ?, notes = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(req.name.trim())
    .bind(&storage_type)
    .bind(req.endpoint.trim())
    .bind(req.region.trim())
    .bind(req.bucket.trim())
    .bind(&prefix)
    .bind(if req.path_style { 1 } else { 0 })
    .bind(if req.tls_verify { 1 } else { 0 })
    .bind(&access_enc)
    .bind(&secret_enc)
    .bind(if req.encrypt_backups { 1 } else { 0 })
    .bind(req.compression_level as i64)
    .bind(if req.enabled { 1 } else { 0 })
    .bind(if req.set_default { 1 } else { 0 })
    .bind(req.notes.as_deref())
    .bind(&now)
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    replace_node_allowlist(&state, id, &req.allowed_node_ids).await?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::DEST_UPDATE,
        "backup_destination",
        Some(&id.to_string()),
        serde_json::json!({"name": req.name, "storage_type": storage_type}),
        None,
        None,
    )
    .await;

    get_destination(State(state), auth, Path(id)).await
}

async fn delete_destination(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;

    let res = sqlx::query("DELETE FROM backup_destinations WHERE id = ?")
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if res.rows_affected() == 0 {
        return Err(AppError(Error::NotFound("backup destination".into())));
    }

    write_audit(
        &state,
        Some(&auth.user),
        audit::DEST_DELETE,
        "backup_destination",
        Some(&id.to_string()),
        serde_json::json!({}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"ok": true})))
}

async fn test_destination(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;

    let row = sqlx::query_as::<_, DestRow>(&format!("{DEST_SELECT} WHERE id = ?"))
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?
        .ok_or_else(|| AppError(Error::NotFound("backup destination".into())))?;

    let now = Utc::now().to_rfc3339();
    let result = test_storage(&state, &row, None, None).await;

    let (ok, err) = match result {
        Ok(()) => (1, None),
        Err(AppError(e)) => (0, Some(e.to_string())),
    };

    sqlx::query(
        r#"
        UPDATE backup_destinations SET last_test_at = ?, last_test_ok = ?, last_test_error = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&now)
    .bind(ok)
    .bind(&err)
    .bind(&now)
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    if ok == 0 {
        return Err(AppError(Error::Validation(
            err.unwrap_or_else(|| "storage test failed".into()),
        )));
    }

    let _ = auth;
    Ok(Json(serde_json::json!({"ok": true, "tested_at": now})))
}

const TARGET_SELECT: &str = r#"
    SELECT t.id, t.cluster_id, t.destination_id, d.name AS destination_name,
           t.enabled, t.priority, t.include_databases, t.exclude_databases, t.cron,
           t.retention_days, t.keep_count, t.schema_only, t.verify_after,
           t.compression_level, t.dump_format, t.exclude_schemas, t.exclude_tables,
           t.parallel_jobs, t.notify_on_success, t.notify_on_failure,
           t.window_start_hour, t.window_end_hour, t.last_run_at, t.last_status
    FROM cluster_backup_targets t
    JOIN backup_destinations d ON d.id = t.destination_id
"#;

#[derive(sqlx::FromRow)]
struct TargetRow {
    id: String,
    cluster_id: String,
    destination_id: String,
    destination_name: String,
    enabled: i64,
    priority: i64,
    include_databases: String,
    exclude_databases: String,
    cron: String,
    retention_days: i64,
    keep_count: i64,
    schema_only: i64,
    verify_after: i64,
    compression_level: Option<i64>,
    dump_format: String,
    exclude_schemas: String,
    exclude_tables: String,
    parallel_jobs: i64,
    notify_on_success: i64,
    notify_on_failure: i64,
    window_start_hour: Option<i64>,
    window_end_hour: Option<i64>,
    last_run_at: Option<String>,
    last_status: Option<String>,
}

impl TargetRow {
    fn into_target(self) -> Result<ClusterBackupTarget, Error> {
        Ok(ClusterBackupTarget {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            cluster_id: Uuid::parse_str(&self.cluster_id)
                .map_err(|e| Error::Internal(e.to_string()))?,
            destination_id: Uuid::parse_str(&self.destination_id)
                .map_err(|e| Error::Internal(e.to_string()))?,
            destination_name: self.destination_name,
            enabled: self.enabled != 0,
            priority: self.priority as i32,
            include_databases: self.include_databases,
            exclude_databases: self.exclude_databases,
            cron: self.cron,
            retention_days: self.retention_days as u32,
            keep_count: self.keep_count as u32,
            schema_only: self.schema_only != 0,
            verify_after: self.verify_after != 0,
            compression_level: self.compression_level.map(|n| n as u8),
            dump_format: self.dump_format,
            exclude_schemas: self.exclude_schemas,
            exclude_tables: self.exclude_tables,
            parallel_jobs: self.parallel_jobs as u8,
            notify_on_success: self.notify_on_success != 0,
            notify_on_failure: self.notify_on_failure != 0,
            window_start_hour: self.window_start_hour.map(|n| n as u8),
            window_end_hour: self.window_end_hour.map(|n| n as u8),
            last_run_at: self.last_run_at.as_ref().map(|s| parse_dt(s)),
            last_status: self.last_status,
        })
    }
}

async fn list_cluster_targets(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ClusterBackupTarget>>> {
    let _ = load_cluster(&state, id).await?;
    let rows = sqlx::query_as::<_, TargetRow>(&format!(
        "{TARGET_SELECT} WHERE t.cluster_id = ? ORDER BY t.priority, d.name"
    ))
    .bind(id.to_string())
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_target().ok())
            .collect(),
    ))
}

async fn upsert_cluster_target(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertClusterBackupTargetRequest>,
) -> ApiResult<Json<ClusterBackupTarget>> {
    require_write(&auth)?;
    let _ = load_cluster(&state, id).await?;

    let dest_exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM backup_destinations WHERE id = ?")
            .bind(req.destination_id.to_string())
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if dest_exists.is_none() {
        return Err(AppError(Error::NotFound("backup destination".into())));
    }

    let now = Utc::now().to_rfc3339();
    let existing_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM cluster_backup_targets WHERE cluster_id = ? AND destination_id = ? AND include_databases = ?",
    )
    .bind(id.to_string())
    .bind(req.destination_id.to_string())
    .bind(&req.include_databases)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let target_id = if let Some(tid) = existing_id {
        sqlx::query(
            r#"
            UPDATE cluster_backup_targets SET
                enabled = ?, priority = ?, exclude_databases = ?, cron = ?,
                retention_days = ?, keep_count = ?, schema_only = ?, verify_after = ?,
                compression_level = ?, dump_format = ?, exclude_schemas = ?, exclude_tables = ?,
                parallel_jobs = ?, notify_on_success = ?, notify_on_failure = ?,
                window_start_hour = ?, window_end_hour = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(if req.enabled { 1 } else { 0 })
        .bind(req.priority as i64)
        .bind(&req.exclude_databases)
        .bind(&req.cron)
        .bind(req.retention_days as i64)
        .bind(req.keep_count as i64)
        .bind(if req.schema_only { 1 } else { 0 })
        .bind(if req.verify_after { 1 } else { 0 })
        .bind(req.compression_level.map(|n| n as i64))
        .bind(&req.dump_format)
        .bind(&req.exclude_schemas)
        .bind(&req.exclude_tables)
        .bind(req.parallel_jobs as i64)
        .bind(if req.notify_on_success { 1 } else { 0 })
        .bind(if req.notify_on_failure { 1 } else { 0 })
        .bind(req.window_start_hour.map(|n| n as i64))
        .bind(req.window_end_hour.map(|n| n as i64))
        .bind(&now)
        .bind(&tid)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
        Uuid::parse_str(&tid).map_err(|e| AppError(Error::Internal(e.to_string())))?
    } else {
        let tid = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO cluster_backup_targets (
                id, cluster_id, destination_id, enabled, priority, include_databases,
                exclude_databases, cron, retention_days, keep_count, schema_only, verify_after,
                compression_level, dump_format, exclude_schemas, exclude_tables, parallel_jobs,
                notify_on_success, notify_on_failure, window_start_hour, window_end_hour,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(tid.to_string())
        .bind(id.to_string())
        .bind(req.destination_id.to_string())
        .bind(if req.enabled { 1 } else { 0 })
        .bind(req.priority as i64)
        .bind(&req.include_databases)
        .bind(&req.exclude_databases)
        .bind(&req.cron)
        .bind(req.retention_days as i64)
        .bind(req.keep_count as i64)
        .bind(if req.schema_only { 1 } else { 0 })
        .bind(if req.verify_after { 1 } else { 0 })
        .bind(req.compression_level.map(|n| n as i64))
        .bind(&req.dump_format)
        .bind(&req.exclude_schemas)
        .bind(&req.exclude_tables)
        .bind(req.parallel_jobs as i64)
        .bind(if req.notify_on_success { 1 } else { 0 })
        .bind(if req.notify_on_failure { 1 } else { 0 })
        .bind(req.window_start_hour.map(|n| n as i64))
        .bind(req.window_end_hour.map(|n| n as i64))
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
        tid
    };

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster_backup_target",
        Some(&target_id.to_string()),
        serde_json::json!({
            "cluster_id": id,
            "destination_id": req.destination_id,
        }),
        None,
        None,
    )
    .await;

    let row = sqlx::query_as::<_, TargetRow>(&format!("{TARGET_SELECT} WHERE t.id = ?"))
        .bind(target_id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?
        .ok_or_else(|| AppError(Error::NotFound("cluster backup target".into())))?;
    Ok(Json(row.into_target().map_err(AppError)?))
}

async fn delete_cluster_target(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, tid)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    let _ = load_cluster(&state, id).await?;

    let res = sqlx::query("DELETE FROM cluster_backup_targets WHERE id = ? AND cluster_id = ?")
        .bind(tid.to_string())
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if res.rows_affected() == 0 {
        return Err(AppError(Error::NotFound("cluster backup target".into())));
    }

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "cluster_backup_target",
        Some(&tid.to_string()),
        serde_json::json!({"action": "delete", "cluster_id": id}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"ok": true})))
}
