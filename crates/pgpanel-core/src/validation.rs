//! Identifier and input validation.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::error::{Error, Result};

/// Safe name pattern for clusters, databases, roles.
/// Must match: ^[a-z][a-z0-9_]{2,62}$
static SAFE_NAME: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z][a-z0-9_]{2,62}$").expect("valid regex"));

/// PostgreSQL reserved keywords that must not be used as identifiers.
static RESERVED: &[&str] = &[
    "all",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asymmetric",
    "authorization",
    "binary",
    "both",
    "case",
    "cast",
    "check",
    "collate",
    "column",
    "concurrently",
    "constraint",
    "create",
    "cross",
    "current_catalog",
    "current_date",
    "current_role",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "default",
    "deferrable",
    "desc",
    "distinct",
    "do",
    "else",
    "end",
    "except",
    "false",
    "fetch",
    "for",
    "foreign",
    "freeze",
    "from",
    "full",
    "grant",
    "group",
    "having",
    "ilike",
    "in",
    "initially",
    "inner",
    "intersect",
    "into",
    "is",
    "isnull",
    "join",
    "lateral",
    "leading",
    "left",
    "like",
    "limit",
    "localtime",
    "localtimestamp",
    "natural",
    "not",
    "notnull",
    "null",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "outer",
    "overlaps",
    "placing",
    "primary",
    "references",
    "returning",
    "right",
    "select",
    "session_user",
    "similar",
    "some",
    "symmetric",
    "table",
    "tablesample",
    "then",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "user",
    "using",
    "variadic",
    "verbose",
    "when",
    "where",
    "window",
    "with",
];

/// Validate cluster / database / role name.
pub fn validate_safe_name(name: &str, field: &str) -> Result<()> {
    if !SAFE_NAME.is_match(name) {
        return Err(Error::Validation(format!(
            "{field} must match ^[a-z][a-z0-9_]{{2,62}}$ (got '{name}')"
        )));
    }
    if RESERVED.contains(&name) {
        return Err(Error::Validation(format!(
            "{field} '{name}' is a reserved PostgreSQL keyword"
        )));
    }
    Ok(())
}

/// Quote a validated PostgreSQL identifier safely.
/// Caller MUST have validated the name first.
pub fn quote_ident(name: &str) -> Result<String> {
    validate_safe_name(name, "identifier")?;
    // Double any embedded quotes (none expected after validation) and wrap.
    let escaped = name.replace('"', "\"\"");
    Ok(format!("\"{escaped}\""))
}

/// Validate display name (more relaxed, for UI labels).
pub fn validate_display_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(Error::Validation(
            "display name must be 1-128 characters".into(),
        ));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(Error::Validation(
            "display name must not contain control characters".into(),
        ));
    }
    Ok(())
}

/// Convert display name to a safe slug candidate.
pub fn slugify(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut out = String::new();
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if (c == ' ' || c == '-' || c == '.') && !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    // ensure starts with letter
    if out.is_empty() || !out.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        out = format!("c_{out}");
    }
    // pad/truncate to valid length
    while out.len() < 3 {
        out.push('x');
    }
    out.truncate(63);
    // strip trailing underscores
    out.trim_end_matches('_').to_string()
}

/// Validate CPU limit (cores as f64 string like "1.0" or "0.5").
pub fn validate_cpu_limit(cpu: f64) -> Result<()> {
    if !(0.1..=64.0).contains(&cpu) {
        return Err(Error::Validation(
            "cpu_limit must be between 0.1 and 64.0".into(),
        ));
    }
    Ok(())
}

/// Validate memory in MB.
pub fn validate_memory_mb(mb: u32) -> Result<()> {
    if !(128..=262_144).contains(&mb) {
        return Err(Error::Validation(
            "memory_mb must be between 128 and 262144".into(),
        ));
    }
    Ok(())
}

/// Validate storage limit in GB.
pub fn validate_storage_gb(gb: u32) -> Result<()> {
    if !(1..=10_000).contains(&gb) {
        return Err(Error::Validation(
            "storage_limit_gb must be between 1 and 10000".into(),
        ));
    }
    Ok(())
}

/// Validate optional public port.
pub fn validate_public_port(port: u16) -> Result<()> {
    if port < 1024 {
        return Err(Error::Validation(
            "public port must be >= 1024 (privileged ports not allowed)".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        assert!(validate_safe_name("abc", "n").is_ok());
        assert!(validate_safe_name("my_cluster_01", "n").is_ok());
        assert!(validate_safe_name(
            "a".repeat(63)
                .as_str()
                .chars()
                .next()
                .map(|_| "aab")
                .unwrap(),
            "n"
        )
        .is_ok());
    }

    #[test]
    fn invalid_names() {
        assert!(validate_safe_name("Ab", "n").is_err());
        assert!(validate_safe_name("1abc", "n").is_err());
        assert!(validate_safe_name("ab", "n").is_err()); // too short
        assert!(validate_safe_name("select", "n").is_err()); // reserved
        assert!(validate_safe_name("has-dash", "n").is_err());
        assert!(validate_safe_name("has space", "n").is_err());
    }

    #[test]
    fn quote_ident_safe() {
        assert_eq!(quote_ident("my_db").unwrap(), "\"my_db\"");
    }

    #[test]
    fn slugify_works() {
        assert_eq!(slugify("My Cluster 01"), "my_cluster_01");
        assert!(validate_safe_name(&slugify("My Cluster 01"), "s").is_ok());
    }
}
