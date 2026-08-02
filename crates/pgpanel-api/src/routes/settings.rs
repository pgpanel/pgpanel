use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use pgpanel_backup::{LocalStorage, S3Storage, S3StorageConfig, StorageBackend};
use pgpanel_core::audit;
use pgpanel_core::crypto::encrypt_secret;
use pgpanel_core::error::Error;

use crate::auth::{write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings/wizard", get(get_wizard).post(set_wizard))
        .route("/api/settings", get(list_settings))
        .route("/api/settings/storage", get(get_storage).post(set_storage))
}

async fn get_wizard(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    let v: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'wizard_completed'")
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    let completed = matches!(v.as_deref(), Some("true") | Some("1") | Some("yes"));
    Ok(Json(serde_json::json!({ "wizard_completed": completed })))
}

#[derive(Deserialize)]
struct WizardBody {
    completed: bool,
}

#[derive(Debug, Serialize)]
struct StorageResponse {
    storage_type: String,
    endpoint: String,
    region: String,
    bucket: String,
    prefix: String,
    path_style: bool,
    tls_verify: bool,
    encrypt: bool,
    access_key_set: bool,
    secret_key_set: bool,
    retention_days: u32,
    keep_count: u32,
    schedule_hour: u8,
}

#[derive(Debug, Deserialize)]
struct StorageBody {
    #[serde(default = "default_storage_type")]
    storage_type: String,
    #[serde(default)]
    endpoint: String,
    #[serde(default = "default_region")]
    region: String,
    #[serde(default)]
    bucket: String,
    #[serde(default = "default_prefix")]
    prefix: String,
    #[serde(default = "default_true")]
    path_style: bool,
    #[serde(default = "default_true")]
    tls_verify: bool,
    #[serde(default = "default_true")]
    encrypt: bool,
    #[serde(default)]
    access_key: String,
    #[serde(default)]
    secret_key: String,
    #[serde(default = "default_retention_days")]
    retention_days: u32,
    #[serde(default = "default_keep_count")]
    keep_count: u32,
    #[serde(default = "default_schedule_hour")]
    schedule_hour: u8,
}

fn default_storage_type() -> String {
    "local".into()
}
fn default_region() -> String {
    "auto".into()
}
fn default_prefix() -> String {
    "pgpanel/".into()
}
fn default_true() -> bool {
    true
}
fn default_retention_days() -> u32 {
    14
}
fn default_keep_count() -> u32 {
    30
}
fn default_schedule_hour() -> u8 {
    3
}

async fn setting(state: &AppState, key: &str) -> ApiResult<Option<String>> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))
}

async fn put_setting(state: &AppState, key: &str, value: &str) -> ApiResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
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

fn parse_bool(value: Option<String>, default: bool) -> bool {
    value
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

fn parse_u32(value: Option<String>, default: u32) -> u32 {
    value.and_then(|v| v.parse().ok()).unwrap_or(default)
}

async fn get_storage(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<StorageResponse>> {
    Ok(Json(StorageResponse {
        storage_type: setting(&state, "storage.type")
            .await?
            .unwrap_or_else(|| state.backup.storage_kind()),
        endpoint: setting(&state, "storage.endpoint")
            .await?
            .unwrap_or_default(),
        region: setting(&state, "storage.region")
            .await?
            .unwrap_or_else(default_region),
        bucket: setting(&state, "storage.bucket").await?.unwrap_or_default(),
        prefix: setting(&state, "storage.prefix")
            .await?
            .unwrap_or_else(default_prefix),
        path_style: parse_bool(setting(&state, "storage.path_style").await?, true),
        tls_verify: parse_bool(setting(&state, "storage.tls_verify").await?, true),
        encrypt: parse_bool(
            setting(&state, "backup.encrypt").await?,
            state.config.backup_encrypt,
        ),
        access_key_set: setting(&state, "storage.access_key").await?.is_some(),
        secret_key_set: setting(&state, "storage.secret_key").await?.is_some(),
        retention_days: parse_u32(
            setting(&state, "backup.retention_days").await?,
            state.config.backup_retention_days,
        ),
        keep_count: parse_u32(
            setting(&state, "backup.keep_count").await?,
            state.config.backup_keep_count,
        ),
        schedule_hour: parse_u32(setting(&state, "backup.schedule_hour").await?, 3) as u8,
    }))
}

async fn set_storage(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<StorageBody>,
) -> ApiResult<Json<StorageResponse>> {
    let storage_type = body.storage_type.trim().to_lowercase();
    if !matches!(
        storage_type.as_str(),
        "local" | "s3" | "r2" | "b2" | "minio" | "hetzner"
    ) {
        return Err(AppError(Error::Validation(
            "unsupported storage type".into(),
        )));
    }
    if body.retention_days == 0
        || body.retention_days > 3650
        || body.keep_count == 0
        || body.keep_count > 10000
        || body.schedule_hour > 23
    {
        return Err(AppError(Error::Validation(
            "retention and keep count are outside the allowed range".into(),
        )));
    }

    let access_key = if body.access_key.trim().is_empty() {
        setting(&state, "storage.access_key").await?.and_then(|v| {
            pgpanel_core::crypto::decrypt_secret(&state.config.master_encryption_key, &v)
                .ok()
                .map(|s| s.expose_secret().to_string())
        })
    } else {
        Some(body.access_key.trim().to_string())
    };
    let secret_key = if body.secret_key.trim().is_empty() {
        setting(&state, "storage.secret_key").await?.and_then(|v| {
            pgpanel_core::crypto::decrypt_secret(&state.config.master_encryption_key, &v)
                .ok()
                .map(|s| s.expose_secret().to_string())
        })
    } else {
        Some(body.secret_key.trim().to_string())
    };

    let backend: Arc<dyn pgpanel_backup::StorageBackend> = if storage_type == "local" {
        Arc::new(LocalStorage::new(
            state
                .config
                .backup_data_dir
                .clone()
                .unwrap_or_else(|| state.config.data_dir.join("backups")),
        ))
    } else {
        let config = S3StorageConfig {
            endpoint: if body.endpoint.trim().is_empty() {
                None
            } else {
                Some(body.endpoint.trim().to_string())
            },
            region: body.region.trim().to_string(),
            bucket: body.bucket.trim().to_string(),
            access_key: access_key.ok_or_else(|| {
                AppError(Error::Validation(
                    "access key is required for S3 storage".into(),
                ))
            })?,
            secret_key: secret_key.ok_or_else(|| {
                AppError(Error::Validation(
                    "secret key is required for S3 storage".into(),
                ))
            })?,
            prefix: body.prefix.trim().trim_matches('/').to_string() + "/",
            path_style: body.path_style,
            tls_verify: body.tls_verify,
        };
        let storage = S3Storage::from_config(
            state
                .config
                .backup_data_dir
                .clone()
                .unwrap_or_else(|| state.config.data_dir.join("backups")),
            config,
        )
        .map_err(AppError)?;
        storage.test().await.map_err(AppError)?;
        Arc::new(storage)
    };

    put_setting(&state, "storage.type", &storage_type).await?;
    put_setting(&state, "storage.endpoint", body.endpoint.trim()).await?;
    put_setting(&state, "storage.region", body.region.trim()).await?;
    put_setting(&state, "storage.bucket", body.bucket.trim()).await?;
    put_setting(
        &state,
        "storage.prefix",
        &(body.prefix.trim().trim_matches('/').to_string() + "/"),
    )
    .await?;
    put_setting(
        &state,
        "storage.path_style",
        if body.path_style { "true" } else { "false" },
    )
    .await?;
    put_setting(
        &state,
        "storage.tls_verify",
        if body.tls_verify { "true" } else { "false" },
    )
    .await?;
    put_setting(
        &state,
        "backup.encrypt",
        if body.encrypt { "true" } else { "false" },
    )
    .await?;
    put_setting(
        &state,
        "backup.retention_days",
        &body.retention_days.to_string(),
    )
    .await?;
    put_setting(&state, "backup.keep_count", &body.keep_count.to_string()).await?;
    put_setting(
        &state,
        "backup.schedule_hour",
        &body.schedule_hour.to_string(),
    )
    .await?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE backup_schedules SET cron = ?, retention_days = ?, keep_count = ?, updated_at = ? WHERE enabled = 1",
    )
    .bind(format!("0 {} * * *", body.schedule_hour))
    .bind(body.retention_days as i64)
    .bind(body.keep_count as i64)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if !body.access_key.trim().is_empty() {
        let encrypted = encrypt_secret(
            &state.config.master_encryption_key,
            &secrecy::SecretString::from(body.access_key.trim().to_string()),
        )
        .map_err(AppError)?;
        put_setting(&state, "storage.access_key", &encrypted).await?;
    }
    if !body.secret_key.trim().is_empty() {
        let encrypted = encrypt_secret(
            &state.config.master_encryption_key,
            &secrecy::SecretString::from(body.secret_key.trim().to_string()),
        )
        .map_err(AppError)?;
        put_setting(&state, "storage.secret_key", &encrypted).await?;
    }
    state.backup.set_storage(backend).await;

    write_audit(
        &state,
        Some(&auth.user),
        audit::BACKUP_CONFIG,
        "storage",
        None,
        serde_json::json!({"storage_type": storage_type}),
        None,
        None,
    )
    .await;

    get_storage(State(state), auth).await
}

async fn set_wizard(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(body): Json<WizardBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let now = chrono::Utc::now().to_rfc3339();
    let val = if body.completed { "true" } else { "false" };
    sqlx::query(
        r#"
        INSERT INTO settings (key, value, updated_at) VALUES ('wizard_completed', ?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
        "#,
    )
    .bind(val)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        serde_json::json!({ "wizard_completed": body.completed }),
    ))
}

async fn list_settings(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM settings ORDER BY key")
            .fetch_all(&state.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    let map: serde_json::Map<String, serde_json::Value> = rows
        .into_iter()
        .map(|(k, v)| {
            let value = if matches!(k.as_str(), "storage.access_key" | "storage.secret_key") {
                serde_json::Value::String("***configured***".into())
            } else {
                serde_json::Value::String(v)
            };
            (k, value)
        })
        .collect();
    Ok(Json(serde_json::Value::Object(map)))
}
