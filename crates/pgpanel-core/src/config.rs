use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

use crate::error::{Error, Result};

/// Allowed PostgreSQL Docker images (no `:latest` in production).
pub const ALLOWED_POSTGRES_IMAGES: &[&str] = &["postgres:16", "postgres:17", "postgres:18"];

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub data_dir: PathBuf,
    pub master_encryption_key: String,
    pub session_ttl_hours: i64,
    pub cookie_secure: bool,
    pub cookie_name: String,
    pub csrf_header: String,
    pub login_max_attempts: u32,
    pub login_lockout_minutes: i64,
    pub cors_origins: Vec<String>,
    pub static_dir: Option<PathBuf>,
    pub databasus_base_url: Option<String>,
    pub databasus_token: Option<String>,
    pub docker_host: Option<String>,
    /// Shared internal network so panel (and Databasus) can reach PG containers by DNS.
    pub management_network: String,
    pub cluster_network_prefix: String,
    pub cluster_volume_prefix: String,
    pub cluster_container_prefix: String,
    /// Deprecated: Databasus removed. Kept for env compatibility.
    pub databasus_http_api: bool,
    /// Backup storage: local | s3 | r2 | b2 | minio | hetzner
    pub backup_storage_type: String,
    pub backup_data_dir: Option<PathBuf>,
    pub backup_retention_days: u32,
    pub backup_keep_count: u32,
    pub backup_encrypt: bool,
    pub default_statement_timeout_ms: u64,
    pub default_lock_timeout_ms: u64,
    pub sql_console_max_rows: usize,
    pub sql_admin_mode_enabled: bool,
    pub bootstrap_token: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let master_key = env::var("PGPANEL_MASTER_KEY").map_err(|_| {
            Error::Internal("PGPANEL_MASTER_KEY is required (32+ byte base64 or hex key)".into())
        })?;

        if master_key.len() < 32 {
            return Err(Error::Internal(
                "PGPANEL_MASTER_KEY must be at least 32 characters".into(),
            ));
        }

        let data_dir = PathBuf::from(
            env::var("PGPANEL_DATA_DIR").unwrap_or_else(|_| "/var/lib/pgpanel".into()),
        );

        let database_url = env::var("PGPANEL_DATABASE_URL").unwrap_or_else(|_| {
            let path = data_dir.join("panel.db");
            format!("sqlite://{}?mode=rwc", path.display())
        });

        Ok(Self {
            bind_addr: env::var("PGPANEL_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            database_url,
            data_dir,
            master_encryption_key: master_key,
            session_ttl_hours: env_parse("PGPANEL_SESSION_TTL_HOURS", 24),
            cookie_secure: env_bool("PGPANEL_COOKIE_SECURE", true),
            cookie_name: env::var("PGPANEL_COOKIE_NAME")
                .unwrap_or_else(|_| "pgpanel_session".into()),
            csrf_header: "x-csrf-token".into(),
            login_max_attempts: env_parse("PGPANEL_LOGIN_MAX_ATTEMPTS", 5),
            login_lockout_minutes: env_parse("PGPANEL_LOGIN_LOCKOUT_MINUTES", 15),
            cors_origins: env::var("PGPANEL_CORS_ORIGINS")
                .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                .unwrap_or_default(),
            static_dir: env::var("PGPANEL_STATIC_DIR").ok().map(PathBuf::from),
            databasus_base_url: env::var("DATABASUS_BASE_URL").ok(),
            databasus_token: env::var("DATABASUS_INTERNAL_TOKEN").ok(),
            docker_host: env::var("DOCKER_HOST").ok(),
            management_network: env::var("PGPANEL_MANAGEMENT_NETWORK")
                .unwrap_or_else(|_| "pgpanel_database_management".into()),
            cluster_network_prefix: env::var("PGPANEL_NETWORK_PREFIX")
                .unwrap_or_else(|_| "pgpanel_net_".into()),
            cluster_volume_prefix: env::var("PGPANEL_VOLUME_PREFIX")
                .unwrap_or_else(|_| "pgpanel_vol_".into()),
            cluster_container_prefix: env::var("PGPANEL_CONTAINER_PREFIX")
                .unwrap_or_else(|_| "pgpanel_pg_".into()),
            databasus_http_api: false,
            backup_storage_type: env::var("BACKUP_STORAGE_TYPE").unwrap_or_else(|_| "local".into()),
            backup_data_dir: env::var("PGPANEL_BACKUP_DIR").ok().map(PathBuf::from),
            backup_retention_days: env_parse("BACKUP_RETENTION_DAYS", 14),
            backup_keep_count: env_parse("BACKUP_MAX_COUNT", 30),
            backup_encrypt: env_bool("BACKUP_ENCRYPT", true),
            default_statement_timeout_ms: env_parse("PGPANEL_STATEMENT_TIMEOUT_MS", 15_000),
            default_lock_timeout_ms: env_parse("PGPANEL_LOCK_TIMEOUT_MS", 3_000),
            sql_console_max_rows: env_parse("PGPANEL_SQL_MAX_ROWS", 1000),
            sql_admin_mode_enabled: env_bool("PGPANEL_SQL_ADMIN_MODE", false),
            bootstrap_token: env::var("PGPANEL_BOOTSTRAP_TOKEN").ok(),
        })
    }

    pub fn postgres_image(version: &str) -> Result<&'static str> {
        let image = match version {
            "16" | "postgres:16" => "postgres:16",
            "17" | "postgres:17" => "postgres:17",
            "18" | "postgres:18" => "postgres:18",
            other => {
                return Err(Error::Validation(format!(
                    "unsupported PostgreSQL version '{other}'; allowed: 16, 17, 18"
                )));
            }
        };
        Ok(image)
    }
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(v) => matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicConfig {
    pub sql_admin_mode_enabled: bool,
    pub sql_console_max_rows: usize,
    pub allowed_postgres_versions: Vec<String>,
}
