use secrecy::{ExposeSecret, SecretString};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgPool, Row};
use tracing::debug;

use pgpanel_core::error::{Error, Result};

/// Admin connection to a managed PostgreSQL cluster.
#[derive(Clone)]
pub struct PgClient {
    pool: PgPool,
    host: String,
    port: u16,
}

impl PgClient {
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        password: &SecretString,
        database: &str,
        statement_timeout_ms: u64,
        lock_timeout_ms: u64,
    ) -> Result<Self> {
        let opts = PgConnectOptions::new()
            .host(host)
            .port(port)
            .username(user)
            .password(password.expose_secret())
            .database(database)
            .application_name("pgpanel");

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect_with(opts)
            .await
            .map_err(|e| Error::Postgres(format!("connect {host}:{port}/{database}: {e}")))?;

        sqlx::query(&format!(
            "SET statement_timeout = '{statement_timeout_ms}ms'"
        ))
        .execute(&pool)
        .await
        .map_err(|e| Error::Postgres(format!("set statement_timeout: {e}")))?;

        sqlx::query(&format!("SET lock_timeout = '{lock_timeout_ms}ms'"))
            .execute(&pool)
            .await
            .map_err(|e| Error::Postgres(format!("set lock_timeout: {e}")))?;

        debug!(%host, %port, %database, "connected to PostgreSQL");

        Ok(Self {
            pool,
            host: host.to_string(),
            port,
        })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn is_ready(&self) -> Result<bool> {
        let row = sqlx::query("SELECT 1 AS ok")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Error::Postgres(format!("ready check: {e}")))?;
        let ok: i32 = row.try_get("ok").unwrap_or(0);
        Ok(ok == 1)
    }

    pub async fn active_connections(&self) -> Result<i64> {
        let row = sqlx::query(
            "SELECT count(*)::bigint AS c FROM pg_stat_activity WHERE datname IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Postgres(format!("active connections: {e}")))?;
        Ok(row.try_get("c").unwrap_or(0))
    }

    pub async fn database_sizes(&self) -> Result<Vec<(String, i64)>> {
        let rows = sqlx::query(
            r#"
            SELECT datname AS name, pg_database_size(datname)::bigint AS size
            FROM pg_database
            WHERE datistemplate = false
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Postgres(format!("database sizes: {e}")))?;

        let mut out = Vec::new();
        for row in rows {
            let name: String = row.try_get("name").unwrap_or_default();
            let size: i64 = row.try_get("size").unwrap_or(0);
            out.push((name, size));
        }
        Ok(out)
    }
}
