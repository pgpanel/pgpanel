//! SQLite database pool and migrations.

use pgpanel_core::config::Config;
use sqlx::{
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions},
};
use std::str::FromStr;
use tracing::info;

static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// Run migrations and create a connection pool.
pub async fn init_pool(config: &Config) -> Result<SqlitePool, sqlx::Error> {
    if let Some(parent) = config.paths.sqlite.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let opts = SqliteConnectOptions::from_str(&format!(
        "sqlite:{}?mode=rwc",
        config.paths.sqlite.display()
    ))?
    .foreign_keys(true)
    .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(opts)
        .await?;

    run_migrations(&pool).await?;
    Ok(pool)
}

/// Run migrations embedded in the release binary at build time.
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    info!("running embedded database migrations");
    MIGRATOR.run(pool).await?;
    Ok(())
}

/// Create an in-memory pool for tests.
#[cfg(test)]
pub async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .expect("memory pool");
    run_migrations(&pool).await.expect("migrations");
    pool
}
