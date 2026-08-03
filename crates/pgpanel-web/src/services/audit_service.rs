//! Audit log service with chained hashes.

use pgpanel_core::audit::{chain_hash, AuditEvent, AUDIT_GENESIS};
use pgpanel_core::CoreResult;

fn db_err(e: sqlx::Error) -> pgpanel_core::CoreError {
    pgpanel_core::CoreError::Internal(format!("database error: {e}"))
}
use serde::Serialize;
use sqlx::SqlitePool;

/// Append-only audit log.
#[derive(Clone)]
pub struct AuditService {
    db: SqlitePool,
}

impl AuditService {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Append an audit event.
    pub async fn record(&self, event: AuditEvent) -> CoreResult<i64> {
        let prev_hash = self.last_hash().await?;
        let entry_hash = chain_hash(&prev_hash, &event);

        let result = sqlx::query(
            "INSERT INTO audit_log (at, user_id, username, source_ip, action, target, result, request_id, before_meta, after_meta, failure_reason, prev_hash, entry_hash)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(event.at.to_rfc3339())
        .bind(event.user_id)
        .bind(&event.username)
        .bind(&event.source_ip)
        .bind(&event.action)
        .bind(&event.target)
        .bind(&event.result)
        .bind(&event.request_id)
        .bind(event.before_meta.as_ref().map(|v| v.to_string()))
        .bind(event.after_meta.as_ref().map(|v| v.to_string()))
        .bind(&event.failure_reason)
        .bind(&prev_hash)
        .bind(&entry_hash)
        .execute(&self.db)
        .await
        .map_err(db_err)?;

        Ok(result.last_insert_rowid())
    }

    async fn last_hash(&self) -> CoreResult<String> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT entry_hash FROM audit_log ORDER BY id DESC LIMIT 1")
                .fetch_optional(&self.db)
                .await
                .map_err(db_err)?;
        Ok(row
            .map(|r| r.0)
            .unwrap_or_else(|| AUDIT_GENESIS.to_string()))
    }

    /// List audit entries with optional filters.
    pub async fn list(
        &self,
        action_filter: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> CoreResult<Vec<AuditRow>> {
        let rows = if let Some(action) = action_filter {
            sqlx::query_as(
                "SELECT id, at, user_id, username, source_ip, action, target, result, request_id, failure_reason
                 FROM audit_log WHERE action LIKE ? ORDER BY id DESC LIMIT ? OFFSET ?",
            )
            .bind(format!("%{action}%"))
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db)
            .await
            .map_err(db_err)?
        } else {
            sqlx::query_as(
                "SELECT id, at, user_id, username, source_ip, action, target, result, request_id, failure_reason
                 FROM audit_log ORDER BY id DESC LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db)
            .await
            .map_err(db_err)?
        };
        Ok(rows)
    }

    /// Count total entries.
    pub async fn count(&self, action_filter: Option<&str>) -> CoreResult<i64> {
        let count: (i64,) = if let Some(action) = action_filter {
            sqlx::query_as("SELECT COUNT(*) FROM audit_log WHERE action LIKE ?")
                .bind(format!("%{action}%"))
                .fetch_one(&self.db)
                .await
                .map_err(db_err)?
        } else {
            sqlx::query_as("SELECT COUNT(*) FROM audit_log")
                .fetch_one(&self.db)
                .await
                .map_err(db_err)?
        };
        Ok(count.0)
    }

    /// Export as CSV rows.
    pub async fn export_csv(&self, limit: i64) -> CoreResult<String> {
        let rows = self.list(None, limit, 0).await?;
        let mut csv = String::from(
            "id,at,username,source_ip,action,target,result,request_id,failure_reason\n",
        );
        for r in rows {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{}\n",
                r.id,
                escape_csv(&r.at),
                escape_csv(&r.username.unwrap_or_default()),
                escape_csv(&r.source_ip.unwrap_or_default()),
                escape_csv(&r.action),
                escape_csv(&r.target.unwrap_or_default()),
                escape_csv(&r.result),
                escape_csv(&r.request_id),
                escape_csv(&r.failure_reason.unwrap_or_default()),
            ));
        }
        Ok(csv)
    }
}

fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Audit row for display.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct AuditRow {
    pub id: i64,
    pub at: String,
    pub user_id: Option<i64>,
    pub username: Option<String>,
    pub source_ip: Option<String>,
    pub action: String,
    pub target: Option<String>,
    pub result: String,
    pub request_id: String,
    pub failure_reason: Option<String>,
}
