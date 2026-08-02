//! HTTP adapter for Databasus.
//!
//! IMPORTANT: Endpoint paths must be verified against the actual Databasus
//! version before production use. The paths below are placeholders that
//! return clear errors if the API shape differs.

use async_trait::async_trait;
use tracing::{info, warn};

use pgpanel_core::error::{Error, Result};
use pgpanel_core::models::{BackupStatus, DatabasusIntegrationStatus};

use crate::traits::*;

pub struct HttpDatabasusAdapter {
    base_url: String,
    token: String,
    client: reqwest::Client,
}

impl HttpDatabasusAdapter {
    pub fn new(base_url: String, token: String) -> Self {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            client,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

#[async_trait]
impl DatabasusAdapter for HttpDatabasusAdapter {
    async fn register_cluster(&self, req: RegisterRequest) -> Result<ClusterRegistration> {
        // Soft-fail path only — endpoints are NOT guaranteed for all Databasus versions.
        // Prefer Manual adapter (DATABASUS_HTTP_API=0, the default).
        let url = self.url("/api/v1/storages");
        info!(%url, cluster = %req.cluster_id, "attempting Databasus registration (opt-in HTTP API)");

        let body = serde_json::json!({
            "name": req.name,
            "type": "postgresql",
            "host": req.host,
            "port": req.port,
            "username": req.username,
            "password": req.password,
            "database": req.database,
        });

        let resp = match self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                // Connection / DNS / TLS failures → manual setup, never hard error.
                warn!(error = %e, %url, "Databasus HTTP unreachable; falling back to manual setup");
                return Ok(ClusterRegistration {
                    external_id: None,
                    status: DatabasusIntegrationStatus::PendingManualSetup,
                    message: Some(format!(
                        "Databasus HTTP unreachable ({e}). Add the cluster manually in the Databasus UI."
                    )),
                    manual_setup_info: Some(serde_json::json!({
                        "host": req.host,
                        "port": req.port,
                        "username": req.username,
                        "database": req.database,
                        "error": e.to_string(),
                    })),
                });
            }
        };

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            warn!(%status, body = %text, "Databasus register failed");
            // Fall back to manual setup rather than inventing endpoints.
            return Ok(ClusterRegistration {
                external_id: None,
                status: DatabasusIntegrationStatus::PendingManualSetup,
                message: Some(format!(
                    "Databasus API returned {status}. Verify API paths for your Databasus version, or keep DATABASUS_HTTP_API=0. Response: {}",
                    text.chars().take(200).collect::<String>()
                )),
                manual_setup_info: Some(serde_json::json!({
                    "host": req.host,
                    "port": req.port,
                    "username": req.username,
                    "database": req.database,
                    "http_status": status.as_u16(),
                })),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let external_id = parsed
            .get("id")
            .or_else(|| parsed.get("storage_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(ClusterRegistration {
            external_id,
            status: DatabasusIntegrationStatus::Registered,
            message: Some("registered with Databasus".into()),
            manual_setup_info: None,
        })
    }

    async fn update_cluster(&self, external_id: &str, req: RegisterRequest) -> Result<()> {
        let url = self.url(&format!("/api/v1/storages/{external_id}"));
        let body = serde_json::json!({
            "name": req.name,
            "host": req.host,
            "port": req.port,
        });
        let resp = self
            .client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Databasus(format!("update: {e}")))?;
        if !resp.status().is_success() {
            return Err(Error::Databasus(format!(
                "update failed: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    async fn unregister_cluster(&self, external_id: &str) -> Result<()> {
        let url = self.url(&format!("/api/v1/storages/{external_id}"));
        let resp = self
            .client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| Error::Databasus(format!("unregister: {e}")))?;
        if !resp.status().is_success() && resp.status().as_u16() != 404 {
            return Err(Error::Databasus(format!(
                "unregister failed: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    async fn get_backup_status(&self, external_id: &str) -> Result<BackupStatus> {
        let url = self.url(&format!("/api/v1/storages/{external_id}/backups/status"));
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| Error::Databasus(format!("backup status: {e}")))?;

        if !resp.status().is_success() {
            return Ok(BackupStatus {
                integration_status: DatabasusIntegrationStatus::Error,
                last_successful_backup: None,
                last_backup_status: None,
                backup_lag_seconds: None,
                wal_status: None,
                failed_backups: 0,
                message: Some(format!("status API returned {}", resp.status())),
                manual_setup_info: None,
            });
        }

        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::Databasus(format!("parse status: {e}")))?;

        Ok(BackupStatus {
            integration_status: DatabasusIntegrationStatus::Registered,
            last_successful_backup: None,
            last_backup_status: v
                .get("status")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            backup_lag_seconds: v.get("lag_seconds").and_then(|x| x.as_i64()),
            wal_status: v
                .get("wal_status")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            failed_backups: v.get("failed_count").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            message: None,
            manual_setup_info: None,
        })
    }

    async fn get_latest_backup(&self, external_id: &str) -> Result<Option<BackupInfo>> {
        let url = self.url(&format!("/api/v1/storages/{external_id}/backups/latest"));
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| Error::Databasus(format!("latest backup: {e}")))?;
        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(Error::Databasus(format!(
                "latest backup: {}",
                resp.status()
            )));
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::Databasus(e.to_string()))?;
        Ok(Some(BackupInfo {
            id: v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()),
            status: v
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string(),
            started_at: None,
            finished_at: None,
            size_bytes: v.get("size_bytes").and_then(|x| x.as_u64()),
        }))
    }

    async fn get_wal_status(&self, external_id: &str) -> Result<WalStatus> {
        let url = self.url(&format!("/api/v1/storages/{external_id}/wal"));
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| Error::Databasus(format!("wal: {e}")))?;
        if !resp.status().is_success() {
            return Ok(WalStatus {
                status: "unknown".into(),
                lag_bytes: None,
                last_archived: None,
            });
        }
        let v: serde_json::Value = resp.json().await.unwrap_or_default();
        Ok(WalStatus {
            status: v
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string(),
            lag_bytes: v.get("lag_bytes").and_then(|x| x.as_u64()),
            last_archived: None,
        })
    }

    async fn trigger_backup(&self, external_id: &str) -> Result<BackupInfo> {
        let url = self.url(&format!("/api/v1/storages/{external_id}/backups"));
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| Error::Databasus(format!("trigger: {e}")))?;
        if !resp.status().is_success() {
            return Err(Error::Databasus(format!(
                "trigger backup failed: {}",
                resp.status()
            )));
        }
        Ok(BackupInfo {
            id: None,
            status: "started".into(),
            started_at: Some(chrono::Utc::now()),
            finished_at: None,
            size_bytes: None,
        })
    }

    async fn test_connection(&self) -> Result<bool> {
        let url = self.url("/api/v1/health");
        match self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
        {
            Ok(r) => Ok(r.status().is_success()),
            Err(e) => {
                warn!(error = %e, "Databasus health check failed");
                Ok(false)
            }
        }
    }
}
