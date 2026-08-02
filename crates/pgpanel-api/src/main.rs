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
use pgpanel_docker::{ClusterProvisioner, DockerClient, NodeRegistry};
use pgpanel_jobs::{JobContext, JobQueue, JobWorker};
use secrecy::ExposeSecret;

use crate::routes::waf::load_active_waf;
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
    let nodes = NodeRegistry::new(docker.clone());
    let backup = build_engine(&config);
    apply_saved_storage(&pool, &backup, &config).await;
    let waf_cfg = load_active_waf(&pool).await;
    let queue = JobQueue::new(pool.clone());

    let job_ctx = Arc::new(JobContext {
        queue: JobQueue::new(pool.clone()),
        pool: pool.clone(),
        config: config.clone(),
        provisioner: ClusterProvisioner::new(docker, config.clone()),
        backup: backup.clone(),
        nodes: nodes.clone(),
    });

    // Spawn background worker
    let worker = JobWorker::new(job_ctx.clone());
    tokio::spawn(async move {
        worker.run().await;
    });

    // Periodic metrics + scheduled backups + retention prune (every 60s check)
    let sched_ctx = job_ctx.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            if let Err(e) = run_backup_scheduler_tick(&sched_ctx).await {
                tracing::warn!(error = %e, "backup scheduler tick");
            }
            if let Err(e) = run_retention_tick(&sched_ctx).await {
                tracing::warn!(error = %e, "backup retention tick");
            }
            if let Err(e) = run_multi_destination_backup_tick(&sched_ctx).await {
                tracing::warn!(error = %e, "multi-destination backup tick");
            }
            if let Err(e) = run_replica_sync_tick(&sched_ctx).await {
                tracing::warn!(error = %e, "replica sync tick");
            }
            if let Err(e) = run_wal_sync_tick(&sched_ctx).await {
                tracing::warn!(error = %e, "WAL sync tick");
            }
            if let Err(e) = run_alert_tick(&sched_ctx).await {
                tracing::warn!(error = %e, "alert tick");
            }
            if let Err(e) = run_metrics_tick(&sched_ctx).await {
                tracing::warn!(error = %e, "metrics tick");
            }
        }
    });

    let state = AppState {
        pool,
        config: config.clone(),
        queue,
        provisioner,
        backup,
        nodes,
        waf: Arc::new(tokio::sync::RwLock::new(waf_cfg)),
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

/// Enqueue due backup schedules. Supports full 5-field cron (`M H Dom Mon Dow`)
/// with `*` wildcards, plus legacy hour-of-day matching.
async fn run_backup_scheduler_tick(ctx: &JobContext) -> anyhow::Result<()> {
    use chrono::{Datelike, Timelike};
    let now = chrono::Utc::now();
    let rows: Vec<(String, String, String, i64, Option<i64>, Option<i64>, Option<String>)> =
        sqlx::query_as(
            r#"
        SELECT cluster_id, database_name, cron, schema_only, window_start_hour, window_end_hour, pause_until
        FROM backup_schedules
        WHERE enabled = 1
          AND (last_run_at IS NULL OR datetime(last_run_at) < datetime('now', '-50 minutes'))
        "#,
        )
        .fetch_all(&ctx.pool)
        .await?;

    for (cluster_id, database, cron, schema_only, win_start, win_end, pause_until) in rows {
        if let Some(pause) = pause_until {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&pause) {
                if dt > now {
                    continue;
                }
            }
        }
        if let (Some(start), Some(end)) = (win_start, win_end) {
            let h = now.hour() as i64;
            if start <= end {
                if h < start || h > end {
                    continue;
                }
            } else if h < start && h > end {
                continue;
            }
        }
        if !cron_matches(&cron, now) {
            continue;
        }
        let id = uuid::Uuid::parse_str(&cluster_id)?;
        let _ = ctx
            .queue
            .enqueue(
                pgpanel_core::models::JobType::RunBackup,
                Some(id),
                serde_json::json!({
                    "database": database,
                    "schema_only": schema_only != 0,
                }),
                Some(&format!(
                    "sched-backup-{cluster_id}-{}-{}",
                    now.date_naive(),
                    now.hour()
                )),
            )
            .await;
        let _ = sqlx::query(
            "UPDATE backup_schedules SET last_run_at = ?, next_run_at = NULL WHERE cluster_id = ? AND database_name = ?",
        )
        .bind(now.to_rfc3339())
        .bind(&cluster_id)
        .bind(&database)
        .execute(&ctx.pool)
        .await;
        let _ = now.weekday(); // silence unused when Datelike imported for cron
    }
    Ok(())
}

fn cron_matches(cron: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
    use chrono::Timelike;
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() < 2 {
        return false;
    }
    // minute hour [day-of-month month day-of-week]
    let minute = now.minute();
    let hour = now.hour();
    fn field_match(field: &str, value: u32) -> bool {
        if field == "*" {
            return true;
        }
        if let Ok(n) = field.parse::<u32>() {
            return n == value;
        }
        if let Some((a, b)) = field.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                return value >= a && value <= b;
            }
        }
        if let Some((base, step)) = field.split_once('/') {
            if let Ok(step) = step.parse::<u32>() {
                if step == 0 {
                    return false;
                }
                let start = if base == "*" {
                    0
                } else {
                    base.parse().unwrap_or(0)
                };
                return value >= start && (value - start) % step == 0;
            }
        }
        field
            .split(',')
            .any(|p| p.parse::<u32>().ok() == Some(value))
    }
    if !field_match(parts[0], minute) || !field_match(parts[1], hour) {
        return false;
    }
    if parts.len() >= 5 {
        use chrono::{Datelike, Weekday};
        let dom = now.day();
        let month = now.month();
        let dow = match now.weekday() {
            Weekday::Sun => 0,
            Weekday::Mon => 1,
            Weekday::Tue => 2,
            Weekday::Wed => 3,
            Weekday::Thu => 4,
            Weekday::Fri => 5,
            Weekday::Sat => 6,
        };
        if !field_match(parts[2], dom)
            || !field_match(parts[3], month)
            || !field_match(parts[4], dow)
        {
            return false;
        }
    }
    true
}

async fn run_retention_tick(ctx: &JobContext) -> anyhow::Result<()> {
    // Run prune at most once per hour globally
    let marker: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'backup.last_prune_at'")
            .fetch_optional(&ctx.pool)
            .await?;
    if let Some(m) = marker {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&m) {
            if chrono::Utc::now() - dt.with_timezone(&chrono::Utc) < chrono::Duration::hours(1) {
                return Ok(());
            }
        }
    }
    let _ = ctx
        .queue
        .enqueue(
            pgpanel_core::models::JobType::PruneBackups,
            None,
            serde_json::json!({}),
            Some(&format!("prune-{}", chrono::Utc::now().date_naive())),
        )
        .await;
    let now = chrono::Utc::now().to_rfc3339();
    let _ = sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES ('backup.last_prune_at', ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(&now)
    .bind(&now)
    .execute(&ctx.pool)
    .await;
    Ok(())
}

async fn run_multi_destination_backup_tick(ctx: &JobContext) -> anyhow::Result<()> {
    let now = chrono::Utc::now();
    let rows: Vec<(String, String, String, String, i64)> = sqlx::query_as(
        r#"
        SELECT id, cluster_id, destination_id, cron, schema_only
        FROM cluster_backup_targets
        WHERE enabled = 1
          AND (last_run_at IS NULL OR datetime(last_run_at) < datetime('now', '-45 minutes'))
        "#,
    )
    .fetch_all(&ctx.pool)
    .await
    .unwrap_or_default();

    for (tid, cluster_id, destination_id, cron, schema_only) in rows {
        if !cron_matches(&cron, now) {
            continue;
        }
        let cid = uuid::Uuid::parse_str(&cluster_id)?;
        let _ = ctx
            .queue
            .enqueue(
                pgpanel_core::models::JobType::RunBackup,
                Some(cid),
                serde_json::json!({
                    "database": "postgres",
                    "schema_only": schema_only != 0,
                    "destination_id": destination_id,
                    "target_id": tid,
                }),
                Some(&format!("dest-backup-{tid}-{}", now.format("%Y%m%d%H"))),
            )
            .await;
        let _ = sqlx::query(
            "UPDATE cluster_backup_targets SET last_run_at = ?, last_status = 'queued' WHERE id = ?",
        )
        .bind(now.to_rfc3339())
        .bind(&tid)
        .execute(&ctx.pool)
        .await;
    }
    Ok(())
}

async fn run_replica_sync_tick(ctx: &JobContext) -> anyhow::Result<()> {
    let now = chrono::Utc::now();
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        r#"
        SELECT id, sync_cron, primary_cluster_id FROM cluster_replicas
        WHERE enabled = 1 AND status IN ('healthy', 'lagging', 'pending')
          AND (last_sync_at IS NULL OR datetime(last_sync_at) < datetime('now', '-20 minutes'))
        "#,
    )
    .fetch_all(&ctx.pool)
    .await
    .unwrap_or_default();

    for (id, cron, primary) in rows {
        if !cron_matches(&cron, now) {
            continue;
        }
        let Some(primary) = primary else { continue };
        let pid = uuid::Uuid::parse_str(&primary)?;
        let _ = ctx
            .queue
            .enqueue(
                pgpanel_core::models::JobType::SyncReplica,
                Some(pid),
                serde_json::json!({"replica_id": id}),
                Some(&format!("replica-sync-{id}-{}", now.format("%Y%m%d%H%M"))),
            )
            .await;
    }
    Ok(())
}

async fn run_wal_sync_tick(ctx: &JobContext) -> anyhow::Result<()> {
    let now = chrono::Utc::now();
    let streams: Vec<(String, i64)> = sqlx::query_as(
        r#"
        SELECT cluster_id, retention_days
        FROM wal_streams
        WHERE enabled = 1
          AND (last_synced_at IS NULL OR datetime(last_synced_at) < datetime('now', '-2 minutes'))
        "#,
    )
    .fetch_all(&ctx.pool)
    .await
    .unwrap_or_default();

    for (cluster_id, retention_days) in streams {
        if let Ok(cluster_uuid) = uuid::Uuid::parse_str(&cluster_id) {
            let _ = ctx
                .queue
                .enqueue(
                    pgpanel_core::models::JobType::SyncWal,
                    Some(cluster_uuid),
                    serde_json::json!({}),
                    Some(&format!(
                        "wal-sync-{cluster_id}-{}",
                        now.format("%Y%m%d%H%M")
                    )),
                )
                .await;
        }

        let cutoff = format!("-{} days", retention_days.clamp(1, 3650));
        let expired: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, storage_key FROM wal_segments WHERE cluster_id = ? AND COALESCE(archived_at, synced_at) < datetime('now', ?)",
        )
        .bind(&cluster_id)
        .bind(&cutoff)
        .fetch_all(&ctx.pool)
        .await
        .unwrap_or_default();
        for (segment_id, storage_key) in expired {
            let _ = ctx.backup.delete_object(&storage_key).await;
            let _ = sqlx::query("DELETE FROM wal_segments WHERE id = ?")
                .bind(segment_id)
                .execute(&ctx.pool)
                .await;
        }
        let _ = sqlx::query(
            "UPDATE wal_streams SET segment_count = (SELECT COUNT(*) FROM wal_segments WHERE cluster_id = ?), total_bytes = (SELECT COALESCE(SUM(size_bytes), 0) FROM wal_segments WHERE cluster_id = ?), updated_at = ? WHERE cluster_id = ?",
        )
        .bind(&cluster_id)
        .bind(&cluster_id)
        .bind(now.to_rfc3339())
        .bind(&cluster_id)
        .execute(&ctx.pool)
        .await;
    }
    Ok(())
}

async fn run_alert_tick(ctx: &JobContext) -> anyhow::Result<()> {
    let _ = ctx
        .queue
        .enqueue(
            pgpanel_core::models::JobType::EvaluateAlerts,
            None,
            serde_json::json!({}),
            Some(&format!(
                "alerts-{}",
                chrono::Utc::now().format("%Y%m%d%H%M")
            )),
        )
        .await;
    Ok(())
}

async fn run_metrics_tick(ctx: &JobContext) -> anyhow::Result<()> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM clusters WHERE status IN ('healthy', 'healthy_with_backup_warning', 'degraded', 'starting')",
    )
    .fetch_all(&ctx.pool)
    .await
    .unwrap_or_default();
    let minute = chrono::Utc::now().format("%Y%m%d%H%M");
    for id in ids {
        let Ok(uuid) = uuid::Uuid::parse_str(&id) else {
            continue;
        };
        let _ = ctx
            .queue
            .enqueue(
                pgpanel_core::models::JobType::RefreshMetrics,
                Some(uuid),
                serde_json::json!({}),
                Some(&format!("metrics-{id}-{minute}")),
            )
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
