//! Client for the privileged updater Unix socket.

#![cfg(unix)]

use crate::error::{UpdaterError, UpdaterResult};
use crate::protocol::{UpdaterRequest, UpdaterResponse, UpdaterStatus, MAX_FRAME_BYTES};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Unprivileged client for `pgpanel-updater serve`.
#[derive(Debug, Clone)]
pub struct UpdaterClient {
    socket: PathBuf,
    timeout: Duration,
}

impl UpdaterClient {
    /// Create a client using a five-second request timeout.
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
            timeout: Duration::from_secs(5),
        }
    }

    /// Set the connection and response timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Socket path.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Fetch current daemon status.
    pub async fn status(&self) -> UpdaterResult<UpdaterStatus> {
        match self.request(UpdaterRequest::Status).await? {
            UpdaterResponse::Status { status } => Ok(status),
            UpdaterResponse::Error { message } => Err(UpdaterError::Protocol(message)),
            other => Err(UpdaterError::Protocol(format!(
                "unexpected status response: {other:?}"
            ))),
        }
    }

    /// Ask the daemon to check for releases.
    pub async fn check(&self) -> UpdaterResult<UpdaterStatus> {
        match self.request(UpdaterRequest::Check).await? {
            UpdaterResponse::Check { status, .. } => Ok(status),
            UpdaterResponse::Error { message } => Err(UpdaterError::Protocol(message)),
            other => Err(UpdaterError::Protocol(format!(
                "unexpected check response: {other:?}"
            ))),
        }
    }

    /// Start installation of the latest release.
    pub async fn install_latest(&self) -> UpdaterResult<u64> {
        self.mutation(UpdaterRequest::InstallLatest).await
    }

    /// Start installation of a specific version.
    pub async fn install_version(&self, version: impl Into<String>) -> UpdaterResult<u64> {
        self.mutation(UpdaterRequest::InstallVersion {
            version: version.into(),
        })
        .await
    }

    /// Start rollback to the previous release.
    pub async fn rollback(&self) -> UpdaterResult<u64> {
        self.mutation(UpdaterRequest::Rollback).await
    }

    async fn mutation(&self, request: UpdaterRequest) -> UpdaterResult<u64> {
        match self.request(request).await? {
            UpdaterResponse::Accepted { operation_id, .. } => Ok(operation_id),
            UpdaterResponse::Error { message } => Err(UpdaterError::Protocol(message)),
            other => Err(UpdaterError::Protocol(format!(
                "unexpected mutation response: {other:?}"
            ))),
        }
    }

    /// Send one typed request and read one typed response.
    pub async fn request(&self, request: UpdaterRequest) -> UpdaterResult<UpdaterResponse> {
        let result = tokio::time::timeout(self.timeout, async {
            let mut stream = UnixStream::connect(&self.socket).await?;
            let mut frame =
                serde_json::to_vec(&request).map_err(|e| UpdaterError::Protocol(e.to_string()))?;
            frame.push(b'\n');
            stream.write_all(&frame).await?;
            stream.shutdown().await?;

            let mut reader = BufReader::new(stream);
            let mut response = Vec::new();
            reader.read_until(b'\n', &mut response).await?;
            if response.len() > MAX_FRAME_BYTES {
                return Err(UpdaterError::Protocol("response frame too large".into()));
            }
            if response.is_empty() {
                return Err(UpdaterError::Protocol(
                    "daemon closed without response".into(),
                ));
            }
            serde_json::from_slice(response.trim_ascii())
                .map_err(|e| UpdaterError::Protocol(format!("invalid daemon response: {e}")))
        })
        .await
        .map_err(|_| UpdaterError::Protocol("updater request timed out".into()))?;
        result
    }
}
