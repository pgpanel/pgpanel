use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use pgpanel_core::error::Result;
use pgpanel_core::models::{BackupStatus, DatabasusIntegrationStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub cluster_id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    pub postgres_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterRegistration {
    pub external_id: Option<String>,
    pub status: DatabasusIntegrationStatus,
    pub message: Option<String>,
    pub manual_setup_info: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: Option<String>,
    pub status: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalStatus {
    pub status: String,
    pub lag_bytes: Option<u64>,
    pub last_archived: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait DatabasusAdapter: Send + Sync {
    async fn register_cluster(&self, req: RegisterRequest) -> Result<ClusterRegistration>;
    async fn update_cluster(&self, external_id: &str, req: RegisterRequest) -> Result<()>;
    async fn unregister_cluster(&self, external_id: &str) -> Result<()>;
    async fn get_backup_status(&self, external_id: &str) -> Result<BackupStatus>;
    async fn get_latest_backup(&self, external_id: &str) -> Result<Option<BackupInfo>>;
    async fn get_wal_status(&self, external_id: &str) -> Result<WalStatus>;
    async fn trigger_backup(&self, external_id: &str) -> Result<BackupInfo>;
    async fn test_connection(&self) -> Result<bool>;
}
