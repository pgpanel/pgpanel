//! WAL archiving / PITR configuration helpers.
//! Continuous archiving uses PostgreSQL archive_command into a panel-managed path.

use chrono::{DateTime, Utc};
use pgpanel_core::error::{Error, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::SystemTime;
use tokio::process::Command;
use uuid::Uuid;

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
    let p = archive_path.trim_end_matches('/').replace('\'', "''");
    vec![
        "ALTER SYSTEM SET wal_level = 'replica';".into(),
        "ALTER SYSTEM SET archive_mode = 'on';".into(),
        format!(
            "ALTER SYSTEM SET archive_command = 'test ! -f \"{p}/%f\" && cp \"%p\" \"{p}/%f\"';"
        ),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalFilename {
    pub filename: String,
    pub timeline: u32,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalSegmentMetadata {
    pub id: String,
    pub cluster_id: String,
    pub filename: String,
    pub timeline: u32,
    pub size_bytes: u64,
    pub archived_at: Option<String>,
    pub synced_at: String,
    pub storage_key: String,
    pub checksum_sha256: String,
}

fn wal_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?P<timeline>[0-9A-Fa-f]{8})(?P<sequence>[0-9A-Fa-f]{16})$")
            .expect("valid WAL regex")
    })
}

/// Parse a PostgreSQL WAL segment name (8 hex timeline + 16 hex segment number).
pub fn parse_wal_filename(filename: &str) -> Result<WalFilename> {
    let Some(caps) = wal_regex().captures(filename) else {
        return Err(Error::Validation(format!(
            "invalid WAL filename: {filename}"
        )));
    };
    let timeline = u32::from_str_radix(&caps["timeline"], 16)
        .map_err(|_| Error::Validation("invalid WAL timeline".into()))?;
    let sequence = u64::from_str_radix(&caps["sequence"], 16)
        .map_err(|_| Error::Validation("invalid WAL sequence".into()))?;
    Ok(WalFilename {
        filename: filename.to_string(),
        timeline,
        sequence,
    })
}

fn docker_error(operation: &str, output: &std::process::Output) -> Error {
    Error::Internal(format!(
        "{operation}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

async fn psql(container: &str, password: &str, sql: &str) -> Result<String> {
    let output = Command::new("docker")
        .args([
            "exec",
            "-e",
            &format!("PGPASSWORD={password}"),
            container,
            "psql",
            "-U",
            "postgres",
            "-d",
            "postgres",
            "-v",
            "ON_ERROR_STOP=1",
            "-At",
            "-c",
            sql,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("docker exec psql: {e}")))?;
    if !output.status.success() {
        return Err(docker_error("psql failed", &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Configure PostgreSQL archiving and create the archive directory.
pub async fn enable_archiving(container: &str, password: &str) -> Result<()> {
    for statement in archive_settings_sql(&WalConfig::default().archive_dir) {
        psql(container, password, &statement).await?;
    }
    psql(container, password, "SELECT pg_reload_conf();").await?;
    let output = Command::new("docker")
        .args([
            "exec",
            container,
            "mkdir",
            "-p",
            WalConfig::default().archive_dir.as_str(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("create WAL archive directory: {e}")))?;
    if !output.status.success() {
        return Err(docker_error("create WAL archive directory failed", &output));
    }
    Ok(())
}

/// Request PostgreSQL to close the current WAL segment.
pub async fn switch_wal(container: &str, password: &str) -> Result<String> {
    psql(container, password, "SELECT pg_switch_wal();").await
}

/// Read archiver configuration and counters from PostgreSQL.
pub async fn query_wal_status(container: &str, password: &str) -> Result<WalStatus> {
    let output = psql(
        container,
        password,
        "SELECT current_setting('archive_mode'), current_setting('wal_level'), coalesce((SELECT last_archived_wal FROM pg_stat_archiver), ''), coalesce((SELECT failed_count::text FROM pg_stat_archiver), '0'), current_setting('archive_command');",
    )
    .await?;
    let fields: Vec<&str> = output.splitn(5, '|').collect();
    if fields.len() != 5 {
        return Err(Error::Internal(
            "unexpected PostgreSQL WAL status response".into(),
        ));
    }
    let failed_count = fields[3].parse::<u64>().unwrap_or(0);
    Ok(WalStatus {
        archive_mode: non_empty(fields[0]),
        wal_level: non_empty(fields[1]),
        last_archived_wal: non_empty(fields[2]),
        failed_count,
        message: format!("archive_command={}", fields[4]),
    })
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

/// Copy newly archived WAL segments from a container to the panel's WAL root.
pub async fn collect_segments(
    container: &str,
    cluster_id: &str,
    host_wal_root: &Path,
) -> Result<Vec<WalSegmentMetadata>> {
    let parsed_cluster =
        Uuid::parse_str(cluster_id).map_err(|_| Error::Validation("invalid cluster id".into()))?;
    let local_root = host_wal_root.join(parsed_cluster.to_string());
    tokio::fs::create_dir_all(&local_root)
        .await
        .map_err(|e| Error::Internal(format!("create local WAL directory: {e}")))?;

    let output = Command::new("docker")
        .args([
            "exec",
            container,
            "find",
            WalConfig::default().archive_dir.as_str(),
            "-maxdepth",
            "1",
            "-type",
            "f",
            "-printf",
            "%f\n",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("list WAL archive: {e}")))?;
    if !output.status.success() {
        return Err(docker_error("list WAL archive failed", &output));
    }

    let mut result = Vec::new();
    for filename in String::from_utf8_lossy(&output.stdout).lines() {
        let filename = filename.trim();
        if filename.is_empty() {
            continue;
        }
        let wal = parse_wal_filename(filename)?;
        let local_path = local_root.join(&wal.filename);
        if tokio::fs::try_exists(&local_path)
            .await
            .map_err(|e| Error::Internal(format!("check WAL segment: {e}")))?
        {
            continue;
        }

        let remote = format!(
            "{}/{}",
            WalConfig::default().archive_dir.trim_end_matches('/'),
            wal.filename
        );
        let destination = local_path.to_string_lossy().to_string();
        let cp = Command::new("docker")
            .args(["cp", &format!("{container}:{remote}"), &destination])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| Error::Internal(format!("copy WAL segment: {e}")))?;
        if !cp.status.success() {
            return Err(docker_error("copy WAL segment failed", &cp));
        }

        let bytes = tokio::fs::read(&local_path)
            .await
            .map_err(|e| Error::Internal(format!("read WAL segment: {e}")))?;
        let checksum = hex::encode(Sha256::digest(&bytes));
        let modified = tokio::fs::metadata(&local_path)
            .await
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(system_time_to_rfc3339);
        result.push(WalSegmentMetadata {
            id: Uuid::new_v4().to_string(),
            cluster_id: parsed_cluster.to_string(),
            filename: wal.filename.clone(),
            timeline: wal.timeline,
            size_bytes: bytes.len() as u64,
            archived_at: modified,
            synced_at: Utc::now().to_rfc3339(),
            storage_key: format!("wal/{cluster_id}/{}", wal.filename),
            checksum_sha256: checksum,
        });
    }
    Ok(result)
}

fn system_time_to_rfc3339(value: SystemTime) -> Option<String> {
    DateTime::<Utc>::from(value).to_rfc3339().into()
}

/// Create a compressed tar-format physical base backup through docker exec.
pub async fn pg_basebackup_tar(container: &str, password: &str) -> Result<Vec<u8>> {
    let output = Command::new("docker")
        .args([
            "exec",
            "-e",
            &format!("PGPASSWORD={password}"),
            container,
            "pg_basebackup",
            "-U",
            "postgres",
            "-D",
            "-",
            "-Ft",
            "-z",
            "-c",
            "fast",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("docker exec pg_basebackup: {e}")))?;
    if !output.status.success() {
        return Err(docker_error("pg_basebackup failed", &output));
    }
    if output.stdout.is_empty() {
        return Err(Error::Internal(
            "pg_basebackup produced empty output".into(),
        ));
    }
    Ok(output.stdout)
}
