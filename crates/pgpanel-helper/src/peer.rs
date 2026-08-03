//! Unix socket peer credential validation.

use nix::sys::socket::{getsockopt, AddressFamily, SockFlag, SockType};
use nix::unistd::getuid;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use tracing::debug;

/// Peer credential snapshot (UID only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    /// Effective UID of the connected peer.
    pub uid: u32,
}

/// Read peer credentials from an accepted Unix socket connection.
pub fn peer_credentials(stream: &UnixStream) -> std::io::Result<PeerCredentials> {
    let fd = stream.as_raw_fd();

    #[cfg(target_os = "linux")]
    {
        use nix::sys::socket::sockopt::PeerCred;
        let cred = getsockopt(fd, PeerCred).map_err(std::io::Error::other)?;
        Ok(PeerCredentials {
            uid: cred.uid(),
        })
    }

    #[cfg(target_os = "macos")]
    {
        use nix::sys::socket::sockopt::LocalPeerCred;
        let cred = getsockopt(fd, LocalPeerCred).map_err(std::io::Error::other)?;
        Ok(PeerCredentials {
            uid: cred.uid(),
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = fd;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "peer credential validation is only supported on Linux and macOS",
        ))
    }
}

/// Returns `true` when the peer UID is permitted to invoke helper operations.
pub fn is_peer_allowed(peer_uid: u32, allowed_uid: u32, dev_mode: bool) -> bool {
    if peer_uid == allowed_uid {
        return true;
    }
    if dev_mode && peer_uid == getuid().as_raw() {
        debug!(
            peer_uid,
            allowed_uid,
            "allowing peer in dev mode (same UID as helper)"
        );
        return true;
    }
    false
}

/// Resolve the configured allowed UID, optionally looking up the `pgpanel` system user.
pub fn resolve_allowed_uid(explicit: Option<u32>, dev_mode: bool) -> Result<u32, String> {
    if let Some(uid) = explicit {
        return Ok(uid);
    }
    if dev_mode {
        return Ok(getuid().as_raw());
    }
    nix::unistd::User::from_name("pgpanel")
        .map_err(|e| format!("failed to look up pgpanel user: {e}"))?
        .map(|u| u.uid.as_raw())
        .ok_or_else(|| {
            "pgpanel user not found; pass --allowed-uid explicitly".to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn same_allowed_uid_permitted() {
        assert!(is_peer_allowed(1000, 1000, false));
    }

    #[test]
    fn different_uid_rejected_in_production() {
        assert!(!is_peer_allowed(1001, 1000, false));
    }

    #[test]
    fn dev_mode_allows_own_uid() {
        let own = getuid().as_raw();
        assert!(is_peer_allowed(own, 9999, true));
    }

    #[test]
    fn reads_peer_credentials_from_socket_pair() {
        let listener = UnixListener::bind("/tmp/pgpanel-helper-peer-test.sock").unwrap();
        let client = UnixStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let cred = peer_credentials(&server).expect("peer creds");
        let client_cred = peer_credentials(&client).expect("client creds");
        assert_eq!(cred.uid, client_cred.uid);
        assert_eq!(cred.uid, getuid().as_raw());
        drop(listener);
        let _ = std::fs::remove_file("/tmp/pgpanel-helper-peer-test.sock");
    }
}
