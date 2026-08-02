use chrono::Utc;
use sqlx::SqlitePool;
use tracing::info;
use uuid::Uuid;

use pgpanel_core::error::{Error, Result};
use pgpanel_core::models::{JobStatus, JobType, Operation, OperationLog};

#[derive(Clone)]
pub struct JobQueue {
    pool: SqlitePool,
}

impl JobQueue {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn enqueue(
        &self,
        job_type: JobType,
        cluster_id: Option<Uuid>,
        payload: serde_json::Value,
        idempotency_key: Option<&str>,
    ) -> Result<Uuid> {
        if let Some(key) = idempotency_key {
            if let Some(existing) = self.find_by_idempotency(key).await? {
                info!(operation_id = %existing, %key, "idempotent enqueue hit");
                return Ok(existing);
            }
        }

        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO operations (id, job_type, status, cluster_id, payload, progress, idempotency_key, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, 0, ?, ?, ?)
            "#,
        )
        .bind(id.to_string())
        .bind(job_type.as_str())
        .bind(JobStatus::Queued.as_str())
        .bind(cluster_id.map(|c| c.to_string()))
        .bind(payload.to_string())
        .bind(idempotency_key)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(format!("enqueue: {e}")))?;

        self.append_log(id, "info", &format!("queued job {}", job_type.as_str()))
            .await?;
        Ok(id)
    }

    async fn find_by_idempotency(&self, key: &str) -> Result<Option<Uuid>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT id FROM operations WHERE idempotency_key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;
        Ok(row.and_then(|(id,)| Uuid::parse_str(&id).ok()))
    }

    /// Claim next queued job (or recover stale running jobs after restart).
    pub async fn claim_next(&self) -> Result<Option<Operation>> {
        // Recover jobs stuck in running for > 30 minutes (backend crash).
        let stale_cutoff = (Utc::now() - chrono::Duration::minutes(30)).to_rfc3339();
        sqlx::query(
            r#"
            UPDATE operations SET status = 'failed', error = 'worker restarted while job was running',
                   finished_at = ?, updated_at = ?
            WHERE status = 'running' AND started_at < ?
            "#,
        )
        .bind(Utc::now().to_rfc3339())
        .bind(Utc::now().to_rfc3339())
        .bind(&stale_cutoff)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        let row = sqlx::query_as::<_, OpRow>(
            r#"
            SELECT id, job_type, status, cluster_id, payload, result, error, progress,
                   created_at, updated_at, started_at, finished_at
            FROM operations
            WHERE status = 'queued'
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let now = Utc::now().to_rfc3339();
        let updated = sqlx::query(
            r#"
            UPDATE operations SET status = 'running', started_at = ?, updated_at = ?
            WHERE id = ? AND status = 'queued'
            "#,
        )
        .bind(&now)
        .bind(&now)
        .bind(&row.id)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        if updated.rows_affected() == 0 {
            return Ok(None); // lost race
        }

        Ok(Some(row.into_operation()?))
    }

    pub async fn set_progress(&self, id: Uuid, progress: u8, message: &str) -> Result<()> {
        sqlx::query("UPDATE operations SET progress = ?, updated_at = ? WHERE id = ?")
            .bind(progress as i64)
            .bind(Utc::now().to_rfc3339())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Job(e.to_string()))?;
        self.append_log(id, "info", message).await
    }

    pub async fn succeed(&self, id: Uuid, result: serde_json::Value) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE operations SET status = 'succeeded', result = ?, progress = 100,
                   finished_at = ?, updated_at = ? WHERE id = ?
            "#,
        )
        .bind(result.to_string())
        .bind(&now)
        .bind(&now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        self.append_log(id, "info", "job succeeded").await
    }

    pub async fn fail(&self, id: Uuid, error: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE operations SET status = 'failed', error = ?, finished_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(error)
        .bind(&now)
        .bind(&now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        self.append_log(id, "error", error).await
    }

    pub async fn append_log(&self, operation_id: Uuid, level: &str, message: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO operation_logs (operation_id, level, message, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(operation_id.to_string())
        .bind(level)
        .bind(message)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        Ok(())
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<Operation>> {
        let row = sqlx::query_as::<_, OpRow>(
            r#"
            SELECT id, job_type, status, cluster_id, payload, result, error, progress,
                   created_at, updated_at, started_at, finished_at
            FROM operations WHERE id = ?
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        row.map(|r| r.into_operation()).transpose()
    }

    pub async fn list(&self, limit: i64) -> Result<Vec<Operation>> {
        let rows = sqlx::query_as::<_, OpRow>(
            r#"
            SELECT id, job_type, status, cluster_id, payload, result, error, progress,
                   created_at, updated_at, started_at, finished_at
            FROM operations ORDER BY created_at DESC LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        rows.into_iter().map(|r| r.into_operation()).collect()
    }

    pub async fn logs(&self, operation_id: Uuid, after_id: i64) -> Result<Vec<OperationLog>> {
        let rows = sqlx::query_as::<_, LogRow>(
            r#"
            SELECT id, operation_id, level, message, created_at
            FROM operation_logs
            WHERE operation_id = ? AND id > ?
            ORDER BY id ASC
            "#,
        )
        .bind(operation_id.to_string())
        .bind(after_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        rows.into_iter().map(|r| r.into_log()).collect()
    }

    /// Mark non-terminal create_cluster jobs after restart for recovery logic.
    pub async fn recover_interrupted(&self) -> Result<u64> {
        let now = Utc::now().to_rfc3339();
        let res = sqlx::query(
            r#"
            UPDATE operations
            SET status = 'queued', error = NULL, updated_at = ?,
                started_at = NULL
            WHERE status = 'running'
            "#,
        )
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        Ok(res.rows_affected())
    }
}

#[derive(sqlx::FromRow)]
struct OpRow {
    id: String,
    job_type: String,
    status: String,
    cluster_id: Option<String>,
    payload: String,
    result: Option<String>,
    error: Option<String>,
    progress: i64,
    created_at: String,
    updated_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
}

impl OpRow {
    fn into_operation(self) -> Result<Operation> {
        Ok(Operation {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            job_type: JobType::parse(&self.job_type)
                .ok_or_else(|| Error::Internal(format!("unknown job type {}", self.job_type)))?,
            status: JobStatus::parse(&self.status),
            cluster_id: self
                .cluster_id
                .as_ref()
                .and_then(|s| Uuid::parse_str(s).ok()),
            payload: serde_json::from_str(&self.payload).unwrap_or_default(),
            result: self
                .result
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
            error: self.error,
            progress: self.progress as u8,
            created_at: parse_dt(&self.created_at),
            updated_at: parse_dt(&self.updated_at),
            started_at: self.started_at.as_ref().map(|s| parse_dt(s)),
            finished_at: self.finished_at.as_ref().map(|s| parse_dt(s)),
        })
    }
}

#[derive(sqlx::FromRow)]
struct LogRow {
    id: i64,
    operation_id: String,
    level: String,
    message: String,
    created_at: String,
}

impl LogRow {
    fn into_log(self) -> Result<OperationLog> {
        Ok(OperationLog {
            id: self.id,
            operation_id: Uuid::parse_str(&self.operation_id)
                .map_err(|e| Error::Internal(e.to_string()))?,
            level: self.level,
            message: self.message,
            created_at: parse_dt(&self.created_at),
        })
    }
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
