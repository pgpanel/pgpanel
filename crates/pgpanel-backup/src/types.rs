use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupKind {
    LogicalFull,
    LogicalSchema,
    BaseBackup,
    WalSegment,
}

impl BackupKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LogicalFull => "logical_full",
            Self::LogicalSchema => "logical_schema",
            Self::BaseBackup => "base_backup",
            Self::WalSegment => "wal_segment",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "logical_schema" => Self::LogicalSchema,
            "base_backup" => Self::BaseBackup,
            "wal_segment" => Self::WalSegment,
            _ => Self::LogicalFull,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupRunStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Verifying,
    Verified,
}

impl BackupRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Verifying => "verifying",
            Self::Verified => "verified",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "verifying" => Self::Verifying,
            "verified" => Self::Verified,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    pub id: Uuid,
    pub cluster_id: Uuid,
    pub kind: BackupKind,
    pub status: BackupRunStatus,
    pub database_name: String,
    pub storage_key: Option<String>,
    pub size_bytes: Option<u64>,
    pub checksum_sha256: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunLogicalBackupRequest {
    pub cluster_id: Uuid,
    pub container_name: String,
    pub database: String,
    pub username: String,
    pub password: String,
    pub schema_only: bool,
    pub encrypt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalBackupResult {
    pub backup_id: Uuid,
    pub storage_key: String,
    pub size_bytes: u64,
    pub checksum_sha256: String,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStatusSummary {
    pub integration_status: String,
    pub storage_type: String,
    pub last_successful_backup: Option<DateTime<Utc>>,
    pub last_backup_status: Option<String>,
    pub failed_backups: u32,
    pub wal_enabled: bool,
    pub wal_status: Option<String>,
    pub retention_days: u32,
    pub message: Option<String>,
    pub recent: Vec<BackupRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMetrics {
    pub cluster_id: Uuid,
    pub cpu_percent: f64,
    pub memory_usage_mb: f64,
    pub memory_limit_mb: f64,
    pub network_rx_bytes: Option<u64>,
    pub network_tx_bytes: Option<u64>,
    pub collected_at: DateTime<Utc>,
}
