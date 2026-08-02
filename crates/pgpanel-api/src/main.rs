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

use pgpanel_backup::build_engine;
use pgpanel_core::config::Config;
use pgpanel_docker::{ClusterProvisioner, DockerClient};
use pgpanel_jobs::{JobContext, JobQueue, JobWorker};

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
    // Default cron `0 3 * * *` → run around 03:00 UTC
    if hour != 3 {
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
                Some(&format!("sched-backup-{cluster_id}-{}", chrono::Utc::now().date_naive())),
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

// Silence unused import if PathBuf not used in some builds
#[allow(dead_code)]
fn _path() -> PathBuf {
    PathBuf::new()
}
