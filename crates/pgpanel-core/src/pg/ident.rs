//! PostgreSQL identifier quoting and SQL helpers.

use crate::error::{CoreError, CoreResult};
use crate::validation::validate_pg_identifier;

/// Quote a PostgreSQL identifier safely (double-quote escaping).
///
/// Prefer validating with [`validate_pg_identifier`] for unquoted-safe names,
/// then quote for all DDL. This function also accepts already-validated names
/// and escapes embedded double quotes.
pub fn quote_ident(ident: &str) -> CoreResult<String> {
    if ident.is_empty() {
        return Err(CoreError::InvalidInput("identifier is empty".into()));
    }
    if ident.contains('\0') {
        return Err(CoreError::InvalidInput(
            "identifier must not contain NUL".into(),
        ));
    }
    if ident.len() > 63 {
        return Err(CoreError::InvalidInput(
            "identifier exceeds 63 bytes".into(),
        ));
    }
    // Reject characters that are not valid in PG identifiers even when quoted,
    // except we allow a broader set when quoting — still block control chars.
    if ident.chars().any(|c| c.is_control()) {
        return Err(CoreError::InvalidInput(
            "identifier must not contain control characters".into(),
        ));
    }
    let escaped = ident.replace('"', "\"\"");
    Ok(format!("\"{escaped}\""))
}

/// Validate then quote a simple identifier.
pub fn quote_simple_ident(ident: &str) -> CoreResult<String> {
    validate_pg_identifier(ident)?;
    quote_ident(ident)
}

/// Quote a literal string for rare cases where parameters cannot be used.
/// Prefer parameterized queries. Escapes single quotes.
pub fn quote_literal(value: &str) -> CoreResult<String> {
    if value.contains('\0') {
        return Err(CoreError::InvalidInput(
            "literal must not contain NUL".into(),
        ));
    }
    let escaped = value.replace('\'', "''");
    Ok(format!("'{escaped}'"))
}

/// Allowlisted extensions that may be installed via the panel.
pub fn is_allowlisted_extension(name: &str) -> bool {
    matches!(
        name,
        "pgcrypto"
            | "uuid-ossp"
            | "pg_stat_statements"
            | "pg_trgm"
            | "btree_gin"
            | "btree_gist"
            | "citext"
            | "unaccent"
            | "hstore"
            | "postgres_fdw"
            | "file_fdw"
            | "tablefunc"
            | "pg_buffercache"
    )
}

/// Build a CREATE ROLE statement with safe defaults (no superuser).
pub fn create_login_role_sql(
    name: &str,
    password: Option<&str>,
    connection_limit: Option<i32>,
    valid_until: Option<&str>,
) -> CoreResult<String> {
    let qname = quote_simple_ident(name)?;
    let mut sql = format!(
        "CREATE ROLE {qname} WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOREPLICATION NOBYPASSRLS INHERIT"
    );
    if let Some(pw) = password {
        let qpw = quote_literal(pw)?;
        sql.push_str(&format!(" PASSWORD {qpw}"));
    }
    if let Some(limit) = connection_limit {
        if !(-1..=100_000).contains(&limit) {
            return Err(CoreError::InvalidInput(
                "connection limit out of range".into(),
            ));
        }
        sql.push_str(&format!(" CONNECTION LIMIT {limit}"));
    }
    if let Some(until) = valid_until {
        // Expect ISO date; quote as literal.
        if until.len() > 64 || until.contains(|c: char| c.is_control()) {
            return Err(CoreError::InvalidInput("invalid valid_until".into()));
        }
        let q = quote_literal(until)?;
        sql.push_str(&format!(" VALID UNTIL {q}"));
    }
    Ok(sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_ident_escapes() {
        assert_eq!(quote_ident("foo").unwrap(), "\"foo\"");
        assert_eq!(quote_ident("foo\"bar").unwrap(), "\"foo\"\"bar\"");
        assert!(quote_ident("").is_err());
        assert!(quote_ident("a\0b").is_err());
    }

    #[test]
    fn quote_simple_rejects_dashes() {
        assert!(quote_simple_ident("foo-bar").is_err());
        assert!(quote_simple_ident("foo_bar").is_ok());
    }

    #[test]
    fn quote_literal_escapes() {
        assert_eq!(quote_literal("a'b").unwrap(), "'a''b'");
    }

    #[test]
    fn malicious_idents() {
        for n in [
            "'; DROP TABLE",
            "Robert'); DROP TABLE students;--",
            "../x",
            "a;b",
        ] {
            assert!(quote_simple_ident(n).is_err());
        }
    }

    #[test]
    fn create_role_defaults() {
        let sql = create_login_role_sql("app_user", Some("s3cret!Pass"), Some(10), None).unwrap();
        assert!(sql.contains("NOSUPERUSER"));
        assert!(sql.contains("NOCREATEDB"));
        assert!(sql.contains("\"app_user\""));
        assert!(!sql.split_whitespace().any(|t| t == "SUPERUSER"));
        assert!(sql.split_whitespace().any(|t| t == "NOSUPERUSER"));
    }
}
