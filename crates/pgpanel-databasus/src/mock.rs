use async_trait::async_trait;
use chrono::Utc;
use std::sync::Mutex;
use uuid::Uuid;

use pgpanel_core::error::Result;
use pgpanel_core::models::{BackupStatus, DatabasusIntegrationStatus};

use crate::traits::*;

/// In-memory mock for tests and MVP without real Databasus.
pub struct MockDatabasusAdapter {
    registered: Mutex<Vec<(Uuid, String)>>,
    fail_next: Mutex<bool>,
}

impl MockDatabasusAdapter {
    pub fn new() -> Self {
        Self {
            registered: Mutex::new(Vec::new()),
            fail_next: Mutex::new(false),
        }
    }

    pub fn set_fail_next(&self, fail: bool) {
        *self.fail_next.lock().unwrap() = fail;
    }
}

impl Default for MockDatabasusAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DatabasusAdapter for MockDatabasusAdapter {
    async fn register_cluster(&self, req: RegisterRequest) -> Result<ClusterRegistration> {
        if *self.fail_next.lock().unwrap() {
            *self.fail_next.lock().unwrap() = false;
            return Err(pgpanel_core::Error::Databasus(
                "mock failure: service unavailable".into(),
            ));
        }
        let external_id = format!("mock-{}", req.cluster_id);
        self.registered
            .lock()
            .unwrap()
            .push((req.cluster_id, external_id.clone()));
        Ok(ClusterRegistration {
            external_id: Some(external_id),
            status: DatabasusIntegrationStatus::Registered,
            message: Some("registered via mock adapter".into()),
            manual_setup_info: None,
        })
    }

    async fn update_cluster(&self, _external_id: &str, _req: RegisterRequest) -> Result<()> {
        Ok(())
    }

    async fn unregister_cluster(&self, external_id: &str) -> Result<()> {
        self.registered
            .lock()
            .unwrap()
            .retain(|(_, id)| id != external_id);
        Ok(())
    }

    async fn get_backup_status(&self, external_id: &str) -> Result<BackupStatus> {
        let known = self
            .registered
            .lock()
            .unwrap()
            .iter()
            .any(|(_, id)| id == external_id);
        Ok(BackupStatus {
            integration_status: if known {
                DatabasusIntegrationStatus::Registered
            } else {
                DatabasusIntegrationStatus::NotConfigured
            },
            last_successful_backup: Some(Utc::now() - chrono::Duration::hours(1)),
            last_backup_status: Some("success".into()),
            backup_lag_seconds: Some(3600),
            wal_status: Some("streaming".into()),
            failed_backups: 0,
            message: Some("mock backup status".into()),
            manual_setup_info: None,
        })
    }

    async fn get_latest_backup(&self, _external_id: &str) -> Result<Option<BackupInfo>> {
        Ok(Some(BackupInfo {
            id: Some("mock-backup-1".into()),
            status: "success".into(),
            started_at: Some(Utc::now() - chrono::Duration::hours(1)),
            finished_at: Some(
                Utc::now() - chrono::Duration::hours(1) + chrono::Duration::minutes(5),
            ),
            size_bytes: Some(1024 * 1024 * 100),
        }))
    }

    async fn get_wal_status(&self, _external_id: &str) -> Result<WalStatus> {
        Ok(WalStatus {
            status: "streaming".into(),
            lag_bytes: Some(0),
            last_archived: Some(Utc::now()),
        })
    }

    async fn trigger_backup(&self, _external_id: &str) -> Result<BackupInfo> {
        Ok(BackupInfo {
            id: Some(format!("mock-backup-{}", Uuid::new_v4())),
            status: "started".into(),
            started_at: Some(Utc::now()),
            finished_at: None,
            size_bytes: None,
        })
    }

    async fn test_connection(&self) -> Result<bool> {
        Ok(true)
    }
}
