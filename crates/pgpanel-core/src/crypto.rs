//! Password hashing (Argon2id) and credential encryption (AES-256-GCM).

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

const NONCE_LEN: usize = 12;
const PASSWORD_BYTES: usize = 24;

/// Derive a 32-byte AES key from the master key material.
fn derive_key(master: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(master.as_bytes());
    hasher.update(b"pgpanel-aes-v1");
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

/// Generate a strong random password (24 random bytes, base64url-ish printable).
pub fn generate_password() -> SecretString {
    let mut bytes = Zeroizing::new([0u8; PASSWORD_BYTES]);
    rand::thread_rng().fill_bytes(bytes.as_mut());
    let encoded = B64.encode(bytes.as_ref());
    // strip padding for cleaner passwords
    SecretString::from(encoded.trim_end_matches('=').to_string())
}

/// Generate a random session token (32 bytes hex).
pub fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Generate CSRF token.
pub fn generate_csrf_token() -> String {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Generate a correlation / request ID.
pub fn generate_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Hash an admin password with Argon2id.
pub fn hash_password(password: &SecretString) -> Result<String> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    let params = Params::new(19_456, 2, 1, None)
        .map_err(|e| Error::Crypto(format!("argon2 params: {e}")))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let hash = argon2
        .hash_password(password.expose_secret().as_bytes(), &salt)
        .map_err(|e| Error::Crypto(format!("argon2 hash: {e}")))?;
    Ok(hash.to_string())
}

/// Verify an admin password against Argon2id hash.
pub fn verify_password(password: &SecretString, hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| Error::Crypto(format!("invalid password hash: {e}")))?;
    let argon2 = Argon2::default();
    Ok(argon2
        .verify_password(password.expose_secret().as_bytes(), &parsed)
        .is_ok())
}

/// Encrypt a secret with AES-256-GCM. Output: base64(nonce || ciphertext).
pub fn encrypt_secret(master_key: &str, plaintext: &SecretString) -> Result<String> {
    let key = derive_key(master_key);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| Error::Crypto(format!("cipher init: {e}")))?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.expose_secret().as_bytes())
        .map_err(|e| Error::Crypto(format!("encrypt: {e}")))?;

    let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Ok(B64.encode(combined))
}

/// Decrypt a secret encrypted with [`encrypt_secret`].
pub fn decrypt_secret(master_key: &str, encoded: &str) -> Result<SecretString> {
    let combined = B64
        .decode(encoded)
        .map_err(|e| Error::Crypto(format!("base64 decode: {e}")))?;
    if combined.len() < NONCE_LEN + 16 {
        return Err(Error::Crypto("ciphertext too short".into()));
    }

    let key = derive_key(master_key);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| Error::Crypto(format!("cipher init: {e}")))?;

    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| Error::Crypto("decrypt failed (wrong key or corrupted data)".into()))?;

    let s = String::from_utf8(plaintext)
        .map_err(|_| Error::Crypto("decrypted data is not valid UTF-8".into()))?;
    Ok(SecretString::from(s))
}

/// Check password strength for manually provided passwords.
pub fn validate_password_strength(password: &str) -> Result<()> {
    if password.len() < 16 {
        return Err(Error::Validation(
            "password must be at least 16 characters".into(),
        ));
    }
    if password.len() > 256 {
        return Err(Error::Validation(
            "password must be at most 256 characters".into(),
        ));
    }
    let has_letter = password.chars().any(|c| c.is_ascii_alphabetic());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    if !has_letter || !has_digit {
        return Err(Error::Validation(
            "password must contain letters and digits".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip_hash() {
        let pw = SecretString::from("super-secret-admin-password-99".to_string());
        let hash = hash_password(&pw).unwrap();
        assert!(verify_password(&pw, &hash).unwrap());
        let wrong = SecretString::from("wrong-password-zzzzzzzzzzzz".to_string());
        assert!(!verify_password(&wrong, &hash).unwrap());
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let master = "test-master-key-at-least-32-chars!!";
        let secret = SecretString::from("postgres-password-value".to_string());
        let enc = encrypt_secret(master, &secret).unwrap();
        let dec = decrypt_secret(master, &enc).unwrap();
        assert_eq!(dec.expose_secret(), secret.expose_secret());
    }

    #[test]
    fn generate_password_length() {
        let pw = generate_password();
        assert!(pw.expose_secret().len() >= 24);
    }

    #[test]
    fn password_strength() {
        assert!(validate_password_strength("short").is_err());
        assert!(validate_password_strength("alllettersonlyhere").is_err());
        assert!(validate_password_strength("validpassword1234").is_ok());
    }
}
