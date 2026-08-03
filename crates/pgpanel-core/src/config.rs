//! Application configuration loaded from TOML + environment overrides.

use crate::error::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Application section.
    pub app: AppConfig,
    /// HTTP server.
    pub server: ServerConfig,
    /// Paths.
    pub paths: PathsConfig,
    /// Session / cookie settings.
    pub session: SessionConfig,
    /// Authentication.
    pub auth: AuthConfig,
    /// Trusted proxies (Cloudflare Tunnel, etc.).
    #[serde(default)]
    pub trusted_proxies: TrustedProxiesConfig,
    /// Rate limiting.
    pub rate_limit: RateLimitConfig,
    /// PostgreSQL management policy.
    pub postgres: PostgresConfig,
    /// Databasus integration.
    #[serde(default)]
    pub databasus: DatabasusConfig,
    /// Monitoring retention.
    pub monitoring: MonitoringConfig,
    /// Updates.
    pub updates: UpdatesConfig,
    /// Caddy blue-green control.
    pub caddy: CaddyConfig,
    /// Operation timeouts (seconds).
    pub timeouts: TimeoutsConfig,
    /// Logging.
    pub logging: LoggingConfig,
}

/// Application identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Secret key (>= 32 chars). Prefer env `PGPANEL_SECRET_KEY`.
    pub secret_key: String,
    /// Active deployment slot: blue or green.
    pub active_slot: String,
    /// Slot name for this process instance.
    pub slot: String,
    /// Allow creating PostgreSQL superusers (default false).
    #[serde(default)]
    pub allow_superuser_creation: bool,
}

/// HTTP listen configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Bind address (default 127.0.0.1).
    pub listen: SocketAddr,
    /// Public base URL if known (for links).
    #[serde(default)]
    pub public_base_url: Option<String>,
}

/// Filesystem paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    /// SQLite database path.
    pub sqlite: PathBuf,
    /// Helper Unix socket.
    pub helper_socket: PathBuf,
    /// State directory.
    pub state_dir: PathBuf,
    /// Config directory.
    pub config_dir: PathBuf,
    /// Signing public key path.
    pub signing_public_key: PathBuf,
    /// Privileged updater Unix socket.
    #[serde(default = "default_updater_socket")]
    pub updater_socket: PathBuf,
}

fn default_updater_socket() -> PathBuf {
    PathBuf::from("/run/pgpanel/updater.sock")
}

/// Session cookies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Cookie name.
    pub cookie_name: String,
    /// TTL seconds.
    pub ttl_secs: u64,
    /// Absolute max session lifetime seconds.
    pub absolute_ttl_secs: u64,
    /// SameSite: Lax or Strict.
    pub same_site: String,
    /// Secure flag (true behind HTTPS / tunnel).
    pub secure: bool,
    /// Recent-auth window for destructive ops (seconds).
    pub reauth_window_secs: u64,
}

/// Auth settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// TOTP issuer label.
    pub totp_issuer: String,
    /// Max failed logins before lockout.
    pub max_failed_logins: u32,
    /// Lockout duration seconds.
    pub lockout_secs: u64,
    /// Require TOTP for destructive ops when user has TOTP enabled.
    #[serde(default = "default_true")]
    pub require_totp_for_destructive: bool,
}

fn default_true() -> bool {
    true
}

/// Trusted proxy / Cloudflare Tunnel.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrustedProxiesConfig {
    /// Enable trusting X-Forwarded-For / CF-Connecting-IP.
    #[serde(default)]
    pub enabled: bool,
    /// Allowed proxy CIDRs or exact IPs (e.g. 127.0.0.1).
    #[serde(default)]
    pub proxies: Vec<String>,
    /// Prefer CF-Connecting-IP when present.
    #[serde(default = "default_true")]
    pub prefer_cf_connecting_ip: bool,
}

/// Rate limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Login attempts per IP per window.
    pub login_per_ip: u32,
    /// Login window seconds.
    pub login_window_secs: u64,
    /// Global API requests per IP per minute.
    pub api_per_ip_per_minute: u32,
}

/// PostgreSQL policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    /// Allowed major versions.
    pub allowed_versions: Vec<String>,
    /// Allowlisted data directory roots.
    pub data_directory_roots: Vec<PathBuf>,
    /// Default statement timeout for SQL editor (ms).
    pub default_statement_timeout_ms: u64,
    /// Default max rows for SQL editor.
    pub default_max_rows: u32,
    /// Absolute executable paths.
    pub binaries: PostgresBinaries,
}

/// Absolute paths to postgresql-common tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresBinaries {
    /// pg_lsclusters
    pub pg_lsclusters: PathBuf,
    /// pg_createcluster
    pub pg_createcluster: PathBuf,
    /// pg_ctlcluster
    pub pg_ctlcluster: PathBuf,
    /// pg_renamecluster
    pub pg_renamecluster: PathBuf,
    /// pg_dropcluster
    pub pg_dropcluster: PathBuf,
    /// pg_conftool (optional validation aid)
    pub pg_conftool: PathBuf,
}

/// Databasus optional integration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabasusConfig {
    /// Enable integration.
    #[serde(default)]
    pub enabled: bool,
    /// Base URL.
    #[serde(default)]
    pub base_url: Option<String>,
    /// API key (prefer env PGPANEL_DATABASUS_API_KEY).
    #[serde(default)]
    pub api_key: Option<String>,
    /// Timeout seconds.
    #[serde(default = "default_databasus_timeout")]
    pub timeout_secs: u64,
    /// Verify TLS.
    #[serde(default = "default_true")]
    pub tls_verify: bool,
}

fn default_databasus_timeout() -> u64 {
    10
}

/// Monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Sample retention hours.
    pub retention_hours: u64,
    /// Sample interval seconds.
    pub sample_interval_secs: u64,
}

/// Update settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatesConfig {
    /// GitHub owner.
    pub github_owner: String,
    /// GitHub repository.
    pub github_repo: String,
    /// Channel: stable or prerelease.
    pub channel: String,
    /// Check interval seconds.
    pub check_interval_secs: u64,
    /// Automatic installation disabled by default.
    #[serde(default)]
    pub auto_install: bool,
    /// Optional cron-like window description.
    #[serde(default)]
    pub schedule_window: Option<String>,
    /// Update timeout seconds.
    pub timeout_secs: u64,
    /// Max download bytes.
    pub max_download_bytes: u64,
    /// Drain period after switch seconds.
    pub drain_secs: u64,
    /// Install root (/opt/pgpanel).
    pub install_root: PathBuf,
}

/// Caddy control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaddyConfig {
    /// Path to Caddyfile snippet that selects upstream.
    pub upstream_config_path: PathBuf,
    /// Blue upstream.
    pub blue_upstream: String,
    /// Green upstream.
    pub green_upstream: String,
    /// Optional admin API endpoint.
    #[serde(default)]
    pub admin_endpoint: Option<String>,
    /// Command to reload Caddy (absolute path + args as list — executed via helper or updater).
    pub reload_argv: Vec<String>,
}

/// Timeouts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutsConfig {
    /// Cluster create seconds.
    pub cluster_create_secs: u64,
    /// Cluster control seconds.
    pub cluster_control_secs: u64,
    /// Cluster delete seconds.
    pub cluster_delete_secs: u64,
    /// Config update seconds.
    pub config_update_secs: u64,
    /// Log read seconds.
    pub log_read_secs: u64,
    /// Helper request default seconds.
    pub helper_default_secs: u64,
}

/// Logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// tracing filter, e.g. info,pgpanel=debug
    pub level: String,
    /// JSON logs.
    #[serde(default)]
    pub json: bool,
}

impl Config {
    /// Load from a TOML file, applying environment overrides for secrets.
    pub fn load(path: &std::path::Path) -> CoreResult<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| CoreError::Config(format!("failed to read {}: {e}", path.display())))?;
        let mut cfg: Config = toml::from_str(&text)?;
        if let Ok(secret) = std::env::var("PGPANEL_SECRET_KEY") {
            if !secret.is_empty() {
                cfg.app.secret_key = secret;
            }
        }
        if let Ok(key) = std::env::var("PGPANEL_DATABASUS_API_KEY") {
            if !key.is_empty() {
                cfg.databasus.api_key = Some(key);
            }
        }
        if let Ok(slot) = std::env::var("PGPANEL_SLOT") {
            if !slot.is_empty() {
                cfg.app.slot = slot;
            }
        }
        if let Ok(listen) = std::env::var("PGPANEL_LISTEN") {
            if let Ok(addr) = listen.parse() {
                cfg.server.listen = addr;
            }
        }
        cfg.validate()?;
        Ok(cfg)
    }

    /// Validate configuration invariants.
    pub fn validate(&self) -> CoreResult<()> {
        if self.app.secret_key.len() < 32 {
            return Err(CoreError::Config(
                "secret_key must be at least 32 characters".into(),
            ));
        }
        if !matches!(self.app.slot.as_str(), "blue" | "green") {
            return Err(CoreError::Config(
                "app.slot must be 'blue' or 'green'".into(),
            ));
        }
        if !matches!(self.app.active_slot.as_str(), "blue" | "green") {
            return Err(CoreError::Config(
                "app.active_slot must be 'blue' or 'green'".into(),
            ));
        }
        if !matches!(self.updates.channel.as_str(), "stable" | "prerelease") {
            return Err(CoreError::Config(
                "updates.channel must be 'stable' or 'prerelease'".into(),
            ));
        }
        if !self.server.listen.ip().is_loopback() {
            // Soft warning via tracing; still allow but document risk.
            tracing::warn!(
                listen = %self.server.listen,
                "PgPanel is not bound to loopback; ensure firewall and TLS terminate correctly"
            );
        }
        Ok(())
    }

    /// Default development configuration (localhost only).
    pub fn dev_default() -> Self {
        Self {
            app: AppConfig {
                secret_key: "dev-secret-key-change-me-in-production-32b".into(),
                active_slot: "blue".into(),
                slot: "blue".into(),
                allow_superuser_creation: false,
            },
            server: ServerConfig {
                listen: "127.0.0.1:8081".parse().expect("listen"),
                public_base_url: None,
            },
            paths: PathsConfig {
                sqlite: PathBuf::from("./data/pgpanel.db"),
                helper_socket: PathBuf::from("./data/helper.sock"),
                state_dir: PathBuf::from("./data"),
                config_dir: PathBuf::from("./config"),
                signing_public_key: PathBuf::from("./keys/signing.pub"),
                updater_socket: PathBuf::from("./data/updater.sock"),
            },
            session: SessionConfig {
                cookie_name: "pgpanel_session".into(),
                ttl_secs: 60 * 60 * 12,
                absolute_ttl_secs: 60 * 60 * 24 * 7,
                same_site: "Strict".into(),
                secure: false,
                reauth_window_secs: 300,
            },
            auth: AuthConfig {
                totp_issuer: "PgPanel".into(),
                max_failed_logins: 5,
                lockout_secs: 900,
                require_totp_for_destructive: true,
            },
            trusted_proxies: TrustedProxiesConfig {
                enabled: true,
                proxies: vec!["127.0.0.1".into(), "::1".into()],
                prefer_cf_connecting_ip: true,
            },
            rate_limit: RateLimitConfig {
                login_per_ip: 10,
                login_window_secs: 300,
                api_per_ip_per_minute: 600,
            },
            postgres: PostgresConfig {
                allowed_versions: vec!["16".into(), "17".into(), "18".into()],
                data_directory_roots: vec![PathBuf::from("/var/lib/postgresql")],
                default_statement_timeout_ms: 30_000,
                default_max_rows: 500,
                binaries: PostgresBinaries {
                    pg_lsclusters: PathBuf::from("/usr/bin/pg_lsclusters"),
                    pg_createcluster: PathBuf::from("/usr/bin/pg_createcluster"),
                    pg_ctlcluster: PathBuf::from("/usr/bin/pg_ctlcluster"),
                    pg_renamecluster: PathBuf::from("/usr/bin/pg_renamecluster"),
                    pg_dropcluster: PathBuf::from("/usr/bin/pg_dropcluster"),
                    pg_conftool: PathBuf::from("/usr/bin/pg_conftool"),
                },
            },
            databasus: DatabasusConfig::default(),
            monitoring: MonitoringConfig {
                retention_hours: 72,
                sample_interval_secs: 60,
            },
            updates: UpdatesConfig {
                github_owner: "pgpanel".into(),
                github_repo: "pgpanel".into(),
                channel: "stable".into(),
                check_interval_secs: 86400,
                auto_install: false,
                schedule_window: None,
                timeout_secs: 600,
                max_download_bytes: 512 * 1024 * 1024,
                drain_secs: 30,
                install_root: PathBuf::from("/opt/pgpanel"),
            },
            caddy: CaddyConfig {
                upstream_config_path: PathBuf::from("/etc/caddy/pgpanel-upstream.caddy"),
                blue_upstream: "127.0.0.1:8081".into(),
                green_upstream: "127.0.0.1:8082".into(),
                admin_endpoint: None,
                reload_argv: vec!["/usr/bin/systemctl".into(), "reload".into(), "caddy".into()],
            },
            timeouts: TimeoutsConfig {
                cluster_create_secs: 120,
                cluster_control_secs: 60,
                cluster_delete_secs: 120,
                config_update_secs: 90,
                log_read_secs: 15,
                helper_default_secs: 30,
            },
            logging: LoggingConfig {
                level: "info,pgpanel_web=debug,pgpanel_helper=debug".into(),
                json: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_default_validates() {
        Config::dev_default().validate().unwrap();
    }
}
