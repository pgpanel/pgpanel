//! Input validation for cluster names, identifiers, ports, and paths.

use crate::error::{CoreError, CoreResult};
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::{Component, Path, PathBuf};

static CLUSTER_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z][a-z0-9_]{1,31}$").expect("cluster name regex"));

static PG_IDENT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]{0,62}$").expect("pg ident regex"));

static VERSION_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(16|17|18)$").expect("version regex"));

/// Validate a cluster system identifier.
pub fn validate_cluster_name(name: &str) -> CoreResult<()> {
    if name.is_empty() {
        return Err(CoreError::InvalidInput("cluster name is required".into()));
    }
    if name.len() > 32 {
        return Err(CoreError::InvalidInput(
            "cluster name must be at most 32 characters".into(),
        ));
    }
    if name.contains(|c: char| c.is_whitespace() || c.is_control()) {
        return Err(CoreError::InvalidInput(
            "cluster name must not contain whitespace or control characters".into(),
        ));
    }
    if !name.is_ascii() {
        return Err(CoreError::InvalidInput(
            "cluster name must be ASCII only".into(),
        ));
    }
    if !CLUSTER_NAME_RE.is_match(name) {
        return Err(CoreError::InvalidInput(
            "cluster name must match ^[a-z][a-z0-9_]{1,31}$ (no dashes, dots, or slashes)".into(),
        ));
    }
    // Extra denylist for shell metacharacters even if regex somehow changes.
    const DENY: &[char] = &[
        '-', '.', '/', '\\', ';', '|', '&', '$', '`', '(', ')', '<', '>', '!', '*', '?', '[', ']',
        '{', '}', '\'', '"', '\n', '\r', '\0', '~',
    ];
    if name.contains(DENY) {
        return Err(CoreError::InvalidInput(
            "cluster name contains forbidden characters".into(),
        ));
    }
    Ok(())
}

/// Validate a PostgreSQL major version string.
pub fn validate_pg_version(version: &str) -> CoreResult<()> {
    if !VERSION_RE.is_match(version) {
        return Err(CoreError::InvalidInput(format!(
            "unsupported PostgreSQL version '{version}' (allowed: 16, 17, 18)"
        )));
    }
    Ok(())
}

/// Validate a TCP port (avoid privileged ports by default).
pub fn validate_port(port: u16, allow_privileged: bool) -> CoreResult<()> {
    if port == 0 {
        return Err(CoreError::InvalidInput("port must not be 0".into()));
    }
    if !allow_privileged && port < 1024 {
        return Err(CoreError::InvalidInput(
            "port must be >= 1024 unless privileged ports are explicitly allowed".into(),
        ));
    }
    Ok(())
}

/// Validate a simple PostgreSQL identifier (unquoted form).
pub fn validate_pg_identifier(ident: &str) -> CoreResult<()> {
    if ident.is_empty() {
        return Err(CoreError::InvalidInput("identifier is required".into()));
    }
    if ident.len() > 63 {
        return Err(CoreError::InvalidInput(
            "identifier must be at most 63 bytes".into(),
        ));
    }
    if ident.contains('\0') {
        return Err(CoreError::InvalidInput(
            "identifier must not contain NUL".into(),
        ));
    }
    if !PG_IDENT_RE.is_match(ident) {
        return Err(CoreError::InvalidInput(
            "identifier must match ^[a-zA-Z_][a-zA-Z0-9_]{0,62}$".into(),
        ));
    }
    Ok(())
}

/// Validate that a path is absolute, normalized, and under one of the allowlisted roots.
pub fn validate_path_under_roots(path: &str, roots: &[PathBuf]) -> CoreResult<PathBuf> {
    if path.is_empty() {
        return Err(CoreError::InvalidInput("path is required".into()));
    }
    if path.contains('\0') {
        return Err(CoreError::InvalidInput("path must not contain NUL".into()));
    }
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err(CoreError::InvalidInput("path must be absolute".into()));
    }
    for c in p.components() {
        match c {
            Component::ParentDir => {
                return Err(CoreError::InvalidInput("path must not contain '..'".into()));
            }
            Component::Normal(os) => {
                let s = os.to_string_lossy();
                if s.contains('\0') {
                    return Err(CoreError::InvalidInput("invalid path component".into()));
                }
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    // Lexical normalize without following symlinks (caller must also check realpath).
    let normalized = normalize_lexical(p);
    let allowed = roots.iter().any(|root| {
        let root_n = normalize_lexical(root);
        normalized.starts_with(&root_n)
    });
    if !allowed {
        return Err(CoreError::InvalidInput(
            "path is outside allowlisted data directory roots".into(),
        ));
    }
    Ok(normalized)
}

/// Lexical path normalization (no symlink resolution).
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s),
            Component::Prefix(p) => out.push(p.as_os_str()),
        }
    }
    out
}

/// Validate listen_addresses to a conservative allowlist of patterns.
pub fn validate_listen_addresses(value: &str) -> CoreResult<()> {
    let v = value.trim();
    if v.is_empty() {
        return Err(CoreError::InvalidInput(
            "listen_addresses must not be empty".into(),
        ));
    }
    if v.len() > 256 {
        return Err(CoreError::InvalidInput(
            "listen_addresses is too long".into(),
        ));
    }
    // Allow localhost-only defaults and explicit IPs / hostnames without shell chars.
    if v.contains(|c: char| c.is_control() || ";&|`$<>\\\"'".contains(c)) {
        return Err(CoreError::InvalidInput(
            "listen_addresses contains forbidden characters".into(),
        ));
    }
    Ok(())
}

/// Validate encoding name.
pub fn validate_encoding(encoding: &str) -> CoreResult<()> {
    const ALLOWED: &[&str] = &[
        "UTF8",
        "SQL_ASCII",
        "LATIN1",
        "LATIN2",
        "WIN1250",
        "WIN1252",
    ];
    if !ALLOWED.iter().any(|a| a.eq_ignore_ascii_case(encoding)) {
        return Err(CoreError::InvalidInput(format!(
            "encoding '{encoding}' is not in the allowlist"
        )));
    }
    Ok(())
}

/// Validate locale conservatively.
pub fn validate_locale(locale: &str) -> CoreResult<()> {
    if locale.is_empty() || locale.len() > 64 {
        return Err(CoreError::InvalidInput("invalid locale".into()));
    }
    if !locale
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
    {
        return Err(CoreError::InvalidInput(
            "locale contains forbidden characters".into(),
        ));
    }
    Ok(())
}

/// Validate destructive confirmation phrase equals `DELETE <name>`.
pub fn validate_delete_confirmation(name: &str, confirmation: &str) -> CoreResult<()> {
    let expected = format!("DELETE {name}");
    if confirmation != expected {
        return Err(CoreError::InvalidInput(format!(
            "confirmation must be exactly '{expected}'"
        )));
    }
    Ok(())
}

/// Allowlisted postgresql.conf keys editable via the panel.
pub fn is_allowlisted_config_key(key: &str) -> bool {
    matches!(
        key,
        "max_connections"
            | "shared_buffers"
            | "effective_cache_size"
            | "maintenance_work_mem"
            | "work_mem"
            | "wal_level"
            | "max_wal_senders"
            | "max_replication_slots"
            | "wal_compression"
            | "checkpoint_timeout"
            | "checkpoint_completion_target"
            | "log_min_duration_statement"
            | "idle_in_transaction_session_timeout"
            | "statement_timeout"
            | "lock_timeout"
            | "ssl"
            | "listen_addresses"
    )
}

/// Validate a config setting value for an allowlisted key.
pub fn validate_config_value(key: &str, value: &str) -> CoreResult<()> {
    if !is_allowlisted_config_key(key) {
        return Err(CoreError::InvalidInput(format!(
            "configuration key '{key}' is not allowlisted"
        )));
    }
    if value.is_empty() || value.len() > 256 {
        return Err(CoreError::InvalidInput(
            "invalid configuration value".into(),
        ));
    }
    if value.contains(|c: char| c.is_control() || c == '\n' || c == '\r') {
        return Err(CoreError::InvalidInput(
            "configuration value must not contain control characters".into(),
        ));
    }
    match key {
        "wal_level" => {
            if !matches!(value, "minimal" | "replica" | "logical") {
                return Err(CoreError::InvalidInput(
                    "wal_level must be minimal, replica, or logical".into(),
                ));
            }
        }
        "ssl" | "wal_compression" => {
            if !matches!(value, "on" | "off" | "true" | "false" | "1" | "0") {
                return Err(CoreError::InvalidInput(format!("{key} must be on/off")));
            }
        }
        "listen_addresses" => validate_listen_addresses(value)?,
        "checkpoint_completion_target" => {
            let f: f64 = value.parse().map_err(|_| {
                CoreError::InvalidInput("checkpoint_completion_target must be a float".into())
            })?;
            if !(0.0..=1.0).contains(&f) {
                return Err(CoreError::InvalidInput(
                    "checkpoint_completion_target must be between 0 and 1".into(),
                ));
            }
        }
        "max_connections" | "max_wal_senders" | "max_replication_slots" => {
            let n: u32 = value.parse().map_err(|_| {
                CoreError::InvalidInput(format!("{key} must be a positive integer"))
            })?;
            if n == 0 || n > 100_000 {
                return Err(CoreError::InvalidInput(format!("{key} out of range")));
            }
        }
        _ => {
            // Memory/time settings: allow digits with optional unit suffix.
            if !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            {
                return Err(CoreError::InvalidInput(format!("invalid value for {key}")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_cluster_names() {
        for n in ["main", "app_db", "a1", "prod_17_primary"] {
            validate_cluster_name(n).unwrap();
        }
    }

    #[test]
    fn rejects_bad_cluster_names() {
        for n in [
            "Main",
            "a",
            "has-dash",
            "has.dot",
            "has/slash",
            "../etc",
            "name;rm",
            "名前",
            "with space",
            "",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", // 33 chars
            "1startsnum",
            "shell`cmd`",
        ] {
            assert!(
                validate_cluster_name(n).is_err(),
                "expected reject for {n:?}"
            );
        }
    }

    #[test]
    fn path_traversal_rejected() {
        let roots = vec![PathBuf::from("/var/lib/postgresql")];
        assert!(validate_path_under_roots("/var/lib/postgresql/../etc/passwd", &roots).is_err());
        assert!(validate_path_under_roots("/etc/passwd", &roots).is_err());
        assert!(validate_path_under_roots("/var/lib/postgresql/17/main", &roots).is_ok());
    }

    #[test]
    fn delete_confirmation() {
        validate_delete_confirmation("main", "DELETE main").unwrap();
        assert!(validate_delete_confirmation("main", "delete main").is_err());
        assert!(validate_delete_confirmation("main", "DELETE other").is_err());
    }

    #[test]
    fn config_allowlist() {
        assert!(is_allowlisted_config_key("max_connections"));
        assert!(!is_allowlisted_config_key("archive_command"));
        validate_config_value("wal_level", "replica").unwrap();
        assert!(validate_config_value("wal_level", "evil").is_err());
        assert!(validate_config_value("archive_command", "/bin/true").is_err());
    }
}
