use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use pgpanel_core::error::{Error, Result};

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn put(&self, key: &str, data: &[u8]) -> Result<String>;
    async fn get(&self, key: &str) -> Result<Vec<u8>>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn exists(&self, key: &str) -> Result<bool>;
    fn kind(&self) -> &'static str;
}

pub struct LocalStorage {
    pub root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        let safe = key.replace("..", "_").replace('\\', "/");
        self.root.join(safe)
    }
}

#[async_trait]
impl StorageBackend for LocalStorage {
    async fn put(&self, key: &str, data: &[u8]) -> Result<String> {
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::Internal(format!("mkdir: {e}")))?;
        }
        let mut f = fs::File::create(&path)
            .await
            .map_err(|e| Error::Internal(format!("create {path:?}: {e}")))?;
        f.write_all(data)
            .await
            .map_err(|e| Error::Internal(format!("write: {e}")))?;
        f.flush()
            .await
            .map_err(|e| Error::Internal(format!("flush: {e}")))?;
        info!(%key, bytes = data.len(), "local backup stored");
        Ok(key.to_string())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let path = self.path_for(key);
        fs::read(&path)
            .await
            .map_err(|e| Error::NotFound(format!("backup {key}: {e}")))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.path_for(key);
        if path.exists() {
            fs::remove_file(&path)
                .await
                .map_err(|e| Error::Internal(format!("delete: {e}")))?;
        }
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        Ok(self.path_for(key).exists())
    }

    fn kind(&self) -> &'static str {
        "local"
    }
}

/// S3-compatible storage via AWS CLI-less HTTP (PutObject signature v4 is complex).
/// For v1 we shell out to `aws s3 cp` when AWS CLI is present; otherwise store local
/// and log that S3 sync is deferred. Production path: set BACKUP_STORAGE_TYPE=s3 and
/// S3_* env — engine uses multipart upload via curl + pre-configured rclone later.
///
/// Current implementation: store under local root `s3-staging/` and record key as
/// `s3://bucket/prefix/key` for metadata; a background sync can upload. Full SDK
/// upload lands in a follow-up without blocking install.
pub struct S3Storage {
    local: LocalStorage,
    bucket: String,
    prefix: String,
    endpoint: Option<String>,
}

impl S3Storage {
    pub fn from_env(local_root: PathBuf) -> Result<Self> {
        let bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| "pgpanel".into());
        let prefix = std::env::var("S3_PREFIX").unwrap_or_else(|_| "pgpanel/".into());
        let endpoint = std::env::var("S3_ENDPOINT").ok();
        let staging = local_root.join("s3-staging");
        std::fs::create_dir_all(&staging).ok();
        Ok(Self {
            local: LocalStorage::new(staging),
            bucket,
            prefix,
            endpoint,
        })
    }

    fn meta_key(&self, key: &str) -> String {
        format!(
            "s3://{}/{}{}",
            self.bucket,
            self.prefix,
            key.trim_start_matches('/')
        )
    }
}

#[async_trait]
impl StorageBackend for S3Storage {
    async fn put(&self, key: &str, data: &[u8]) -> Result<String> {
        // Stage locally always
        let staged = self.local.put(key, data).await?;
        // Best-effort: aws s3 cp if available
        if let Ok(aws) = which_aws() {
            let dest = format!("s3://{}/{}{}", self.bucket, self.prefix, key);
            let path = self.local.root.join(key.replace("..", "_"));
            let mut cmd = tokio::process::Command::new(aws);
            cmd.arg("s3").arg("cp").arg(&path).arg(&dest);
            if let Some(ep) = &self.endpoint {
                cmd.arg("--endpoint-url").arg(ep);
            }
            match cmd.output().await {
                Ok(o) if o.status.success() => {
                    info!(%dest, "uploaded backup to S3");
                    return Ok(self.meta_key(key));
                }
                Ok(o) => {
                    warn!(
                        stderr = %String::from_utf8_lossy(&o.stderr),
                        "aws s3 cp failed; kept local staging"
                    );
                }
                Err(e) => warn!(error = %e, "aws cli invoke failed"),
            }
        } else {
            warn!("aws CLI not found; backup staged locally for S3 path {}", self.meta_key(key));
        }
        Ok(format!("local-staging:{staged}"))
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let stripped = key
            .strip_prefix("local-staging:")
            .or_else(|| key.strip_prefix(&format!("s3://{}/{}", self.bucket, self.prefix)))
            .unwrap_or(key);
        self.local.get(stripped).await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let stripped = key
            .strip_prefix("local-staging:")
            .unwrap_or(key);
        self.local.delete(stripped).await
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let stripped = key
            .strip_prefix("local-staging:")
            .unwrap_or(key);
        self.local.exists(stripped).await
    }

    fn kind(&self) -> &'static str {
        "s3"
    }
}

fn which_aws() -> Result<String> {
    which_bin("aws")
}

fn which_bin(name: &str) -> Result<String> {
    let output = std::process::Command::new("which")
        .arg(name)
        .output()
        .map_err(|e| Error::Internal(e.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(Error::NotFound(name.into()))
    }
}

// Expose root for S3 staging path join
impl S3Storage {
    #[allow(dead_code)]
    fn root(&self) -> &std::path::Path {
        &self.local.root
    }
}
