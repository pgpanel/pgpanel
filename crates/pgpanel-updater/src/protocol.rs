//! Typed wire protocol for the privileged updater daemon.

use crate::deploy::DeployStatus;
use crate::github::ReleaseInfo;
use crate::progress::UpdatePhase;
use serde::{Deserialize, Serialize};

/// Maximum newline-delimited JSON frame accepted from a client.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Requests accepted by the updater daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum UpdaterRequest {
    /// Return daemon and deployment status.
    Status,
    /// Check GitHub for the latest release.
    Check,
    /// Install the latest suitable release in the background.
    InstallLatest,
    /// Install a specific release in the background.
    InstallVersion { version: String },
    /// Roll back to the previous release in the background.
    Rollback,
}

/// Release metadata exposed to unprivileged clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseMetadata {
    /// Normalized version.
    pub version: String,
    /// GitHub tag.
    pub tag: String,
    /// Published timestamp supplied by GitHub.
    pub published_at: String,
}

impl From<&ReleaseInfo> for ReleaseMetadata {
    fn from(info: &ReleaseInfo) -> Self {
        Self {
            version: info.version.clone(),
            tag: info.tag.clone(),
            published_at: info.release.published_at.clone(),
        }
    }
}

/// Daemon operation phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonPhase {
    /// No operation is currently running.
    Idle,
    /// A release check is running.
    Checking,
    /// An installation is running.
    Installing,
    /// A rollback is running.
    RollingBack,
}

/// Result of a completed background operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationOutcome {
    /// Daemon-assigned operation identifier.
    pub operation_id: u64,
    /// Installed or restored version, when known.
    pub version: Option<String>,
    /// Unix timestamp when the operation completed.
    pub finished_at: u64,
}

/// A bounded, daemon-wide progress event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressRecord {
    /// Monotonically increasing daemon event identifier.
    pub id: u64,
    /// Operation identifier that emitted the event.
    pub operation_id: u64,
    /// Update phase.
    pub phase: UpdatePhase,
    /// Human-readable message.
    pub message: String,
    /// Optional structured detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
    /// Progress fraction, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
}

/// Serializable state maintained by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdaterStatus {
    /// Current daemon operation phase.
    pub phase: DaemonPhase,
    /// Most recently discovered release.
    pub latest_available: Option<ReleaseMetadata>,
    /// Unix timestamp of the last completed check.
    pub last_check: Option<u64>,
    /// Last successful background operation.
    pub last_success: Option<OperationOutcome>,
    /// Last error, without credentials or request contents.
    pub last_error: Option<String>,
    /// Recent progress events, bounded by the daemon.
    pub progress: Vec<ProgressRecord>,
    /// Current blue-green deployment status.
    pub deployment: Option<DeployStatus>,
}

/// Responses returned by the updater daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum UpdaterResponse {
    /// Current state.
    Status { status: UpdaterStatus },
    /// Result of a release check.
    Check {
        latest: Option<ReleaseMetadata>,
        status: UpdaterStatus,
    },
    /// Background operation accepted.
    Accepted {
        operation_id: u64,
        status: UpdaterStatus,
    },
    /// Request rejected or operation failed to start.
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip_is_typed() {
        let request = UpdaterRequest::InstallVersion {
            version: "1.2.3".into(),
        };
        let encoded = serde_json::to_vec(&request).unwrap();
        let decoded: UpdaterRequest = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, request);
        assert!(!String::from_utf8(encoded).unwrap().contains("command"));
    }

    #[test]
    fn response_roundtrip_is_typed() {
        let response = UpdaterResponse::Accepted {
            operation_id: 7,
            status: UpdaterStatus {
                phase: DaemonPhase::Installing,
                latest_available: None,
                last_check: None,
                last_success: None,
                last_error: None,
                progress: Vec::new(),
                deployment: None,
            },
        };
        let encoded = serde_json::to_vec(&response).unwrap();
        let decoded: UpdaterResponse = serde_json::from_slice(&encoded).unwrap();
        assert!(matches!(
            decoded,
            UpdaterResponse::Accepted {
                operation_id: 7,
                ..
            }
        ));
    }
}
