//! Symmetric encryption for stored secrets.

use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use pgpanel_core::auth::SecretKey;
use pgpanel_core::CoreError;
use pgpanel_core::CoreResult;
use rand::RngCore;

const NONCE_LEN: usize = 12;

/// Encrypt plaintext with ChaCha20-Poly1305 using the app secret key.
pub fn encrypt(secret_key: &SecretKey, plaintext: &str) -> CoreResult<String> {
    encrypt_with_derived_key(&derive_legacy_key(secret_key), plaintext)
}

/// Encrypt plaintext with a domain-separated ChaCha20-Poly1305 key.
///
/// A separate key derivation domain prevents ciphertexts for one secret type
/// from being reused as another secret type.
pub fn encrypt_with_domain(
    secret_key: &SecretKey,
    domain: &str,
    plaintext: &str,
) -> CoreResult<String> {
    encrypt_with_derived_key(&derive_key(secret_key, domain), plaintext)
}

fn encrypt_with_derived_key(key_bytes: &[u8; 32], plaintext: &str) -> CoreResult<String> {
    let cipher = ChaCha20Poly1305::new_from_slice(key_bytes)
        .map_err(|e| CoreError::Internal(format!("cipher init: {e}")))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| CoreError::Internal(format!("encrypt: {e}")))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(hex::encode(out))
}

/// Decrypt a hex-encoded ciphertext.
pub fn decrypt(secret_key: &SecretKey, encoded: &str) -> CoreResult<String> {
    match decrypt_with_domain(secret_key, "generic-secret-v1", encoded) {
        Ok(plaintext) => Ok(plaintext),
        Err(_) => decrypt_with_derived_key(&derive_legacy_key(secret_key), encoded),
    }
}

/// Decrypt a hex-encoded, domain-separated ciphertext.
pub fn decrypt_with_domain(
    secret_key: &SecretKey,
    domain: &str,
    encoded: &str,
) -> CoreResult<String> {
    decrypt_with_derived_key(&derive_key(secret_key, domain), encoded)
}

fn decrypt_with_derived_key(key_bytes: &[u8; 32], encoded: &str) -> CoreResult<String> {
    let data = hex::decode(encoded).map_err(|e| CoreError::Internal(format!("hex decode: {e}")))?;
    if data.len() < NONCE_LEN {
        return Err(CoreError::Internal("ciphertext too short".into()));
    }
    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new_from_slice(key_bytes)
        .map_err(|e| CoreError::Internal(format!("cipher init: {e}")))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CoreError::Auth("failed to decrypt secret".into()))?;
    String::from_utf8(plaintext).map_err(|e| CoreError::Internal(format!("utf8: {e}")))
}

fn derive_legacy_key(secret_key: &SecretKey) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"pgpanel-secret-v1");
    hasher.update(secret_key.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

fn derive_key(secret_key: &SecretKey, domain: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"pgpanel-secret-key-v2");
    hasher.update(domain.as_bytes());
    hasher.update([0u8]);
    hasher.update(secret_key.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgpanel_core::auth::SecretKey;

    #[test]
    fn roundtrip() {
        let key = SecretKey::new("dev-secret-key-change-me-in-production-32b".into()).unwrap();
        let enc = encrypt(&key, "super-secret-password").unwrap();
        let dec = decrypt(&key, &enc).unwrap();
        assert_eq!(dec, "super-secret-password");
    }

    #[test]
    fn domain_separated_roundtrip_and_rejection() {
        let key = SecretKey::new("dev-secret-key-change-me-in-production-32b".into()).unwrap();
        let enc = encrypt_with_domain(&key, "totp-secret-v1", "JBSWY3DPEHPK3PXP").unwrap();
        assert_eq!(
            decrypt_with_domain(&key, "totp-secret-v1", &enc).unwrap(),
            "JBSWY3DPEHPK3PXP"
        );
        assert!(decrypt_with_domain(&key, "other-secret-v1", &enc).is_err());
    }
}
