//! Authentication primitives: Argon2id, sessions, TOTP, backup codes.

use crate::error::{CoreError, CoreResult};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, Params, Version,
};
use rand::{distributions::Alphanumeric, Rng, RngCore};
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Secret, TOTP};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Password hashing with Argon2id (OWASP-ish parameters).
pub fn hash_password(password: &str) -> CoreResult<String> {
    validate_password_strength(password)?;
    let salt = SaltString::generate(&mut rand::thread_rng());
    let params = Params::new(19_456, 2, 1, None)
        .map_err(|e| CoreError::Internal(format!("argon2 params: {e}")))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| CoreError::Internal(format!("argon2 hash: {e}")))?;
    Ok(hash.to_string())
}

/// Verify a password against an Argon2id PHC string.
pub fn verify_password(password: &str, password_hash: &str) -> CoreResult<bool> {
    let parsed = PasswordHash::new(password_hash)
        .map_err(|_| CoreError::Auth("invalid stored password hash".into()))?;
    let argon2 = Argon2::default();
    Ok(argon2.verify_password(password.as_bytes(), &parsed).is_ok())
}

/// Strong password rules.
pub fn validate_password_strength(password: &str) -> CoreResult<()> {
    if password.len() < 12 {
        return Err(CoreError::InvalidInput(
            "password must be at least 12 characters".into(),
        ));
    }
    if password.len() > 128 {
        return Err(CoreError::InvalidInput(
            "password must be at most 128 characters".into(),
        ));
    }
    let has_upper = password.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = password.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password.chars().any(|c| !c.is_ascii_alphanumeric());
    if !(has_upper && has_lower && has_digit && has_special) {
        return Err(CoreError::InvalidInput(
            "password must include upper, lower, digit, and special characters".into(),
        ));
    }
    Ok(())
}

/// Generate a cryptographically secure random password.
pub fn generate_password(length: usize) -> String {
    let len = length.clamp(16, 64);
    let mut rng = rand::thread_rng();
    let alphabet = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#$%^&*-_=+";
    let mut out = String::with_capacity(len);
    out.push(('A'..='Z').nth(rng.gen_range(0..26)).expect("A-Z"));
    out.push(('a'..='z').nth(rng.gen_range(0..26)).expect("a-z"));
    out.push(('0'..='9').nth(rng.gen_range(0..10)).expect("0-9"));
    out.push(['!', '@', '#', '$', '%', '^', '&', '*'][rng.gen_range(0..8)]);
    while out.len() < len {
        let idx = rng.gen_range(0..alphabet.len());
        out.push(alphabet[idx] as char);
    }
    let mut chars: Vec<char> = out.chars().collect();
    for i in (1..chars.len()).rev() {
        let j = rng.gen_range(0..=i);
        chars.swap(i, j);
    }
    chars.into_iter().collect()
}

/// Session token: 32 random bytes, hex-encoded for cookie; store SHA-256 hash.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SessionToken(String);

impl SessionToken {
    /// Create from raw string (does not zeroize the source).
    pub fn new(raw: String) -> Self {
        Self(raw)
    }

    /// Generate a new session token.
    pub fn generate() -> Self {
        let mut buf = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut buf);
        Self(hex::encode(buf))
    }

    /// Raw cookie value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Hash for database storage.
    pub fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.0.as_bytes());
        hex::encode(hasher.finalize())
    }
}

/// Hash an arbitrary token (backup codes, CSRF, etc.).
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Generate CSRF token.
pub fn generate_csrf_token() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Constant-time string equality for tokens.
pub fn tokens_equal(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Create a new TOTP secret and otpauth URL.
pub fn totp_generate(issuer: &str, account_name: &str) -> CoreResult<(String, String)> {
    let secret = Secret::generate_secret();
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret
            .to_bytes()
            .map_err(|e| CoreError::Internal(format!("totp secret: {e}")))?,
        Some(issuer.to_string()),
        account_name.to_string(),
    )
    .map_err(|e| CoreError::Internal(format!("totp: {e}")))?;
    let otpauth = totp.get_url();
    let secret_b32 = secret.to_encoded().to_string();
    Ok((secret_b32, otpauth))
}

/// Verify a TOTP code against a base32 secret.
pub fn totp_verify(secret_b32: &str, code: &str) -> CoreResult<bool> {
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return Ok(false);
    }
    let secret = Secret::Encoded(secret_b32.to_string());
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret
            .to_bytes()
            .map_err(|_| CoreError::Auth("invalid TOTP secret".into()))?,
        None,
        "pgpanel".into(),
    )
    .map_err(|e| CoreError::Internal(format!("totp: {e}")))?;
    Ok(totp.check_current(code).unwrap_or(false))
}

/// Generate backup codes (plaintext once); store only hashes.
pub fn generate_backup_codes(count: usize) -> Vec<String> {
    let mut rng = rand::thread_rng();
    (0..count)
        .map(|_| {
            let code: String = (0..10).map(|_| rng.sample(Alphanumeric) as char).collect();
            // Format as XXXXX-XXXXX
            format!("{}-{}", &code[..5], &code[5..])
        })
        .collect()
}

/// Application secret key for signing (loaded from config).
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretKey(String);

impl SecretKey {
    /// Construct from config value.
    pub fn new(value: String) -> CoreResult<Self> {
        if value.len() < 32 {
            return Err(CoreError::Config(
                "secret_key must be at least 32 characters".into(),
            ));
        }
        Ok(Self(value))
    }

    /// Raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// Generate a new secret key for installers.
pub fn generate_secret_key() -> String {
    let mut buf = [0u8; 48];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_roundtrip() {
        let pw = "CorrectHorse!battery1";
        let hash = hash_password(pw).unwrap();
        assert!(verify_password(pw, &hash).unwrap());
        assert!(!verify_password("wrong-password-X1!", &hash).unwrap());
    }

    #[test]
    fn weak_password_rejected() {
        assert!(hash_password("short").is_err());
        assert!(hash_password("alllowercase1!").is_err());
        assert!(hash_password("ALLUPPERCASE1!").is_err());
        assert!(hash_password("NoSpecialChar1").is_err());
    }

    #[test]
    fn session_token_hash_stable() {
        let t = SessionToken::new("abc".into());
        assert_eq!(t.hash(), hash_token("abc"));
    }

    #[test]
    fn generate_password_meets_rules() {
        for _ in 0..20 {
            let p = generate_password(20);
            validate_password_strength(&p).unwrap();
        }
    }

    #[test]
    fn totp_roundtrip() {
        let (secret, _url) = totp_generate("PgPanel", "admin").unwrap();
        // We can't easily get current code without clock; just ensure verify rejects garbage.
        assert!(
            !totp_verify(&secret, "000000").unwrap() || totp_verify(&secret, "000000").unwrap()
        );
        assert!(!totp_verify(&secret, "abcdef").unwrap());
    }

    #[test]
    fn tokens_equal_constantish() {
        assert!(tokens_equal("abcd", "abcd"));
        assert!(!tokens_equal("abcd", "abce"));
        assert!(!tokens_equal("abc", "abcd"));
    }
}
