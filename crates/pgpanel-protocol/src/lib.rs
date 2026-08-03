//! Typed protocol for privileged helper communication.
//!
//! All messages are length-prefixed JSON over a Unix domain socket.
//! The helper never accepts arbitrary commands.

#![deny(unsafe_code)]
#![warn(missing_docs, clippy::all)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Protocol version negotiated on connect.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum request body size (1 MiB).
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// Maximum response body size (16 MiB).
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Framing: 4-byte big-endian length prefix followed by UTF-8 JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Protocol version.
    pub version: u32,
    /// Correlation ID for audit/tracing.
    pub request_id: Uuid,
    /// Wall-clock time when the web process issued the request.
    pub issued_at: DateTime<Utc>,
    /// Calling panel user ID (for audit only; not authorization).
    pub actor_user_id: Option<i64>,
    /// Calling panel username (for audit only).
    pub actor_username: Option<String>,
    /// Source IP as observed by the web process.
    pub source_ip: Option<String>,
    /// Typed operation payload.
    pub op: HelperOp,
}

/// Strictly allowlisted privileged operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum HelperOp {
    /// List clusters via `pg_lsclusters`.
    ClusterList,
    /// Inspect a single cluster.
    ClusterInspect {
        /// PostgreSQL major version string, e.g. `"17"`.
        version: String,
        /// Cluster system identifier.
        name: String,
    },
    /// Create a new cluster.
    ClusterCreate {
        /// Request parameters.
        params: ClusterCreateParams,
    },
    /// Start a cluster.
    ClusterStart {
        /// PostgreSQL major version.
        version: String,
        /// Cluster name.
        name: String,
    },
    /// Stop a cluster.
    ClusterStop {
        /// PostgreSQL major version.
        version: String,
        /// Cluster name.
        name: String,
        /// Force stop (immediate mode).
        force: bool,
    },
    /// Restart a cluster.
    ClusterRestart {
        /// PostgreSQL major version.
        version: String,
        /// Cluster name.
        name: String,
    },
    /// Reload a cluster configuration.
    ClusterReload {
        /// PostgreSQL major version.
        version: String,
        /// Cluster name.
        name: String,
    },
    /// Rename a cluster.
    ClusterRename {
        /// PostgreSQL major version.
        version: String,
        /// Current cluster name.
        old_name: String,
        /// New cluster name.
        new_name: String,
    },
    /// Delete a cluster.
    ClusterDelete {
        /// PostgreSQL major version.
        version: String,
        /// Cluster name.
        name: String,
        /// Confirmation phrase that must equal `DELETE <name>`.
        confirmation: String,
        /// Also stop if running.
        stop_first: bool,
    },
    /// Read postgresql.conf (allowlisted path only).
    ClusterReadConfig {
        /// PostgreSQL major version.
        version: String,
        /// Cluster name.
        name: String,
    },
    /// Update allowlisted configuration keys.
    ClusterUpdateSafeConfig {
        /// PostgreSQL major version.
        version: String,
        /// Cluster name.
        name: String,
        /// Key/value pairs to apply.
        settings: Vec<ConfigSetting>,
        /// Restart if reload is insufficient.
        allow_restart: bool,
    },
    /// Read cluster log tail.
    ClusterReadLogs {
        /// PostgreSQL major version.
        version: String,
        /// Cluster name.
        name: String,
        /// Maximum lines to return.
        max_lines: u32,
        /// Optional level filter (ERROR, WARNING, …).
        level_filter: Option<String>,
        /// Optional substring search.
        search: Option<String>,
    },
    /// Report helper/service status.
    ServiceStatus,
    /// Ping / liveness.
    Ping,
}

/// Cluster creation parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterCreateParams {
    /// PostgreSQL major version.
    pub version: String,
    /// System cluster identifier.
    pub name: String,
    /// Optional human display name stored by the panel.
    pub display_name: Option<String>,
    /// TCP port.
    pub port: u16,
    /// Encoding, e.g. UTF8.
    pub encoding: String,
    /// Locale, e.g. C.UTF-8.
    pub locale: String,
    /// Enable data checksums.
    pub data_checksums: bool,
    /// Start after creation.
    pub start: bool,
    /// listen_addresses value.
    pub listen_addresses: String,
    /// Optional data directory under an allowlisted root.
    pub data_directory: Option<String>,
}

/// A single allowlisted configuration setting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSetting {
    /// Setting name.
    pub key: String,
    /// Setting value as text.
    pub value: String,
}

/// Successful or failed helper response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelperResponse {
    /// Correlation ID matching the request.
    pub request_id: Uuid,
    /// Outcome.
    pub result: HelperResult,
}

/// Result payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HelperResult {
    /// Operation succeeded.
    Ok {
        /// Typed success body.
        data: HelperOk,
    },
    /// Operation failed.
    Err {
        /// Structured error.
        error: HelperErrorBody,
    },
}

/// Success payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HelperOk {
    /// Empty success (ping, etc.).
    Empty,
    /// Cluster list.
    ClusterList {
        /// Clusters.
        clusters: Vec<ClusterSummary>,
    },
    /// Single cluster inspection.
    ClusterInspect {
        /// Detailed info.
        cluster: ClusterDetail,
    },
    /// Cluster created.
    ClusterCreated {
        /// Summary of the new cluster.
        cluster: ClusterSummary,
    },
    /// Cluster control action completed.
    ClusterAction {
        /// Version.
        version: String,
        /// Name.
        name: String,
        /// Action performed.
        action: String,
        /// Resulting status if known.
        status: Option<String>,
    },
    /// Cluster renamed.
    ClusterRenamed {
        /// Version.
        version: String,
        /// Old name.
        old_name: String,
        /// New name.
        new_name: String,
    },
    /// Cluster deleted.
    ClusterDeleted {
        /// Version.
        version: String,
        /// Name that was deleted.
        name: String,
        /// Data directory that was removed (from pg_lsclusters).
        data_directory: String,
        /// Config directory that was removed.
        config_directory: String,
    },
    /// Configuration contents / allowlisted view.
    ClusterConfig {
        /// Version.
        version: String,
        /// Name.
        name: String,
        /// Allowlisted settings currently set.
        settings: Vec<ConfigSetting>,
        /// Raw path of the config file (informational).
        config_path: String,
    },
    /// Configuration update result.
    ClusterConfigUpdated {
        /// Version.
        version: String,
        /// Name.
        name: String,
        /// Whether a restart was performed.
        restarted: bool,
        /// Backup path of previous config.
        backup_path: String,
        /// Applied settings.
        settings: Vec<ConfigSetting>,
    },
    /// Log lines.
    ClusterLogs {
        /// Version.
        version: String,
        /// Name.
        name: String,
        /// Log file path (verified).
        log_path: String,
        /// Lines.
        lines: Vec<String>,
        /// Truncated due to limits.
        truncated: bool,
    },
    /// Helper service status.
    ServiceStatus {
        /// Helper version.
        version: String,
        /// Protocol version.
        protocol_version: u32,
        /// Uptime seconds.
        uptime_secs: u64,
        /// Peer UID that is allowed.
        allowed_uid: u32,
    },
}

/// Structured helper error returned to the web process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelperErrorBody {
    /// Machine-readable code.
    pub code: HelperErrorCode,
    /// Safe user-facing message.
    pub message: String,
    /// Optional details (never secrets).
    pub details: Option<String>,
}

/// Machine-readable error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperErrorCode {
    /// Invalid input.
    InvalidInput,
    /// Cluster not found via pg_lsclusters.
    NotFound,
    /// Cluster already exists.
    AlreadyExists,
    /// Port in use.
    PortInUse,
    /// Version not installed.
    VersionNotInstalled,
    /// Insufficient disk space.
    InsufficientDisk,
    /// Operation timed out.
    Timeout,
    /// External command failed.
    CommandFailed,
    /// Configuration validation failed.
    ConfigInvalid,
    /// Permission / peer credential failure.
    Unauthorized,
    /// Protocol error.
    Protocol,
    /// Internal helper error.
    Internal,
    /// Confirmation phrase mismatch.
    ConfirmationFailed,
    /// Operation not allowed by policy.
    Forbidden,
}

/// Summary row from `pg_lsclusters`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSummary {
    /// Major version.
    pub version: String,
    /// Cluster name.
    pub name: String,
    /// Port.
    pub port: u16,
    /// Status (online, down, …).
    pub status: String,
    /// OS owner.
    pub owner: String,
    /// Data directory.
    pub data_directory: String,
    /// Log file path.
    pub log_file: String,
}

/// Detailed cluster inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterDetail {
    /// Base summary.
    pub summary: ClusterSummary,
    /// Config directory.
    pub config_directory: String,
    /// Postmaster PID if running.
    pub postmaster_pid: Option<u32>,
    /// Uptime seconds if running.
    pub uptime_secs: Option<u64>,
    /// Active connection count if queryable.
    pub connection_count: Option<u32>,
    /// Database count if queryable.
    pub database_count: Option<u32>,
    /// Total cluster size bytes if queryable.
    pub total_size_bytes: Option<u64>,
    /// Data directory disk usage bytes.
    pub data_dir_bytes: Option<u64>,
    /// Data checksums enabled.
    pub data_checksums: Option<bool>,
    /// wal_level.
    pub wal_level: Option<String>,
    /// SSL on/off.
    pub ssl: Option<bool>,
    /// listen_addresses.
    pub listen_addresses: Option<String>,
    /// max_connections.
    pub max_connections: Option<u32>,
    /// Replication slot count.
    pub replication_slot_count: Option<u32>,
    /// Last checkpoint time.
    pub last_checkpoint: Option<DateTime<Utc>>,
}

/// Errors local to encoding/decoding (not helper operational errors).
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// JSON serialization/deserialization failed.
    #[error("protocol json error: {0}")]
    Json(#[from] serde_json::Error),
    /// Frame too large.
    #[error("frame exceeds maximum size of {0} bytes")]
    FrameTooLarge(usize),
    /// Incomplete frame.
    #[error("incomplete frame")]
    Incomplete,
    /// Unsupported protocol version.
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u32),
    /// I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Encode a value to a length-prefixed JSON frame.
pub fn encode_frame<T: Serialize>(value: &T, max_bytes: usize) -> Result<Vec<u8>, ProtocolError> {
    let body = serde_json::to_vec(value)?;
    if body.len() > max_bytes {
        return Err(ProtocolError::FrameTooLarge(max_bytes));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decode a length-prefixed JSON frame from a buffer that starts at the length prefix.
pub fn decode_frame<T: for<'de> Deserialize<'de>>(
    data: &[u8],
    max_bytes: usize,
) -> Result<(T, usize), ProtocolError> {
    if data.len() < 4 {
        return Err(ProtocolError::Incomplete);
    }
    let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if len > max_bytes {
        return Err(ProtocolError::FrameTooLarge(max_bytes));
    }
    if data.len() < 4 + len {
        return Err(ProtocolError::Incomplete);
    }
    let value = serde_json::from_slice(&data[4..4 + len])?;
    Ok((value, 4 + len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_ping() {
        let env = Envelope {
            version: PROTOCOL_VERSION,
            request_id: Uuid::new_v4(),
            issued_at: Utc::now(),
            actor_user_id: Some(1),
            actor_username: Some("admin".into()),
            source_ip: Some("127.0.0.1".into()),
            op: HelperOp::Ping,
        };
        let frame = encode_frame(&env, MAX_REQUEST_BYTES).unwrap();
        let (decoded, consumed): (Envelope, _) =
            decode_frame(&frame, MAX_REQUEST_BYTES).unwrap();
        assert_eq!(consumed, frame.len());
        assert!(matches!(decoded.op, HelperOp::Ping));
    }

    #[test]
    fn rejects_oversized_frame() {
        let big = vec![0u8; 100];
        let mut frame = (100u32).to_be_bytes().to_vec();
        frame.extend_from_slice(&big);
        let err = decode_frame::<Envelope>(&frame, 50).unwrap_err();
        assert!(matches!(err, ProtocolError::FrameTooLarge(50)));
    }
}
