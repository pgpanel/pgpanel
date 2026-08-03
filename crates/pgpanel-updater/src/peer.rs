//! Unix socket peer credential validation.

#![cfg(unix)]

use nix::sys::socket::getsockopt;
use nix::unistd::getuid;
use std::os::fd::AsFd;

/// Peer credential snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    /// Effective UID of the connected peer.
    pub uid: u32,
}

/// Read credentials from an accepted Unix stream.
#[cfg(target_os = "linux")]
pub fn peer_credentials<F: AsFd>(stream: &F) -> std::io::Result<PeerCredentials> {
    use nix::sys::socket::sockopt::PeerCred;
    let cred = getsockopt(stream, PeerCred).map_err(std::io::Error::other)?;
    Ok(PeerCredentials { uid: cred.uid() })
}

/// Read credentials from an accepted Unix stream.
#[cfg(target_os = "macos")]
pub fn peer_credentials<F: AsFd>(stream: &F) -> std::io::Result<PeerCredentials> {
    use nix::sys::socket::sockopt::LocalPeerCred;
    let cred = getsockopt(stream, LocalPeerCred).map_err(std::io::Error::other)?;
    Ok(PeerCredentials { uid: cred.uid() })
}

/// Read credentials from an accepted Unix stream.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn peer_credentials<F: AsFd>(stream: &F) -> std::io::Result<PeerCredentials> {
    let _ = stream;
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "peer credential validation is only supported on Linux and macOS",
    ))
}

/// Return whether a peer UID is allowed.
pub fn is_peer_allowed(peer_uid: u32, allowed_uid: u32, dev_mode: bool) -> bool {
    peer_uid == allowed_uid || (dev_mode && peer_uid == getuid().as_raw())
}

/// Resolve the configured peer UID.
pub fn resolve_allowed_uid(explicit: Option<u32>, dev_mode: bool) -> Result<u32, String> {
    if let Some(uid) = explicit {
        return Ok(uid);
    }
    if dev_mode {
        return Ok(getuid().as_raw());
    }
    nix::unistd::User::from_name("pgpanel")
        .map_err(|e| format!("failed to look up pgpanel user: {e}"))?
        .map(|user| user.uid.as_raw())
        .ok_or_else(|| "pgpanel user not found; pass --allowed-uid explicitly".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_policy_is_exact_in_production() {
        assert!(is_peer_allowed(1000, 1000, false));
        assert!(!is_peer_allowed(1001, 1000, false));
    }

    #[test]
    fn dev_policy_allows_only_own_uid() {
        let own = getuid().as_raw();
        assert!(is_peer_allowed(own, 9999, true));
        assert!(!is_peer_allowed(own.saturating_add(1), 9999, true));
    }
}
