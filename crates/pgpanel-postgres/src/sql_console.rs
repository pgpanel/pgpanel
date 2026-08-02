//! Read-only SQL console with parser-based allowlisting.

use pg_query::NodeEnum;
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, TypeInfo};
use std::time::Instant;
use tracing::info;

use pgpanel_core::error::{Error, Result};
use pgpanel_core::models::SqlQueryResponse;

use crate::client::PgClient;

/// Validates SQL is safe for the read-only console.
pub struct SqlGuard {
    admin_mode: bool,
}

impl SqlGuard {
    pub fn new(admin_mode: bool) -> Self {
        Self { admin_mode }
    }

    pub fn validate(&self, sql: &str) -> Result<()> {
        let trimmed = sql.trim();
        if trimmed.is_empty() {
            return Err(Error::SqlConsole("empty query".into()));
        }
        if trimmed.len() > 100_000 {
            return Err(Error::SqlConsole("query too long (max 100KB)".into()));
        }

        // Block dangerous tokens even before parse (defense in depth).
        let lower = trimmed.to_lowercase();
        for forbidden in [
            "copy program",
            "pg_read_file",
            "pg_write_file",
            "lo_import",
            "lo_export",
            "dblink",
            "pg_execute_server_program",
        ] {
            if lower.contains(forbidden) {
                return Err(Error::SqlConsole(format!(
                    "forbidden construct: {forbidden}"
                )));
            }
        }

        if self.admin_mode {
            return Ok(());
        }

        let parse = pg_query::parse(trimmed)
            .map_err(|e| Error::SqlConsole(format!("SQL parse error: {e}")))?;

        for stmt in &parse.protobuf.stmts {
            let Some(node) = &stmt.stmt else {
                continue;
            };
            let Some(node_enum) = &node.node else {
                continue;
            };
            if !is_readonly_node(node_enum) {
                return Err(Error::SqlConsole(format!(
                    "statement type not allowed in read-only console: {}",
                    node_kind_name(node_enum)
                )));
            }
        }

        Ok(())
    }
}

fn is_readonly_node(node: &NodeEnum) -> bool {
    match node {
        NodeEnum::SelectStmt(sel) => {
            // Reject SELECT INTO and SELECT FOR UPDATE
            if sel.into_clause.is_some() {
                return false;
            }
            // locking_clause indicates FOR UPDATE / FOR SHARE
            if !sel.locking_clause.is_empty() {
                return false;
            }
            true
        }
        NodeEnum::ExplainStmt(explain) => {
            // EXPLAIN of a nested statement — check the nested query if present
            if let Some(query) = &explain.query {
                if let Some(inner) = &query.node {
                    return is_readonly_node(inner);
                }
            }
            true
        }
        NodeEnum::VariableShowStmt(_) => true,
        // Transaction control used by us internally only — reject user BEGIN etc.
        _ => false,
    }
}

fn node_kind_name(node: &NodeEnum) -> &'static str {
    match node {
        NodeEnum::InsertStmt(_) => "INSERT",
        NodeEnum::UpdateStmt(_) => "UPDATE",
        NodeEnum::DeleteStmt(_) => "DELETE",
        NodeEnum::TruncateStmt(_) => "TRUNCATE",
        NodeEnum::DropStmt(_) => "DROP",
        NodeEnum::AlterTableStmt(_) => "ALTER TABLE",
        NodeEnum::CreateStmt(_) => "CREATE TABLE",
        NodeEnum::CreatedbStmt(_) => "CREATE DATABASE",
        NodeEnum::DropdbStmt(_) => "DROP DATABASE",
        NodeEnum::IndexStmt(_) => "CREATE INDEX",
        NodeEnum::ViewStmt(_) => "CREATE VIEW",
        NodeEnum::DoStmt(_) => "DO",
        NodeEnum::CallStmt(_) => "CALL",
        NodeEnum::VacuumStmt(_) => "VACUUM",
        NodeEnum::ReindexStmt(_) => "REINDEX",
        NodeEnum::CopyStmt(_) => "COPY",
        NodeEnum::GrantStmt(_) => "GRANT",
        NodeEnum::TransactionStmt(_) => "TRANSACTION",
        NodeEnum::VariableSetStmt(_) => "SET",
        NodeEnum::SelectStmt(_) => "SELECT",
        NodeEnum::ExplainStmt(_) => "EXPLAIN",
        NodeEnum::VariableShowStmt(_) => "SHOW",
        _ => "UNKNOWN",
    }
}

pub struct SqlConsole<'a> {
    client: &'a PgClient,
    max_rows: usize,
    statement_timeout_ms: u64,
    lock_timeout_ms: u64,
    admin_mode_enabled: bool,
}

impl<'a> SqlConsole<'a> {
    pub fn new(
        client: &'a PgClient,
        max_rows: usize,
        statement_timeout_ms: u64,
        lock_timeout_ms: u64,
        admin_mode_enabled: bool,
    ) -> Self {
        Self {
            client,
            max_rows,
            statement_timeout_ms,
            lock_timeout_ms,
            admin_mode_enabled,
        }
    }

    pub async fn execute(&self, sql: &str, request_admin_mode: bool) -> Result<SqlQueryResponse> {
        let admin = request_admin_mode && self.admin_mode_enabled;
        let guard = SqlGuard::new(admin);
        guard.validate(sql)?;

        info!(admin, "executing SQL console query");
        let start = Instant::now();

        let mut tx = self
            .client
            .pool()
            .begin()
            .await
            .map_err(|e| Error::Postgres(format!("begin: {e}")))?;

        if !admin {
            sqlx::query("SET TRANSACTION READ ONLY")
                .execute(&mut *tx)
                .await
                .map_err(|e| Error::Postgres(format!("set read only: {e}")))?;
        }

        sqlx::query(&format!(
            "SET LOCAL statement_timeout = '{}ms'",
            self.statement_timeout_ms
        ))
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Postgres(format!("statement_timeout: {e}")))?;

        sqlx::query(&format!(
            "SET LOCAL lock_timeout = '{}ms'",
            self.lock_timeout_ms
        ))
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Postgres(format!("lock_timeout: {e}")))?;

        // Limit rows via wrapping when possible; also truncate after fetch.
        let limited_sql = if sql.to_lowercase().contains("limit") {
            sql.to_string()
        } else {
            format!(
                "SELECT * FROM ({sql}) AS _pgpanel_q LIMIT {}",
                self.max_rows + 1
            )
        };

        let query_sql = if admin { sql.to_string() } else { limited_sql };

        let rows = sqlx::query(&query_sql)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| Error::Postgres(format!("query: {e}")))?;

        // Always rollback read-only transaction (no commits needed).
        tx.rollback()
            .await
            .map_err(|e| Error::Postgres(format!("rollback: {e}")))?;

        let truncated = rows.len() > self.max_rows;
        let rows: Vec<PgRow> = rows.into_iter().take(self.max_rows).collect();

        let columns: Vec<String> = if let Some(first) = rows.first() {
            first
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect()
        } else {
            Vec::new()
        };

        let mut out_rows = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut vals = Vec::with_capacity(columns.len());
            for (i, col) in row.columns().iter().enumerate() {
                vals.push(pg_value_to_json(row, i, col.type_info().name()));
            }
            out_rows.push(vals);
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        let row_count = out_rows.len();

        Ok(SqlQueryResponse {
            columns,
            rows: out_rows,
            row_count,
            truncated,
            duration_ms,
            notices: Vec::new(),
        })
    }
}

fn pg_value_to_json(row: &PgRow, idx: usize, type_name: &str) -> Value {
    // Try common types; fall back to string or null.
    if let Ok(v) = row.try_get::<Option<bool>, _>(idx) {
        return match v {
            Some(b) => Value::Bool(b),
            None => Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<Option<i64>, _>(idx) {
        return match v {
            Some(n) => Value::Number(n.into()),
            None => Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<Option<i32>, _>(idx) {
        return match v {
            Some(n) => Value::Number(n.into()),
            None => Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(idx) {
        return match v {
            Some(n) => serde_json::Number::from_f64(n)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            None => Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<Option<String>, _>(idx) {
        return match v {
            Some(s) => {
                const MAX: usize = 1024;
                if s.len() > MAX {
                    Value::String(format!("{}…[truncated {} bytes]", &s[..MAX], s.len()))
                } else {
                    Value::String(s)
                }
            }
            None => Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return match v {
            Some(b) => Value::String(format!("\\x[binary {} bytes]", b.len())),
            None => Value::Null,
        };
    }
    // Unknown type
    let _ = type_name;
    Value::String(format!("[{type_name}]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_select() {
        let g = SqlGuard::new(false);
        assert!(g.validate("SELECT 1").is_ok());
        assert!(g.validate("SELECT * FROM users WHERE id = 1").is_ok());
        assert!(g.validate("WITH c AS (SELECT 1) SELECT * FROM c").is_ok());
        assert!(g.validate("EXPLAIN SELECT 1").is_ok());
        assert!(g.validate("SHOW search_path").is_ok());
    }

    #[test]
    fn rejects_writes() {
        let g = SqlGuard::new(false);
        assert!(g.validate("INSERT INTO t VALUES (1)").is_err());
        assert!(g.validate("UPDATE t SET a = 1").is_err());
        assert!(g.validate("DELETE FROM t").is_err());
        assert!(g.validate("DROP TABLE t").is_err());
        assert!(g.validate("CREATE TABLE t (id int)").is_err());
        assert!(g.validate("TRUNCATE t").is_err());
        assert!(g.validate("ALTER TABLE t ADD COLUMN x int").is_err());
        assert!(g.validate("VACUUM FULL t").is_err());
        assert!(g.validate("DO $$ BEGIN END $$").is_err());
    }

    #[test]
    fn rejects_select_for_update() {
        let g = SqlGuard::new(false);
        assert!(g.validate("SELECT * FROM t FOR UPDATE").is_err());
    }

    #[test]
    fn rejects_dangerous_functions() {
        let g = SqlGuard::new(false);
        assert!(g.validate("SELECT pg_read_file('/etc/passwd')").is_err());
    }

    #[test]
    fn admin_mode_allows_writes() {
        let g = SqlGuard::new(true);
        assert!(g.validate("DELETE FROM t").is_ok());
    }
}
