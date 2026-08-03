//! Monitoring overview, alerts, and alert rules.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use serde::Deserialize;
use uuid::Uuid;

use pgpanel_core::audit;
use pgpanel_core::error::Error;
use pgpanel_core::models::*;

use crate::auth::{require_admin, write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::routes::clusters::load_cluster;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/monitoring/overview", get(monitoring_overview))
        .route("/api/monitoring/clusters/{id}", get(cluster_monitoring))
        .route("/api/alerts", get(list_alerts))
        .route("/api/alerts/{id}/ack", post(ack_alert))
        .route("/api/alerts/{id}/resolve", post(resolve_alert))
        .route("/api/alert-rules", get(list_alert_rules))
        .route(
            "/api/alert-rules/{id}",
            axum::routing::put(update_alert_rule),
        )
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

async fn monitoring_overview(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(query): Query<MonitoringQuery>,
) -> ApiResult<Json<MonitoringOverview>> {
    let hours = query.hours.unwrap_or(24).clamp(1, 168);
    let since = (Utc::now() - Duration::hours(hours)).to_rfc3339();

    let cluster_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM clusters")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    let healthy_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM clusters WHERE status IN ('healthy', 'healthy_with_backup_warning')",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    let open_alerts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM alerts WHERE status = 'open'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let avg_cpu: f64 =
        sqlx::query_scalar("SELECT AVG(cpu_percent) FROM cluster_metrics WHERE collected_at >= ?")
            .bind(&since)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0.0);
    let avg_memory_mb: f64 = sqlx::query_scalar(
        "SELECT AVG(memory_usage_mb) FROM cluster_metrics WHERE collected_at >= ?",
    )
    .bind(&since)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0.0);
    let max_cpu_24h: f64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(cpu_percent),0) FROM cluster_metrics WHERE collected_at >= ?",
    )
    .bind(&since)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0.0);
    let max_memory_mb_24h: f64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(memory_usage_mb),0) FROM cluster_metrics WHERE collected_at >= ?",
    )
    .bind(&since)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0.0);
    let node_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    let online_nodes: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM nodes WHERE status = 'online'")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);
    let operations_failed_24h: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operations WHERE status IN ('failed','error') AND created_at >= ?",
    )
    .bind(&since)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    let operations_running: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM operations WHERE status IN ('running','pending')")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);
    let databases_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM databases")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    #[derive(sqlx::FromRow)]
    struct NodeRow {
        node_id: String,
        name: String,
        cluster_count: i64,
        avg_cpu: f64,
        status: String,
    }
    let nodes = sqlx::query_as::<_, NodeRow>(
        "SELECT n.id AS node_id, n.name, COUNT(DISTINCT c.id) AS cluster_count, COALESCE(AVG(m.cpu_percent),0) AS avg_cpu, n.status FROM nodes n LEFT JOIN clusters c ON c.node_id=n.id LEFT JOIN cluster_metrics m ON m.cluster_id=c.id AND m.collected_at >= ? GROUP BY n.id, n.name, n.status ORDER BY n.name"
    ).bind(&since).fetch_all(&state.pool).await.unwrap_or_default().into_iter().map(|n| NodeMonitoringRow { node_id:n.node_id, name:n.name, cluster_count:n.cluster_count, avg_cpu:n.avg_cpu, status:n.status }).collect();

    let backups_last_24h: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM backups WHERE created_at >= ? AND status = 'succeeded'",
    )
    .bind(&since)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    let failed_backups_24h: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM backups WHERE created_at >= ? AND status = 'failed'",
    )
    .bind(&since)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    let backup_total = backups_last_24h + failed_backups_24h;
    let backup_success_rate = if backup_total == 0 {
        1.0
    } else {
        backups_last_24h as f64 / backup_total as f64
    };

    let replica_total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM cluster_replicas WHERE enabled = 1")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);
    let replica_healthy: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cluster_replicas WHERE enabled = 1 AND status = 'healthy'",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    #[derive(sqlx::FromRow)]
    struct SeriesRow {
        hour: String,
        cpu: f64,
        memory_mb: f64,
    }

    let series_rows = sqlx::query_as::<_, SeriesRow>(
        r#"
        SELECT substr(collected_at, 1, 13) AS hour,
               AVG(cpu_percent) AS cpu,
               AVG(memory_usage_mb) AS memory_mb
        FROM cluster_metrics
        WHERE collected_at >= ?
        GROUP BY substr(collected_at, 1, 13)
        ORDER BY hour
        "#,
    )
    .bind(&since)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let series: Vec<MonitoringPoint> = series_rows
        .into_iter()
        .map(|r| MonitoringPoint {
            at: r.hour,
            cpu: r.cpu,
            memory_mb: r.memory_mb,
        })
        .collect();

    #[derive(sqlx::FromRow)]
    struct TopRow {
        cluster_id: String,
        name: String,
        cpu_percent: f64,
        memory_usage_mb: f64,
        status: String,
    }

    let top_rows = sqlx::query_as::<_, TopRow>(
        r#"
        SELECT c.id AS cluster_id, c.name,
               MAX(m.cpu_percent) AS cpu_percent,
               MAX(m.memory_usage_mb) AS memory_usage_mb,
               c.status
        FROM clusters c
        INNER JOIN cluster_metrics m ON m.cluster_id = c.id
        WHERE m.collected_at >= ?
        GROUP BY c.id, c.name, c.status
        ORDER BY cpu_percent DESC
        LIMIT 10
        "#,
    )
    .bind(&since)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let top_clusters: Vec<ClusterLoadRow> = top_rows
        .into_iter()
        .filter_map(|r| {
            Uuid::parse_str(&r.cluster_id)
                .ok()
                .map(|cid| ClusterLoadRow {
                    cluster_id: cid,
                    name: r.name,
                    cpu_percent: r.cpu_percent,
                    memory_usage_mb: r.memory_usage_mb,
                    status: r.status,
                })
        })
        .collect();

    Ok(Json(MonitoringOverview {
        cluster_count,
        healthy_count,
        open_alerts,
        avg_cpu,
        avg_memory_mb,
        backups_last_24h,
        failed_backups_24h,
        replica_healthy,
        replica_total,
        series,
        top_clusters,
        max_cpu_24h,
        max_memory_mb_24h,
        node_count,
        online_nodes,
        operations_failed_24h,
        operations_running,
        databases_total,
        backup_success_rate,
        nodes,
    }))
}

#[derive(Debug, Deserialize)]
struct MonitoringQuery {
    hours: Option<i64>,
}

async fn cluster_monitoring(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<MonitoringQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let cluster = load_cluster(&state, id).await?;
    let since =
        (Utc::now() - Duration::hours(query.hours.unwrap_or(24).clamp(1, 168))).to_rfc3339();

    #[derive(sqlx::FromRow, serde::Serialize)]
    struct MetricSample {
        cpu_percent: f64,
        memory_usage_mb: f64,
        memory_limit_mb: f64,
        network_rx_bytes: Option<i64>,
        network_tx_bytes: Option<i64>,
        collected_at: String,
    }

    let samples = sqlx::query_as::<_, MetricSample>(
        r#"
        SELECT cpu_percent, memory_usage_mb, memory_limit_mb, network_rx_bytes,
               network_tx_bytes, collected_at
        FROM cluster_metrics
        WHERE cluster_id = ? AND collected_at >= ?
        ORDER BY collected_at ASC
        "#,
    )
    .bind(id.to_string())
    .bind(&since)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    #[derive(sqlx::FromRow, serde::Serialize)]
    struct DailyRow {
        day: String,
        avg_cpu: f64,
        max_cpu: f64,
        avg_memory_mb: f64,
        max_memory_mb: f64,
        samples: i64,
    }

    let daily = sqlx::query_as::<_, DailyRow>(
        r#"
        SELECT day, avg_cpu, max_cpu, avg_memory_mb, max_memory_mb, samples
        FROM metric_daily
        WHERE cluster_id = ?
        ORDER BY day DESC
        LIMIT 30
        "#,
    )
    .bind(id.to_string())
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    Ok(Json(serde_json::json!({
        "cluster": cluster,
        "samples": samples,
        "daily": daily,
    })))
}

#[derive(sqlx::FromRow)]
struct AlertRow {
    id: String,
    rule_id: Option<String>,
    severity: String,
    title: String,
    message: String,
    resource_type: Option<String>,
    resource_id: Option<String>,
    status: String,
    fired_at: String,
    acked_at: Option<String>,
    resolved_at: Option<String>,
}

impl AlertRow {
    fn into_alert(self) -> Result<Alert, Error> {
        Ok(Alert {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            rule_id: self.rule_id.as_ref().and_then(|s| Uuid::parse_str(s).ok()),
            severity: self.severity,
            title: self.title,
            message: self.message,
            resource_type: self.resource_type,
            resource_id: self.resource_id,
            status: self.status,
            fired_at: parse_dt(&self.fired_at),
            acked_at: self.acked_at.as_ref().map(|s| parse_dt(s)),
            resolved_at: self.resolved_at.as_ref().map(|s| parse_dt(s)),
        })
    }
}

async fn list_alerts(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<Alert>>> {
    let rows = sqlx::query_as::<_, AlertRow>(
        r#"
        SELECT id, rule_id, severity, title, message, resource_type, resource_id,
               status, fired_at, acked_at, resolved_at
        FROM alerts
        ORDER BY fired_at DESC
        LIMIT 200
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_alert().ok())
            .collect(),
    ))
}

async fn ack_alert(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&auth)?;
    let now = Utc::now().to_rfc3339();

    let res = sqlx::query(
        r#"
        UPDATE alerts SET status = 'acked', acked_at = ?, acked_by = ?
        WHERE id = ? AND status = 'open'
        "#,
    )
    .bind(&now)
    .bind(auth.user.id.to_string())
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    if res.rows_affected() == 0 {
        return Err(AppError(Error::NotFound("open alert".into())));
    }

    write_audit(
        &state,
        Some(&auth.user),
        audit::ALERT_ACK,
        "alert",
        Some(&id.to_string()),
        serde_json::json!({}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"ok": true})))
}

async fn resolve_alert(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&auth)?;
    let now = Utc::now().to_rfc3339();

    let res = sqlx::query(
        r#"
        UPDATE alerts SET status = 'resolved', resolved_at = ?
        WHERE id = ? AND status IN ('open', 'acked')
        "#,
    )
    .bind(&now)
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    if res.rows_affected() == 0 {
        return Err(AppError(Error::NotFound("alert".into())));
    }

    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(sqlx::FromRow)]
struct AlertRuleRow {
    id: String,
    name: String,
    enabled: i64,
    severity: String,
    metric: String,
    operator: String,
    threshold: f64,
    duration_seconds: i64,
    scope: String,
    scope_id: Option<String>,
    notify_channels: String,
    cooldown_seconds: i64,
}

impl AlertRuleRow {
    fn into_rule(self) -> AlertRule {
        AlertRule {
            id: Uuid::parse_str(&self.id).unwrap_or_default(),
            name: self.name,
            enabled: self.enabled != 0,
            severity: self.severity,
            metric: self.metric,
            operator: self.operator,
            threshold: self.threshold,
            duration_seconds: self.duration_seconds as u32,
            scope: self.scope,
            scope_id: self.scope_id,
            notify_channels: self.notify_channels,
            cooldown_seconds: self.cooldown_seconds as u32,
        }
    }
}

async fn list_alert_rules(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<AlertRule>>> {
    let rows = sqlx::query_as::<_, AlertRuleRow>(
        r#"
        SELECT id, name, enabled, severity, metric, operator, threshold, duration_seconds,
               scope, scope_id, notify_channels, cooldown_seconds
        FROM alert_rules
        ORDER BY name
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    Ok(Json(rows.into_iter().map(|r| r.into_rule()).collect()))
}

#[derive(Deserialize)]
struct UpdateAlertRuleBody {
    enabled: Option<bool>,
    threshold: Option<f64>,
    cooldown_seconds: Option<u32>,
}

async fn update_alert_rule(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateAlertRuleBody>,
) -> ApiResult<Json<AlertRule>> {
    require_admin(&auth)?;

    let existing = sqlx::query_as::<_, AlertRuleRow>(
        r#"
        SELECT id, name, enabled, severity, metric, operator, threshold, duration_seconds,
               scope, scope_id, notify_channels, cooldown_seconds
        FROM alert_rules WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?
    .ok_or_else(|| AppError(Error::NotFound("alert rule".into())))?;

    let enabled = body.enabled.unwrap_or(existing.enabled != 0);
    let threshold = body.threshold.unwrap_or(existing.threshold);
    let cooldown = body
        .cooldown_seconds
        .unwrap_or(existing.cooldown_seconds as u32);
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        UPDATE alert_rules SET enabled = ?, threshold = ?, cooldown_seconds = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(if enabled { 1 } else { 0 })
    .bind(threshold)
    .bind(cooldown as i64)
    .bind(&now)
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let row = sqlx::query_as::<_, AlertRuleRow>(
        r#"
        SELECT id, name, enabled, severity, metric, operator, threshold, duration_seconds,
               scope, scope_id, notify_channels, cooldown_seconds
        FROM alert_rules WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_one(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let _ = auth;
    Ok(Json(row.into_rule()))
}
