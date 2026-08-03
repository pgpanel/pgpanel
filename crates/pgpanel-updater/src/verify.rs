//! SHA-256 checksum and Ed25519 signature verification.

use crate::error::{UpdaterError, UpdaterResult};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Component;
use std::path::Path;

/// Compute SHA-256 hex digest of a file.
pub fn sha256_file(path: &Path) -> UpdaterResult<String> {
    let bytes = std::fs::read(path)?;
    Ok(sha256_bytes(&bytes))
}

/// Compute SHA-256 hex digest of bytes.
pub fn sha256_bytes(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    hex::encode(digest)
}

/// Parse `SHA256SUMS` content into (filename, hex digest) pairs.
pub fn parse_sha256sums(content: &str) -> UpdaterResult<Vec<(String, String)>> {
    let mut entries = Vec::new();
    let mut filenames = HashSet::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let hash = parts
            .next()
            .ok_or_else(|| UpdaterError::Verify("malformed SHA256SUMS line".into()))?;
        let filename = parts
            .next()
            .ok_or_else(|| UpdaterError::Verify("malformed SHA256SUMS line".into()))?;
        if parts.next().is_some() {
            return Err(UpdaterError::Verify(
                "SHA256SUMS line has unexpected extra fields".into(),
            ));
        }
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(UpdaterError::Verify(format!(
                "invalid SHA-256 digest for {filename}"
            )));
        }
        let path = Path::new(filename);
        if path.is_absolute()
            || path.components().count() != 1
            || !matches!(path.components().next(), Some(Component::Normal(_)))
            || filename.contains('\\')
        {
            return Err(UpdaterError::Verify(format!(
                "invalid path in SHA256SUMS: {filename}"
            )));
        }
        if !filenames.insert(filename) {
            return Err(UpdaterError::Verify(format!(
                "duplicate checksum entry: {filename}"
            )));
        }
        entries.push((filename.to_string(), hash.to_lowercase()));
    }
    if entries.is_empty() {
        return Err(UpdaterError::Verify("SHA256SUMS is empty".into()));
    }
    Ok(entries)
}

/// Verify exactly the downloaded subset of files listed in SHA256SUMS.
///
/// The manifest may contain checksums for both architecture archives, while
/// the updater intentionally downloads only the selected archive.
pub fn verify_sha256sums(
    content: &str,
    base_dir: &Path,
    required_files: &[&str],
) -> UpdaterResult<()> {
    let entries = parse_sha256sums(content)?;
    let mut required = HashSet::new();
    for filename in required_files {
        let path = Path::new(filename);
        if path.is_absolute()
            || path.components().count() != 1
            || !matches!(path.components().next(), Some(Component::Normal(_)))
        {
            return Err(UpdaterError::Verify(format!(
                "invalid required checksum path: {filename}"
            )));
        }
        if !required.insert(*filename) {
            return Err(UpdaterError::Verify(format!(
                "duplicate required checksum path: {filename}"
            )));
        }
    }
    let mut found = HashSet::new();
    for (filename, expected) in entries {
        if !required.contains(filename.as_str()) {
            continue;
        }
        if !found.insert(filename.clone()) {
            return Err(UpdaterError::Verify(format!(
                "duplicate checksum entry: {filename}"
            )));
        }
        let path = base_dir.join(&filename);
        if !path.is_file() {
            return Err(UpdaterError::Verify(format!(
                "artifact missing for checksum verification: {filename}"
            )));
        }
        let actual = sha256_file(&path)?;
        if actual != expected {
            return Err(UpdaterError::Verify(format!(
                "checksum mismatch for {filename}: expected {expected}, got {actual}"
            )));
        }
    }
    for filename in required {
        if !found.contains(filename) {
            return Err(UpdaterError::Verify(format!(
                "required file missing from SHA256SUMS: {filename}"
            )));
        }
    }
    Ok(())
}

/// Load a pinned Ed25519 verifying key from raw 32-byte hex file.
pub fn load_verifying_key(path: &Path) -> UpdaterResult<VerifyingKey> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| UpdaterError::Verify(format!("failed to read public key: {e}")))?;
    let trimmed = text.trim();
    if trimmed.starts_with("-----") {
        return Err(UpdaterError::Verify(
            "PEM public keys are not supported; use raw 32-byte hex".into(),
        ));
    }
    let key_bytes =
        hex::decode(trimmed).map_err(|e| UpdaterError::Verify(format!("invalid hex key: {e}")))?;
    let array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| UpdaterError::Verify("public key must be 32 bytes".into()))?;
    VerifyingKey::from_bytes(&array)
        .map_err(|e| UpdaterError::Verify(format!("invalid Ed25519 public key: {e}")))
}

/// Parse detached signature bytes (hex or raw).
pub fn parse_signature_bytes(data: &[u8]) -> UpdaterResult<Signature> {
    let sig_bytes = if data
        .iter()
        .all(|b| b.is_ascii_hexdigit() || b.is_ascii_whitespace())
    {
        let hex_str = String::from_utf8_lossy(data)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();
        hex::decode(hex_str)
            .map_err(|e| UpdaterError::Verify(format!("invalid hex signature: {e}")))?
    } else {
        data.to_vec()
    };
    let array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| UpdaterError::Verify("signature must be 64 bytes".into()))?;
    Ok(Signature::from_bytes(&array))
}

/// Verify detached Ed25519 signature over `message` using `key`.
pub fn verify_ed25519_signature(
    key: &VerifyingKey,
    message: &[u8],
    signature: &Signature,
) -> UpdaterResult<()> {
    key.verify(message, signature)
        .map_err(|_| UpdaterError::Verify("Ed25519 signature verification failed".into()))
}

/// Verify `SHA256SUMS.sig` against `SHA256SUMS` content with pinned public key.
pub fn verify_sha256sums_signature(
    public_key_path: &Path,
    sums_content: &[u8],
    sig_content: &[u8],
) -> UpdaterResult<()> {
    let key = load_verifying_key(public_key_path)?;
    let signature = parse_signature_bytes(sig_content)?;
    verify_ed25519_signature(&key, sums_content, &signature)
}

/// Verify a downloaded artifact against SHA256SUMS entry.
pub fn verify_artifact_checksum(
    artifact_path: &Path,
    sums_content: &str,
    artifact_name: &str,
) -> UpdaterResult<()> {
    let entries = parse_sha256sums(sums_content)?;
    let expected = entries
        .iter()
        .find(|(name, _)| name == artifact_name)
        .ok_or_else(|| {
            UpdaterError::Verify(format!("artifact {artifact_name} not listed in SHA256SUMS"))
        })?
        .1
        .clone();
    let actual = sha256_file(artifact_path)?;
    if actual != expected {
        return Err(UpdaterError::Verify(format!(
            "checksum mismatch for {artifact_name}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn parse_sha256sums_works() {
        let content = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  file.tar.gz\n\
                       bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  manifest.json\n";
        let entries = parse_sha256sums(content).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "file.tar.gz");
    }

    #[test]
    fn ed25519_sign_and_verify_roundtrip() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let message = b"test SHA256SUMS content\n";
        let signature = signing_key.sign(message);

        verify_ed25519_signature(&verifying_key, message, &signature).unwrap();
    }

    #[test]
    fn ed25519_verify_rejects_tampered_message() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let signature = signing_key.sign(b"original");
        let err = verify_ed25519_signature(&verifying_key, b"tampered", &signature).unwrap_err();
        assert!(matches!(err, UpdaterError::Verify(_)));
    }

    #[test]
    fn verify_sha256sums_signature_with_hex_key_file() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let sums = b"deadbeef  pgpanel-linux-amd64.tar.gz\n";

        let signature = signing_key.sign(sums);

        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("signing.pub");
        std::fs::write(&key_path, hex::encode(verifying_key.to_bytes())).unwrap();
        let sig_hex = hex::encode(signature.to_bytes());
        verify_sha256sums_signature(&key_path, sums, sig_hex.as_bytes()).unwrap();
    }

    #[test]
    fn verify_artifact_checksum_matches_file() {
        let dir = TempDir::new().unwrap();
        let artifact = dir.path().join("pgpanel-linux-amd64.tar.gz");
        let mut f = std::fs::File::create(&artifact).unwrap();
        f.write_all(b"test artifact bytes").unwrap();
        drop(f);

        let hash = sha256_file(&artifact).unwrap();
        let sums = format!("{hash}  pgpanel-linux-amd64.tar.gz\n");
        verify_artifact_checksum(&artifact, &sums, "pgpanel-linux-amd64.tar.gz").unwrap();
    }

    #[test]
    fn verify_sha256sums_checks_required_subset() {
        let dir = TempDir::new().unwrap();
        let selected = dir.path().join("selected.tar.gz");
        let manifest = dir.path().join("manifest.json");
        std::fs::write(&selected, b"selected").unwrap();
        std::fs::write(&manifest, b"manifest").unwrap();
        let selected_hash = sha256_file(&selected).unwrap();
        let manifest_hash = sha256_file(&manifest).unwrap();
        let sums = format!(
            "{selected_hash}  selected.tar.gz\n\
             {}  other-arch.tar.gz\n\
             {manifest_hash}  manifest.json\n",
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );
        verify_sha256sums(&sums, dir.path(), &["selected.tar.gz", "manifest.json"]).unwrap();
    }

    #[test]
    fn checksum_parser_rejects_duplicates_and_paths() {
        let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert!(parse_sha256sums(&format!("{hash}  file\n{hash}  file\n")).is_err());
        assert!(parse_sha256sums(&format!("{hash}  ../file\n")).is_err());
    }

    #[test]
    fn verify_artifact_checksum_rejects_mismatch() {
        let dir = TempDir::new().unwrap();
        let artifact = dir.path().join("pgpanel-linux-amd64.tar.gz");
        std::fs::write(&artifact, b"data").unwrap();
        let sums = "0000000000000000000000000000000000000000000000000000000000000000  pgpanel-linux-amd64.tar.gz\n";
        let err =
            verify_artifact_checksum(&artifact, sums, "pgpanel-linux-amd64.tar.gz").unwrap_err();
        assert!(matches!(err, UpdaterError::Verify(_)));
    }
}
