//! WAL archiving / PITR configuration helpers.
//! Continuous archiving uses PostgreSQL archive_command into a panel-managed path.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalConfig {
    pub enabled: bool,
    pub archive_dir: String,
    pub compress: bool,
}

impl Default for WalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            archive_dir: "/var/lib/postgresql/wal_archive".into(),
            compress: true,
        }
    }
}

/// SQL statements to enable WAL archiving (archive_mode requires restart).
pub fn archive_settings_sql(archive_path: &str) -> Vec<String> {
    let p = archive_path.trim_end_matches('/');
    vec![
        "ALTER SYSTEM SET wal_level = 'replica';".into(),
        "ALTER SYSTEM SET archive_mode = 'on';".into(),
        format!("ALTER SYSTEM SET archive_command = 'test ! -f {p}/%f && cp %p {p}/%f';"),
        "ALTER SYSTEM SET archive_timeout = '60s';".into(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalStatus {
    pub archive_mode: Option<String>,
    pub wal_level: Option<String>,
    pub last_archived_wal: Option<String>,
    pub failed_count: u64,
    pub message: String,
}
