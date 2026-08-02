use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::str::FromStr;
use tracing::info;

pub async fn connect_and_migrate(database_url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(options)
        .await?;

    // Run migrations from ./migrations relative to CWD or embedded
    let migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations")).await;
    match migrator {
        Ok(m) => {
            m.run(&pool).await?;
            info!("migrations applied");
        }
        Err(_) => {
            // Fallback: embed critical schema
            info!("migrations dir not found; applying embedded schema");
            sqlx::raw_sql(include_str!("../../../migrations/001_initial.sql"))
                .execute(&pool)
                .await?;
            let _ = sqlx::raw_sql(include_str!("../../../migrations/002_native_backup.sql"))
                .execute(&pool)
                .await;
            let _ = sqlx::raw_sql(include_str!("../../../migrations/003_admin_email.sql"))
                .execute(&pool)
                .await;
            let _ = sqlx::raw_sql(include_str!("../../../migrations/007_wal_streaming.sql"))
                .execute(&pool)
                .await;
        }
    }

    Ok(pool)
}
