//! Unix domain socket server with framed JSON protocol.

use pgpanel_protocol::{
    decode_frame, encode_frame, Envelope, HelperErrorCode, HelperResponse, MAX_REQUEST_BYTES,
    MAX_RESPONSE_BYTES, PROTOCOL_VERSION,
};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::cluster::helper_err;
use crate::ops::{dispatch, protocol_error_response, HelperState};
use crate::peer::{is_peer_allowed, peer_credentials};

/// Run the helper Unix socket server until shutdown.
pub async fn run_server(state: Arc<HelperState>, socket_path: &Path) -> std::io::Result<()> {
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if socket_path.exists() {
        tokio::fs::remove_file(socket_path).await?;
    }

    let listener = UnixListener::bind(socket_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The helper service runs as root with primary group `pgpanel`.
        // Group write access lets the unprivileged web process connect;
        // SO_PEERCRED still enforces the exact allowed UID after connect.
        let perms = std::fs::Permissions::from_mode(0o660);
        tokio::fs::set_permissions(socket_path, perms).await?;
    }

    info!(
        socket = %socket_path.display(),
        allowed_uid = state.allowed_uid,
        dev_mode = state.dev_mode,
        protocol_version = PROTOCOL_VERSION,
        "pgpanel-helper listening"
    );

    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(state, stream).await {
                warn!(error = %e, "connection handler error");
            }
        });
    }
}

async fn handle_connection(state: Arc<HelperState>, mut stream: UnixStream) -> std::io::Result<()> {
    let creds = match peer_credentials(&stream) {
        Ok(c) => c,
        Err(e) => {
            if state.dev_mode {
                warn!(
                    error = %e,
                    "failed to read peer credentials; allowing in --dev mode"
                );
                // Proceed with current process UID as peer for local testing.
                let uid = nix::unistd::getuid().as_raw();
                if !is_peer_allowed(uid, state.allowed_uid, state.dev_mode) {
                    return Ok(());
                }
                return handle_authorized(state, stream).await;
            }
            warn!(error = %e, "failed to read peer credentials");
            return Ok(());
        }
    };

    if !is_peer_allowed(creds.uid, state.allowed_uid, state.dev_mode) {
        warn!(
            peer_uid = creds.uid,
            allowed_uid = state.allowed_uid,
            "rejected unauthorized peer"
        );
        let request_id = Uuid::new_v4();
        let response = protocol_error_response(
            request_id,
            helper_err(
                HelperErrorCode::Unauthorized,
                "peer UID is not authorized",
                Some(format!("peer_uid={}", creds.uid)),
            ),
        );
        let _ = write_response(&mut stream, &response).await;
        return Ok(());
    }

    handle_authorized(state, stream).await
}

async fn handle_authorized(state: Arc<HelperState>, mut stream: UnixStream) -> std::io::Result<()> {
    debug!("accepted peer connection");
    serve_stream(state, &mut stream).await
}

async fn serve_stream(state: Arc<HelperState>, stream: &mut UnixStream) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(4096);

    loop {
        let mut chunk = [0u8; 4096];
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            if buf.is_empty() {
                return Ok(());
            }
            break;
        }
        buf.extend_from_slice(&chunk[..n]);

        loop {
            match decode_frame::<Envelope>(&buf, MAX_REQUEST_BYTES) {
                Ok((envelope, consumed)) => {
                    buf.drain(..consumed);
                    let response = dispatch(&state, envelope).await;
                    write_response(stream, &response).await?;
                }
                Err(pgpanel_protocol::ProtocolError::Incomplete) => break,
                Err(e) => {
                    error!(error = %e, "protocol decode error");
                    let response = protocol_error_response(
                        Uuid::new_v4(),
                        helper_err(
                            HelperErrorCode::Protocol,
                            "invalid request frame",
                            Some(e.to_string()),
                        ),
                    );
                    write_response(stream, &response).await?;
                    return Ok(());
                }
            }
        }
    }

    if !buf.is_empty() {
        let response = protocol_error_response(
            Uuid::new_v4(),
            helper_err(
                HelperErrorCode::Protocol,
                "incomplete request frame at connection close",
                None,
            ),
        );
        write_response(stream, &response).await?;
    }

    Ok(())
}

async fn write_response(stream: &mut UnixStream, response: &HelperResponse) -> std::io::Result<()> {
    let frame = encode_frame(response, MAX_RESPONSE_BYTES).map_err(std::io::Error::other)?;
    stream.write_all(&frame).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditWriter;
    use pgpanel_core::config::Config;
    use pgpanel_protocol::{HelperOp, HelperResult};
    use std::time::Instant;

    #[tokio::test]
    async fn ping_roundtrip_over_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("helper.sock");
        let mut cfg = Config::dev_default();
        cfg.paths.helper_socket = sock.clone();

        let state = Arc::new(HelperState {
            config: cfg.clone(),
            allowed_uid: crate::peer::resolve_allowed_uid(None, true).unwrap(),
            dev_mode: true,
            started_at: Instant::now(),
            audit: AuditWriter::new(None),
        });

        let listener_state = Arc::clone(&state);
        let sock_path = sock.clone();
        let server = tokio::spawn(async move { run_server(listener_state, &sock_path).await });

        // Wait for socket to appear.
        let mut ready = false;
        for _ in 0..100 {
            if sock.exists() {
                ready = true;
                break;
            }
            if server.is_finished() {
                match server.await {
                    Ok(Ok(())) => panic!("helper server exited cleanly before listen"),
                    Ok(Err(e)) => panic!("helper server failed before listen: {e}"),
                    Err(e) => panic!("helper server task join error: {e}"),
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(ready, "helper socket was not created at {}", sock.display());

        let mut client = match UnixStream::connect(&sock).await {
            Ok(c) => c,
            Err(e) => {
                server.abort();
                panic!("connect to helper socket failed: {e}");
            }
        };
        let envelope = Envelope {
            version: PROTOCOL_VERSION,
            request_id: Uuid::new_v4(),
            issued_at: chrono::Utc::now(),
            actor_user_id: None,
            actor_username: None,
            source_ip: None,
            op: HelperOp::Ping,
        };
        let frame = encode_frame(&envelope, MAX_REQUEST_BYTES).unwrap();
        client.write_all(&frame).await.unwrap();
        client.flush().await.unwrap();

        let mut buf = vec![0u8; 4096];
        let n = client.read(&mut buf).await.unwrap();
        let (response, _): (HelperResponse, _) =
            decode_frame(&buf[..n], MAX_RESPONSE_BYTES).unwrap();
        assert!(matches!(response.result, HelperResult::Ok { .. }));

        server.abort();
    }
}
