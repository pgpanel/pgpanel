//! PgPanel API server.
#![forbid(unsafe_code)]

mod auth;
mod db;
mod error;
#[allow(dead_code)]
mod openapi;
mod routes;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use pgpanel_backup::{build_engine, LocalStorage, S3Storage, S3StorageConfig, StorageBackend};
use pgpanel_core::config::Config;
use pgpanel_core::crypto::decrypt_secret;
use pgpanel_docker::{ClusterProvisioner, DockerClient};
use pgpanel_jobs::{JobContext, JobQueue, JobWorker};
use secrecy::ExposeSecret;

use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pgpanel=info,tower_http=info,sqlx=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let config = Config::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
    std::fs::create_dir_all(&config.data_dir)?;

    info!(bind = %config.bind_addr, "starting PgPanel");

    let pool = db::connect_and_migrate(&config.database_url).await?;
    let docker = match DockerClient::connect(config.docker_host.as_deref()) {
        Ok(d) => {
            if let Err(e) = d.ping().await {
                warn!(error = %e, "Docker daemon not reachable at startup");
            }
            d
        }
        Err(e) => {
            warn!(error = %e, "Docker client init failed; cluster ops will fail until fixed");
            DockerClient::connect(None).unwrap_or_else(|_| {
                // Last resort: still create struct — operations will error
                DockerClient::connect(Some("unix:///var/run/docker.sock")).expect("docker client")
            })
        }
    };

    let provisioner = ClusterProvisioner::new(docker.clone(), config.clone());
    let backup = build_engine(&config);
    apply_saved_storage(&pool, &backup, &config).await;
    let queue = JobQueue::new(pool.clone());

    let job_ctx = Arc::new(JobContext {
        queue: JobQueue::new(pool.clone()),
        pool: pool.clone(),
        config: config.clone(),
        provisioner: ClusterProvisioner::new(docker, config.clone()),
        backup: backup.clone(),
    });

    // Spawn background worker
    let worker = JobWorker::new(job_ctx.clone());
    tokio::spawn(async move {
        worker.run().await;
    });

    // Periodic metrics + scheduled backups (every 60s check)
    let sched_ctx = job_ctx.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            if let Err(e) = run_backup_scheduler_tick(&sched_ctx).await {
                tracing::warn!(error = %e, "backup scheduler tick");
            }
        }
    });

    let state = AppState {
        pool,
        config: config.clone(),
        queue,
        provisioner,
        backup,
        job_ctx,
    };

    let api = routes::router(state.clone());

    let app = if let Some(static_dir) = &config.static_dir {
        let index = static_dir.join("index.html");
        let spa = ServeDir::new(static_dir).not_found_service(ServeFile::new(index));
        Router::new()
            .merge(api)
            .fallback_service(spa)
            .layer(TraceLayer::new_for_http())
    } else {
        api.layer(TraceLayer::new_for_http())
    };

    let addr: SocketAddr = config
        .bind_addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid bind addr: {e}"))?;

    info!(%addr, "listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// Enqueue due backup schedules (simple daily/cron hour match — MVP).
async fn run_backup_scheduler_tick(ctx: &JobContext) -> anyhow::Result<()> {
    use chrono::Timelike;
    let hour = chrono::Utc::now().hour();
    let configured_hour: u32 = sqlx::query_scalar::<_, String>(
        "SELECT value FROM settings WHERE key = 'backup.schedule_hour'",
    )
    .fetch_optional(&ctx.pool)
    .await?
    .and_then(|value| value.parse().ok())
    .filter(|value: &u32| *value <= 23)
    .unwrap_or(3);
    if hour != configured_hour {
        return Ok(());
    }
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT cluster_id, database_name FROM backup_schedules
        WHERE enabled = 1
          AND (last_run_at IS NULL OR date(last_run_at) < date('now'))
        "#,
    )
    .fetch_all(&ctx.pool)
    .await?;

    for (cluster_id, database) in rows {
        let id = uuid::Uuid::parse_str(&cluster_id)?;
        let _ = ctx
            .queue
            .enqueue(
                pgpanel_core::models::JobType::RunBackup,
                Some(id),
                serde_json::json!({"database": database}),
                Some(&format!(
                    "sched-backup-{cluster_id}-{}",
                    chrono::Utc::now().date_naive()
                )),
            )
            .await;
        let _ = sqlx::query("UPDATE backup_schedules SET last_run_at = ? WHERE cluster_id = ?")
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&cluster_id)
            .execute(&ctx.pool)
            .await;
    }
    Ok(())
}

/// Web-managed storage is persisted encrypted in SQLite. Load it after
/// migrations so a restart does not revert the panel to local disk or require
/// S3 credentials in the process environment.
async fn apply_saved_storage(
    pool: &sqlx::SqlitePool,
    backup: &std::sync::Arc<pgpanel_backup::BackupEngine>,
    config: &Config,
) {
    let storage_type: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'storage.type'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    let root = config
        .backup_data_dir
        .clone()
        .unwrap_or_else(|| config.data_dir.join("backups"));
    if storage_type.as_deref() == Some("local") {
        backup
            .set_storage(std::sync::Arc::new(LocalStorage::new(root)))
            .await;
        return;
    }
    // No DB-managed setting means this is a legacy installation. Keep the
    // environment-backed engine until the operator saves the new web config.
    if storage_type.is_none() {
        return;
    }

    let value = |key: &'static str| async move {
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let access_key = decrypt_secret(
        &config.master_encryption_key,
        &value("storage.access_key").await,
    )
    .ok()
    .map(|v| v.expose_secret().to_string())
    .unwrap_or_default();
    let secret_key = decrypt_secret(
        &config.master_encryption_key,
        &value("storage.secret_key").await,
    )
    .ok()
    .map(|v| v.expose_secret().to_string())
    .unwrap_or_default();
    let endpoint = value("storage.endpoint").await;
    let s3 = S3Storage::from_config(
        root,
        S3StorageConfig {
            endpoint: (!endpoint.is_empty()).then_some(endpoint),
            region: value("storage.region").await,
            bucket: value("storage.bucket").await,
            access_key,
            secret_key,
            prefix: value("storage.prefix").await,
            path_style: value("storage.path_style").await != "false",
            tls_verify: value("storage.tls_verify").await != "false",
        },
    );
    match s3 {
        Ok(storage) => {
            if let Err(error) = storage.test().await {
                tracing::warn!(%error, "saved S3 storage failed its startup check");
            }
            backup.set_storage(std::sync::Arc::new(storage)).await;
        }
        Err(error) => {
            tracing::warn!(%error, "saved storage configuration is invalid; keeping local storage")
        }
    }
}

// Silence unused import if PathBuf not used in some builds
#[allow(dead_code)]
fn _path() -> PathBuf {
    PathBuf::new()
}
