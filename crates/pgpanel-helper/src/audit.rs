//! Helper-side audit logging to JSONL and tracing.

use pgpanel_core::audit::{chain_hash, AuditEvent, AUDIT_GENESIS};
use pgpanel_core::config::Config;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Persisted audit record with chained hash.
#[derive(Debug, Clone, Serialize)]
struct AuditRecord {
    #[serde(flatten)]
    event: AuditEvent,
    chain_hash: String,
}

/// Thread-safe audit writer.
#[derive(Clone)]
pub struct AuditWriter {
    path: Option<PathBuf>,
    chain: Arc<Mutex<String>>,
}

impl AuditWriter {
    /// Create an audit writer. When `jsonl_path` is `None`, events are tracing-only.
    pub fn new(jsonl_path: Option<PathBuf>) -> Self {
        Self {
            path: jsonl_path,
            chain: Arc::new(Mutex::new(AUDIT_GENESIS.to_string())),
        }
    }

    /// Build from application config (`state_dir/helper-audit.jsonl`).
    pub fn from_config(config: &Config) -> Self {
        let path = config.paths.state_dir.join("helper-audit.jsonl");
        Self::new(Some(path))
    }

    /// Record an audit event.
    pub async fn record(&self, event: AuditEvent) {
        let mut chain = self.chain.lock().await;
        let hash = chain_hash(&chain, &event);
        let record = AuditRecord {
            event: event.clone(),
            chain_hash: hash.clone(),
        };
        *chain = hash;

        info!(
            target: "pgpanel_helper::audit",
            action = %event.action,
            result = %event.result,
            request_id = %event.request_id,
            user_id = ?event.user_id,
            username = ?event.username,
            source_ip = ?event.source_ip,
            target = ?event.target,
            failure_reason = ?event.failure_reason,
            "audit event"
        );

        if let Some(path) = &self.path {
            if let Err(e) = self.append_jsonl(path, &record).await {
                warn!(
                    target: "pgpanel_helper::audit",
                    path = %path.display(),
                    error = %e,
                    "failed to write audit JSONL"
                );
            }
        }
    }

    async fn append_jsonl(&self, path: &PathBuf, record: &AuditRecord) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let line = serde_json::to_string(record).map_err(std::io::Error::other)?;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        Ok(())
    }
}
