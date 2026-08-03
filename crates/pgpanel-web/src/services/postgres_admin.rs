//! PostgreSQL administration via tokio-postgres.

use crate::crypto;
use async_trait::async_trait;
use pgpanel_core::auth::SecretKey;
use pgpanel_core::config::Config;
use pgpanel_core::pg::quote_ident;
use pgpanel_core::CoreError;
use pgpanel_core::CoreResult;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio_postgres::{Client, NoTls, Row};

/// Cluster connection credentials.
#[derive(Debug, Clone)]
pub struct ClusterCredentials {
    pub version: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub database: String,
    pub connection_mode: String,
    pub password: Option<String>,
    pub socket_dir: Option<String>,
}

/// Database info row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DatabaseInfo {
    pub name: String,
    pub owner: String,
    pub encoding: String,
    pub size_bytes: i64,
}

/// Role info row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RoleInfo {
    pub name: String,
    pub is_superuser: bool,
    pub can_login: bool,
    pub can_create_db: bool,
    pub can_create_role: bool,
}

/// SQL query result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub row_count: usize,
    pub duration_ms: u64,
    pub truncated: bool,
}

/// Trait for testability.
#[async_trait]
pub trait PostgresConnector: Send + Sync {
    async fn connect(&self, creds: &ClusterCredentials) -> CoreResult<Client>;
}

/// Default tokio-postgres connector.
#[derive(Debug, Default)]
pub struct TokioPostgresConnector;

#[async_trait]
impl PostgresConnector for TokioPostgresConnector {
    async fn connect(&self, creds: &ClusterCredentials) -> CoreResult<Client> {
        connect_client(creds).await
    }
}

/// PostgreSQL admin operations.
#[derive(Clone)]
pub struct PostgresAdmin {
    db: SqlitePool,
    secret_key: Arc<SecretKey>,
    config: Arc<Config>,
    connector: Arc<dyn PostgresConnector>,
}

impl PostgresAdmin {
    pub fn new(db: SqlitePool, secret_key: Arc<SecretKey>, config: Arc<Config>) -> Self {
        Self {
            db,
            secret_key,
            config,
            connector: Arc::new(TokioPostgresConnector),
        }
    }

    #[cfg(test)]
    pub fn with_connector(
        db: SqlitePool,
        secret_key: Arc<SecretKey>,
        config: Arc<Config>,
        connector: Arc<dyn PostgresConnector>,
    ) -> Self {
        Self {
            db,
            secret_key,
            config,
            connector,
        }
    }

    /// Store encrypted cluster credentials.
    #[allow(clippy::too_many_arguments)]
    pub async fn save_credentials(
        &self,
        version: &str,
        name: &str,
        host: &str,
        port: u16,
        username: &str,
        database: &str,
        connection_mode: &str,
        password: Option<&str>,
        socket_dir: Option<&str>,
    ) -> CoreResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let enc_password = match password {
            Some(p) if !p.is_empty() => Some(crypto::encrypt(&self.secret_key, p)?),
            _ => None,
        };

        sqlx::query(
            "INSERT INTO cluster_credentials (version, name, host, port, username, database, connection_mode, password_encrypted, socket_dir, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(version, name) DO UPDATE SET
               host = excluded.host, port = excluded.port, username = excluded.username,
               database = excluded.database, connection_mode = excluded.connection_mode,
               password_encrypted = excluded.password_encrypted, socket_dir = excluded.socket_dir,
               updated_at = excluded.updated_at",
        )
        .bind(version)
        .bind(name)
        .bind(host)
        .bind(port as i64)
        .bind(username)
        .bind(database)
        .bind(connection_mode)
        .bind(enc_password)
        .bind(socket_dir)
        .bind(&now)
        .bind(&now)
        .execute(&self.db)
        .await
        .map_err(|e| CoreError::Internal(format!("database error: {e}")))?;

        Ok(())
    }

    /// Load credentials for a cluster.
    pub async fn get_credentials(
        &self,
        version: &str,
        name: &str,
    ) -> CoreResult<Option<ClusterCredentials>> {
        let row: Option<CredRow> = sqlx::query_as(
            "SELECT version, name, host, port, username, database, connection_mode, password_encrypted, socket_dir
             FROM cluster_credentials WHERE version = ? AND name = ?",
        )
        .bind(version)
        .bind(name)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| CoreError::Internal(format!("database error: {e}")))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let password = match row.password_encrypted {
            Some(enc) => Some(crypto::decrypt(&self.secret_key, &enc)?),
            None => None,
        };

        Ok(Some(ClusterCredentials {
            version: row.version,
            name: row.name,
            host: row.host,
            port: row.port as u16,
            username: row.username,
            database: row.database,
            connection_mode: row.connection_mode,
            password,
            socket_dir: row.socket_dir,
        }))
    }

    /// Connect to a cluster.
    pub async fn connect(&self, version: &str, name: &str, port: u16) -> CoreResult<Client> {
        let creds = self
            .get_credentials(version, name)
            .await?
            .unwrap_or_else(|| default_credentials(version, name, port));

        self.connector.connect(&creds).await
    }

    /// List databases.
    pub async fn list_databases(
        &self,
        version: &str,
        name: &str,
        port: u16,
    ) -> CoreResult<Vec<DatabaseInfo>> {
        let client = self.connect(version, name, port).await?;
        let rows = client
            .query(
                "SELECT d.datname, pg_catalog.pg_get_userbyid(d.datdba) AS owner,
                        pg_catalog.pg_encoding_to_char(d.encoding) AS encoding,
                        pg_catalog.pg_database_size(d.datname) AS size
                 FROM pg_catalog.pg_database d
                 WHERE d.datistemplate = false
                 ORDER BY d.datname",
                &[],
            )
            .await
            .map_err(|e| CoreError::Internal(format!("postgres query: {e}")))?;

        Ok(rows
            .iter()
            .map(|r| DatabaseInfo {
                name: r.get(0),
                owner: r.get(1),
                encoding: r.get(2),
                size_bytes: r.get(3),
            })
            .collect())
    }

    /// Create database.
    pub async fn create_database(
        &self,
        version: &str,
        cluster: &str,
        port: u16,
        db_name: &str,
        owner: &str,
    ) -> CoreResult<()> {
        pgpanel_core::validation::validate_pg_identifier(db_name)?;
        pgpanel_core::validation::validate_pg_identifier(owner)?;
        let client = self.connect(version, cluster, port).await?;
        let sql = format!(
            "CREATE DATABASE {} OWNER {}",
            quote_ident(db_name)?,
            quote_ident(owner)?
        );
        client
            .batch_execute(&sql)
            .await
            .map_err(|e| CoreError::Internal(format!("postgres: {e}")))?;
        Ok(())
    }

    /// Drop database.
    pub async fn drop_database(
        &self,
        version: &str,
        cluster: &str,
        port: u16,
        db_name: &str,
    ) -> CoreResult<()> {
        pgpanel_core::validation::validate_pg_identifier(db_name)?;
        let client = self.connect(version, cluster, port).await?;
        let sql = format!("DROP DATABASE {}", quote_ident(db_name)?);
        client
            .batch_execute(&sql)
            .await
            .map_err(|e| CoreError::Internal(format!("postgres: {e}")))?;
        Ok(())
    }

    /// List roles.
    pub async fn list_roles(
        &self,
        version: &str,
        name: &str,
        port: u16,
    ) -> CoreResult<Vec<RoleInfo>> {
        let client = self.connect(version, name, port).await?;
        let rows = client
            .query(
                "SELECT rolname, rolsuper, rolcanlogin, rolcreatedb, rolcreaterole
                 FROM pg_roles ORDER BY rolname",
                &[],
            )
            .await
            .map_err(|e| CoreError::Internal(format!("postgres query: {e}")))?;

        Ok(rows
            .iter()
            .map(|r| RoleInfo {
                name: r.get(0),
                is_superuser: r.get(1),
                can_login: r.get(2),
                can_create_db: r.get(3),
                can_create_role: r.get(4),
            })
            .collect())
    }

    /// Create role.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_role(
        &self,
        version: &str,
        cluster: &str,
        port: u16,
        role_name: &str,
        password: Option<&str>,
        can_login: bool,
        can_create_db: bool,
        is_superuser: bool,
    ) -> CoreResult<()> {
        if is_superuser && !self.config.app.allow_superuser_creation {
            return Err(CoreError::Forbidden(
                "superuser creation is disabled".into(),
            ));
        }
        pgpanel_core::validation::validate_pg_identifier(role_name)?;
        let client = self.connect(version, cluster, port).await?;
        let mut parts = vec![format!("CREATE ROLE {}", quote_ident(role_name)?)];
        if can_login {
            parts.push("LOGIN".into());
        } else {
            parts.push("NOLOGIN".into());
        }
        if can_create_db {
            parts.push("CREATEDB".into());
        }
        if is_superuser {
            parts.push("SUPERUSER".into());
        }
        if let Some(pw) = password {
            parts.push(format!("PASSWORD '{}'", pw.replace('\'', "''")));
        }
        client
            .batch_execute(&parts.join(" "))
            .await
            .map_err(|e| CoreError::Internal(format!("postgres: {e}")))?;
        Ok(())
    }

    /// Execute SQL query with limits.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_query(
        &self,
        version: &str,
        cluster: &str,
        port: u16,
        database: &str,
        query: &str,
        max_rows: u32,
        timeout_ms: u64,
        read_only: bool,
    ) -> CoreResult<QueryResult> {
        let mut creds = self
            .get_credentials(version, cluster)
            .await?
            .unwrap_or_else(|| default_credentials(version, cluster, port));
        creds.database = database.to_string();

        let start = std::time::Instant::now();
        let client = self.connector.connect(&creds).await?;

        if read_only {
            client
                .batch_execute("SET default_transaction_read_only = on")
                .await
                .map_err(|e| CoreError::Internal(format!("postgres: {e}")))?;
        }

        let stmt = format!("SET statement_timeout = '{}ms'", timeout_ms);
        client
            .batch_execute(&stmt)
            .await
            .map_err(|e| CoreError::Internal(format!("postgres: {e}")))?;

        let rows: Vec<Row> = client
            .query(query, &[])
            .await
            .map_err(|e| CoreError::Internal(format!("postgres query: {e}")))?;
        let duration_ms = start.elapsed().as_millis() as u64;

        let truncated = rows.len() > max_rows as usize;
        let rows: Vec<Row> = rows.into_iter().take(max_rows as usize).collect();

        let columns: Vec<String> = if let Some(first) = rows.first() {
            first
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect()
        } else {
            vec![]
        };

        let data: Vec<Vec<String>> = rows
            .iter()
            .map(|row| {
                (0..row.len())
                    .map(|i| {
                        row.try_get::<_, Option<String>>(i)
                            .ok()
                            .flatten()
                            .unwrap_or_else(|| "NULL".into())
                    })
                    .collect()
            })
            .collect();

        Ok(QueryResult {
            columns,
            rows: data,
            row_count: rows.len(),
            duration_ms,
            truncated,
        })
    }
}

#[derive(sqlx::FromRow)]
struct CredRow {
    version: String,
    name: String,
    host: String,
    port: i64,
    username: String,
    database: String,
    connection_mode: String,
    password_encrypted: Option<String>,
    socket_dir: Option<String>,
}

fn default_credentials(version: &str, name: &str, port: u16) -> ClusterCredentials {
    ClusterCredentials {
        version: version.to_string(),
        name: name.to_string(),
        host: "127.0.0.1".into(),
        port,
        username: "postgres".into(),
        database: "postgres".into(),
        connection_mode: "socket".into(),
        password: None,
        socket_dir: Some("/var/run/postgresql".into()),
    }
}

async fn connect_client(creds: &ClusterCredentials) -> CoreResult<Client> {
    let conn_str = if creds.connection_mode == "socket" {
        let dir = creds.socket_dir.as_deref().unwrap_or("/var/run/postgresql");
        format!(
            "host={dir} port={} user={} dbname={}",
            creds.port, creds.username, creds.database
        )
    } else {
        let mut s = format!(
            "host={} port={} user={} dbname={}",
            creds.host, creds.port, creds.username, creds.database
        );
        if let Some(pw) = &creds.password {
            s.push_str(&format!(" password={}", pw.replace('\'', "''")));
        }
        s
    };

    let (client, connection) = tokio_postgres::connect(&conn_str, NoTls)
        .await
        .map_err(|e| CoreError::Internal(format!("postgres connect: {e}")))?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::warn!(error = %e, "postgres connection error");
        }
    });

    Ok(client)
}
