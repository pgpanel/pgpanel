use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use chrono::Utc;
use flate2::write::GzEncoder;
use flate2::Compression;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use tracing::info;
use uuid::Uuid;

use pgpanel_core::config::Config;
use pgpanel_core::crypto::{decrypt_secret, encrypt_secret};
use pgpanel_core::error::{Error, Result};

use crate::dump;
use crate::notify;
use crate::storage::StorageBackend;
use crate::types::*;

pub struct BackupEngine {
    pub data_dir: PathBuf,
    pub storage: Arc<RwLock<Arc<dyn StorageBackend>>>,
    pub config: Config,
}

impl BackupEngine {
    pub fn new(data_dir: PathBuf, storage: Arc<dyn StorageBackend>, config: Config) -> Self {
        Self {
            data_dir,
            storage: Arc::new(RwLock::new(storage)),
            config,
        }
    }

    pub fn storage_kind(&self) -> String {
        self.storage
            .try_read()
            .map(|storage| storage.kind().to_string())
            .unwrap_or_else(|_| "unknown".into())
    }

    pub async fn set_storage(&self, storage: Arc<dyn StorageBackend>) {
        *self.storage.write().await = storage;
    }

    pub async fn run_logical_backup(
        &self,
        req: RunLogicalBackupRequest,
    ) -> Result<LogicalBackupResult> {
        let backup_id = Uuid::new_v4();
        let started = Utc::now();
        info!(
            %backup_id,
            cluster = %req.cluster_id,
            db = %req.database,
            "logical backup start"
        );

        let raw = dump::pg_dump_custom(
            &req.container_name,
            &req.database,
            &req.username,
            &req.password,
            req.schema_only,
        )
        .await?;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        use std::io::Write;
        encoder
            .write_all(&raw)
            .map_err(|e| Error::Internal(format!("gzip: {e}")))?;
        let compressed = encoder
            .finish()
            .map_err(|e| Error::Internal(format!("gzip finish: {e}")))?;

        let payload = if req.encrypt {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
            let enc = encrypt_secret(&self.config.master_encryption_key, &SecretString::from(b64))?;
            format!("enc:v1:{enc}").into_bytes()
        } else {
            compressed
        };

        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let checksum = hex::encode(hasher.finalize());
        let size_bytes = payload.len() as u64;

        let kind = if req.schema_only { "schema" } else { "full" };
        let key = format!(
            "logical/{}/{}_{}_{}.dump.gz{}",
            req.cluster_id,
            req.database,
            kind,
            started.format("%Y%m%dT%H%M%SZ"),
            if req.encrypt { ".enc" } else { "" }
        );

        let storage = self.storage.read().await.clone();
        let storage_key = storage.put(&key, &payload).await?;

        let local = self
            .data_dir
            .join("logical")
            .join(format!("{backup_id}.dump.gz"));
        if let Some(parent) = local.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::write(&local, &payload).await;

        if let Ok(url) = std::env::var("WEBHOOK_URL") {
            if !url.is_empty() {
                notify::notify_webhook(
                    &url,
                    "PgPanel backup succeeded",
                    &format!(
                        "cluster={} db={} size={}B key={}",
                        req.cluster_id, req.database, size_bytes, storage_key
                    ),
                )
                .await;
            }
        }

        Ok(LogicalBackupResult {
            backup_id,
            storage_key,
            size_bytes,
            checksum_sha256: checksum,
            local_path: Some(local.display().to_string()),
        })
    }

    pub async fn load_raw_dump(&self, storage_key: &str, encrypted: bool) -> Result<Vec<u8>> {
        let storage = self.storage.read().await.clone();
        let mut data = storage.get(storage_key).await?;
        if encrypted || storage_key.ends_with(".enc") || data.starts_with(b"enc:v1:") {
            let s = String::from_utf8_lossy(&data);
            let token = s.strip_prefix("enc:v1:").unwrap_or(&s);
            let plain = decrypt_secret(&self.config.master_encryption_key, token)?;
            use base64::Engine;
            data = base64::engine::general_purpose::STANDARD
                .decode(plain.expose_secret().as_bytes())
                .map_err(|e| Error::Internal(format!("b64 decode: {e}")))?;
        }
        use flate2::read::GzDecoder;
        use std::io::Read;
        let mut decoder = GzDecoder::new(&data[..]);
        let mut raw = Vec::new();
        decoder
            .read_to_end(&mut raw)
            .map_err(|e| Error::Internal(format!("gunzip: {e}")))?;
        Ok(raw)
    }

    pub async fn verify_logical(
        &self,
        container: &str,
        storage_key: &str,
        encrypted: bool,
    ) -> Result<String> {
        let raw = self.load_raw_dump(storage_key, encrypted).await?;
        dump::pg_restore_list(container, &raw).await
    }

    pub async fn restore_logical(
        &self,
        container: &str,
        database: &str,
        username: &str,
        password: &str,
        storage_key: &str,
        encrypted: bool,
        clean: bool,
    ) -> Result<()> {
        let raw = self.load_raw_dump(storage_key, encrypted).await?;
        dump::pg_restore_custom(container, database, username, password, &raw, clean).await
    }
}
