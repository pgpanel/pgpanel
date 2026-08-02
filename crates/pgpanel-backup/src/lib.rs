//! Native backup engine for PgPanel (replaces Databasus).
#![forbid(unsafe_code)]

pub mod dump;
mod engine;
mod notify;
mod storage;
mod types;
mod wal;

pub use engine::BackupEngine;
pub use notify::notify_webhook;
pub use storage::{LocalStorage, S3Storage, S3StorageConfig, StorageBackend};
pub use types::*;
pub use wal::{
    archive_settings_sql, collect_segments, enable_archiving, parse_wal_filename,
    pg_basebackup_tar, query_wal_status, switch_wal, WalConfig, WalFilename, WalSegmentMetadata,
    WalStatus,
};

use pgpanel_core::config::Config;
use std::path::PathBuf;
use std::sync::Arc;

pub fn build_engine(config: &Config) -> Arc<BackupEngine> {
    let data = ensure_dirs(config);
    let storage: Arc<dyn StorageBackend> = match config.backup_storage_type.as_str() {
        "s3" | "r2" | "b2" | "hetzner" | "minio" => match S3Storage::from_env(data.clone()) {
            Ok(s) => {
                tracing::info!("backup storage: S3-compatible (signed HTTP)");
                Arc::new(s)
            }
            Err(e) => {
                tracing::warn!(error = %e, "S3 init failed; local storage");
                Arc::new(LocalStorage::new(data.clone()))
            }
        },
        _ => {
            tracing::info!(path = %data.display(), "backup storage: local");
            Arc::new(LocalStorage::new(data.clone()))
        }
    };
    Arc::new(BackupEngine::new(data, storage, config.clone()))
}

pub fn ensure_dirs(config: &Config) -> PathBuf {
    let data = config
        .backup_data_dir
        .clone()
        .unwrap_or_else(|| config.data_dir.join("backups"));
    let _ = std::fs::create_dir_all(&data);
    let _ = std::fs::create_dir_all(data.join("logical"));
    let _ = std::fs::create_dir_all(data.join("wal"));
    let _ = std::fs::create_dir_all(data.join("verify"));
    data
}
