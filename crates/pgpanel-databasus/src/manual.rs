use async_trait::async_trait;
use serde_json::json;

use pgpanel_core::error::Result;
use pgpanel_core::models::{BackupStatus, DatabasusIntegrationStatus};

use crate::traits::*;

/// Creates connection details and leaves status as PendingManualSetup.
/// Used when Databasus has no documented provisioning API or is not configured.
pub struct ManualDatabasusAdapter;

impl ManualDatabasusAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ManualDatabasusAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DatabasusAdapter for ManualDatabasusAdapter {
    async fn register_cluster(&self, req: RegisterRequest) -> Result<ClusterRegistration> {
        let info = json!({
            "instructions": "Add this PostgreSQL instance manually in the Databasus UI.",
            "host": req.host,
            "port": req.port,
            "username": req.username,
            "database": req.database,
            "postgres_version": req.postgres_version,
            "cluster_id": req.cluster_id,
            "note": "Password is stored encrypted in PgPanel; reveal it from the cluster credentials UI if needed. Password is never logged."
        });
        Ok(ClusterRegistration {
            external_id: None,
            status: DatabasusIntegrationStatus::PendingManualSetup,
            message: Some(
                "Databasus automatic registration unavailable; complete setup manually".into(),
            ),
            manual_setup_info: Some(info),
        })
    }

    async fn update_cluster(&self, _external_id: &str, _req: RegisterRequest) -> Result<()> {
        Ok(())
    }

    async fn unregister_cluster(&self, _external_id: &str) -> Result<()> {
        Ok(())
    }

    async fn get_backup_status(&self, _external_id: &str) -> Result<BackupStatus> {
        Ok(BackupStatus {
            integration_status: DatabasusIntegrationStatus::PendingManualSetup,
            last_successful_backup: None,
            last_backup_status: None,
            backup_lag_seconds: None,
            wal_status: None,
            failed_backups: 0,
            message: Some("Manual Databasus setup pending".into()),
            manual_setup_info: None,
        })
    }

    async fn get_latest_backup(&self, _external_id: &str) -> Result<Option<BackupInfo>> {
        Ok(None)
    }

    async fn get_wal_status(&self, _external_id: &str) -> Result<WalStatus> {
        Ok(WalStatus {
            status: "unknown".into(),
            lag_bytes: None,
            last_archived: None,
        })
    }

    async fn trigger_backup(&self, _external_id: &str) -> Result<BackupInfo> {
        Err(pgpanel_core::Error::Databasus(
            "cannot trigger backup: manual setup required".into(),
        ))
    }

    async fn test_connection(&self) -> Result<bool> {
        Ok(false)
    }
}
