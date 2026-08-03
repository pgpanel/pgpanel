//! Unix socket client for the privileged helper daemon.

use pgpanel_core::config::Config;
use pgpanel_protocol::{
    decode_frame, encode_frame, Envelope, HelperOp, HelperResponse, HelperResult,
    MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, PROTOCOL_VERSION,
};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::timeout;
use tracing::{instrument, warn};
use uuid::Uuid;

/// Client for helper operations over a Unix domain socket.
#[derive(Clone, Debug)]
pub struct HelperClient {
    socket_path: std::path::PathBuf,
    default_timeout: Duration,
}

impl HelperClient {
    /// Create from configuration.
    pub fn new(config: &Config) -> Self {
        Self {
            socket_path: config.paths.helper_socket.clone(),
            default_timeout: Duration::from_secs(config.timeouts.helper_default_secs),
        }
    }

    /// Check if the helper socket is reachable.
    pub async fn is_ready(&self) -> bool {
        Path::new(&self.socket_path).exists() && self.ping().await.is_ok()
    }

    /// Send a ping operation.
    pub async fn ping(&self) -> Result<(), HelperClientError> {
        let resp = self.call(HelperOp::Ping, None, None, None, None).await?;
        match resp.result {
            HelperResult::Ok { .. } => Ok(()),
            HelperResult::Err { error } => Err(HelperClientError::Helper(error.message)),
        }
    }

    /// Execute a helper operation.
    #[instrument(skip(self, op), fields(request_id))]
    pub async fn call(
        &self,
        op: HelperOp,
        actor_user_id: Option<i64>,
        actor_username: Option<String>,
        source_ip: Option<String>,
        op_timeout: Option<Duration>,
    ) -> Result<HelperResponse, HelperClientError> {
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));

        let envelope = Envelope {
            version: PROTOCOL_VERSION,
            request_id,
            issued_at: chrono::Utc::now(),
            actor_user_id,
            actor_username,
            source_ip,
            op,
        };

        let frame =
            encode_frame(&envelope, MAX_REQUEST_BYTES).map_err(HelperClientError::Protocol)?;
        let timeout_dur = op_timeout.unwrap_or(self.default_timeout);

        timeout(timeout_dur, self.send_and_receive(&frame))
            .await
            .map_err(|_| HelperClientError::Timeout)?
    }

    async fn send_and_receive(&self, frame: &[u8]) -> Result<HelperResponse, HelperClientError> {
        let mut stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            warn!(path = %self.socket_path.display(), error = %e, "helper socket connect failed");
            HelperClientError::Connect(e.to_string())
        })?;

        stream
            .write_all(frame)
            .await
            .map_err(|e| HelperClientError::Connect(e.to_string()))?;
        stream
            .flush()
            .await
            .map_err(|e| HelperClientError::Connect(e.to_string()))?;

        let mut buf = Vec::with_capacity(4096);
        let mut tmp = [0u8; 4096];
        loop {
            let n = stream
                .read(&mut tmp)
                .await
                .map_err(|e| HelperClientError::Connect(e.to_string()))?;
            if n == 0 {
                if buf.is_empty() {
                    return Err(HelperClientError::Closed);
                }
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            match decode_frame::<HelperResponse>(&buf, MAX_RESPONSE_BYTES) {
                Ok((resp, consumed)) => {
                    if consumed == buf.len() {
                        return Ok(resp);
                    }
                    buf.drain(..consumed);
                    return Ok(resp);
                }
                Err(pgpanel_protocol::ProtocolError::Incomplete) => continue,
                Err(e) => return Err(HelperClientError::Protocol(e)),
            }
        }
        Err(HelperClientError::Closed)
    }
}

/// Helper client errors.
#[derive(Debug, thiserror::Error)]
pub enum HelperClientError {
    #[error("helper connect failed: {0}")]
    Connect(String),
    #[error("helper connection closed")]
    Closed,
    #[error("helper request timed out")]
    Timeout,
    #[error("helper error: {0}")]
    Helper(String),
    #[error("protocol error: {0}")]
    Protocol(#[from] pgpanel_protocol::ProtocolError),
}
