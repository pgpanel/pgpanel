//! Monitoring sample collection and retrieval.

use pgpanel_core::config::Config;
use pgpanel_core::CoreResult;

fn db_err(e: sqlx::Error) -> pgpanel_core::CoreError {
    pgpanel_core::CoreError::Internal(format!("database error: {e}"))
}
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Arc;

/// Monitoring sample.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MonitoringSample {
    pub id: i64,
    pub version: String,
    pub cluster_name: String,
    pub sampled_at: String,
    pub connections: Option<i64>,
    pub transactions_per_sec: Option<f64>,
    pub cache_hit_ratio: Option<f64>,
    pub db_size_bytes: Option<i64>,
    pub replication_lag_bytes: Option<i64>,
}

/// Monitoring service.
#[derive(Clone)]
pub struct MonitoringService {
    db: SqlitePool,
    retention_hours: u64,
}

impl MonitoringService {
    pub fn new(db: SqlitePool, config: Arc<Config>) -> Self {
        Self {
            db,
            retention_hours: config.monitoring.retention_hours,
        }
    }

    /// Record a sample.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_sample(
        &self,
        version: &str,
        cluster_name: &str,
        connections: Option<i64>,
        tps: Option<f64>,
        cache_hit: Option<f64>,
        db_size: Option<i64>,
        repl_lag: Option<i64>,
    ) -> CoreResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO monitoring_samples (version, cluster_name, sampled_at, connections, transactions_per_sec, cache_hit_ratio, db_size_bytes, replication_lag_bytes)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(version)
        .bind(cluster_name)
        .bind(&now)
        .bind(connections)
        .bind(tps)
        .bind(cache_hit)
        .bind(db_size)
        .bind(repl_lag)
        .execute(&self.db)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// Get recent samples for a cluster.
    pub async fn get_samples(
        &self,
        version: &str,
        cluster_name: &str,
        limit: i64,
    ) -> CoreResult<Vec<MonitoringSample>> {
        let rows = sqlx::query_as(
            "SELECT id, version, cluster_name, sampled_at, connections, transactions_per_sec, cache_hit_ratio, db_size_bytes, replication_lag_bytes
             FROM monitoring_samples
             WHERE version = ? AND cluster_name = ?
             ORDER BY sampled_at DESC LIMIT ?",
        )
        .bind(version)
        .bind(cluster_name)
        .bind(limit)
        .fetch_all(&self.db)
        .await
        .map_err(db_err)?;
        Ok(rows)
    }

    /// Dashboard summary across all clusters.
    pub async fn dashboard_summary(&self) -> CoreResult<Vec<MonitoringSample>> {
        let rows = sqlx::query_as(
            "SELECT ms.id, ms.version, ms.cluster_name, ms.sampled_at, ms.connections, ms.transactions_per_sec, ms.cache_hit_ratio, ms.db_size_bytes, ms.replication_lag_bytes
             FROM monitoring_samples ms
             INNER JOIN (
               SELECT version, cluster_name, MAX(id) AS max_id
               FROM monitoring_samples GROUP BY version, cluster_name
             ) latest ON ms.id = latest.max_id
             ORDER BY ms.cluster_name",
        )
        .fetch_all(&self.db)
        .await
        .map_err(db_err)?;
        Ok(rows)
    }

    /// Purge old samples.
    pub async fn purge_old(&self) -> CoreResult<u64> {
        let cutoff = (chrono::Utc::now() - chrono::Duration::hours(self.retention_hours as i64))
            .to_rfc3339();
        let result = sqlx::query("DELETE FROM monitoring_samples WHERE sampled_at < ?")
            .bind(cutoff)
            .execute(&self.db)
            .await
            .map_err(db_err)?;
        Ok(result.rows_affected())
    }
}
