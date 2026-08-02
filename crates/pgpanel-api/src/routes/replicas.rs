//! Cluster replica management.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use pgpanel_core::audit;
use pgpanel_core::error::Error;
use pgpanel_core::models::*;

use crate::auth::{require_write, write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::routes::clusters::load_cluster;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/replicas", get(list_all_replicas))
        .route(
            "/api/clusters/{id}/replicas",
            get(list_cluster_replicas).post(create_replica),
        )
        .route("/api/replicas/{id}/sync", post(sync_replica))
        .route("/api/replicas/{id}/promote", post(promote_replica))
        .route("/api/replicas/{id}", axum::routing::delete(delete_replica))
}

const REPLICA_SELECT: &str = r#"
    SELECT id, primary_cluster_id, replica_cluster_id, name, mode, target_node_id,
           sync_cron, status, lag_seconds, last_sync_at, last_error,
           auto_failover, promote_protection, enabled, created_at, updated_at
    FROM cluster_replicas
"#;

#[derive(sqlx::FromRow)]
struct ReplicaRow {
    id: String,
    primary_cluster_id: String,
    replica_cluster_id: Option<String>,
    name: String,
    mode: String,
    target_node_id: String,
    sync_cron: String,
    status: String,
    lag_seconds: Option<i64>,
    last_sync_at: Option<String>,
    last_error: Option<String>,
    auto_failover: i64,
    promote_protection: i64,
    enabled: i64,
    created_at: String,
    updated_at: String,
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

impl ReplicaRow {
    fn into_replica(self) -> Result<ClusterReplica, Error> {
        Ok(ClusterReplica {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            primary_cluster_id: Uuid::parse_str(&self.primary_cluster_id)
                .map_err(|e| Error::Internal(e.to_string()))?,
            replica_cluster_id: self
                .replica_cluster_id
                .as_ref()
                .and_then(|s| Uuid::parse_str(s).ok()),
            name: self.name,
            mode: self.mode,
            target_node_id: Uuid::parse_str(&self.target_node_id)
                .map_err(|e| Error::Internal(e.to_string()))?,
            sync_cron: self.sync_cron,
            status: self.status,
            lag_seconds: self.lag_seconds,
            last_sync_at: self.last_sync_at.as_ref().map(|s| parse_dt(s)),
            last_error: self.last_error,
            auto_failover: self.auto_failover != 0,
            promote_protection: self.promote_protection != 0,
            enabled: self.enabled != 0,
            created_at: parse_dt(&self.created_at),
            updated_at: parse_dt(&self.updated_at),
        })
    }
}

async fn load_replica(state: &AppState, id: Uuid) -> Result<ReplicaRow, AppError> {
    sqlx::query_as::<_, ReplicaRow>(&format!("{REPLICA_SELECT} WHERE id = ?"))
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?
        .ok_or_else(|| AppError(Error::NotFound("replica".into())))
}

async fn list_all_replicas(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<ClusterReplica>>> {
    let rows =
        sqlx::query_as::<_, ReplicaRow>(&format!("{REPLICA_SELECT} ORDER BY created_at DESC"))
            .fetch_all(&state.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_replica().ok())
            .collect(),
    ))
}

async fn list_cluster_replicas(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ClusterReplica>>> {
    let _ = load_cluster(&state, id).await?;
    let rows = sqlx::query_as::<_, ReplicaRow>(&format!(
        "{REPLICA_SELECT} WHERE primary_cluster_id = ? ORDER BY created_at DESC"
    ))
    .bind(id.to_string())
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_replica().ok())
            .collect(),
    ))
}

async fn create_replica(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateReplicaRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    let _ = load_cluster(&state, id).await?;

    let node_exists: Option<String> = sqlx::query_scalar("SELECT id FROM nodes WHERE id = ?")
        .bind(req.target_node_id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if node_exists.is_none() {
        return Err(AppError(Error::NotFound("node".into())));
    }

    let replica_id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        INSERT INTO cluster_replicas (
            id, primary_cluster_id, name, mode, target_node_id, sync_cron,
            status, auto_failover, promote_protection, enabled, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, 1, 1, ?, ?)
        "#,
    )
    .bind(replica_id.to_string())
    .bind(id.to_string())
    .bind(req.name.trim())
    .bind(req.mode.trim())
    .bind(req.target_node_id.to_string())
    .bind(req.sync_cron.trim())
    .bind(if req.auto_failover { 1 } else { 0 })
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let mut operation_id = None;
    if req.provision_now {
        let op = state
            .queue
            .enqueue(
                JobType::DuplicateCluster,
                Some(id),
                serde_json::json!({
                    "replica_id": replica_id,
                    "primary_cluster_id": id,
                    "target_node_id": req.target_node_id,
                    "name": req.name,
                    "mode": req.mode,
                    "sync_cron": req.sync_cron,
                }),
                Some(&format!("duplicate-{replica_id}")),
            )
            .await
            .map_err(AppError)?;
        operation_id = Some(op);
    }

    write_audit(
        &state,
        Some(&auth.user),
        audit::REPLICA_CREATE,
        "replica",
        Some(&replica_id.to_string()),
        serde_json::json!({
            "primary_cluster_id": id,
            "target_node_id": req.target_node_id,
            "provision_now": req.provision_now,
        }),
        None,
        None,
    )
    .await;

    let row = load_replica(&state, replica_id).await?;
    Ok(Json(serde_json::json!({
        "replica": row.into_replica().map_err(AppError)?,
        "operation_id": operation_id,
    })))
}

async fn sync_replica(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    let row = load_replica(&state, id).await?;
    let primary_id = Uuid::parse_str(&row.primary_cluster_id)
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let op = state
        .queue
        .enqueue(
            JobType::SyncReplica,
            Some(primary_id),
            serde_json::json!({"replica_id": id}),
            Some(&format!("sync-replica-{id}")),
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::REPLICA_SYNC,
        "replica",
        Some(&id.to_string()),
        serde_json::json!({}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"operation_id": op})))
}

#[derive(Deserialize)]
struct PromoteBody {
    confirm: bool,
}

async fn promote_replica(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PromoteBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    if !body.confirm {
        return Err(AppError(Error::Validation(
            "promote requires confirm: true in request body".into(),
        )));
    }

    let row = load_replica(&state, id).await?;
    if row.promote_protection != 0 {
        return Err(AppError(Error::Forbidden(
            "replica has promote protection enabled".into(),
        )));
    }

    let primary_id = Uuid::parse_str(&row.primary_cluster_id)
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let op = state
        .queue
        .enqueue(
            JobType::PromoteReplica,
            Some(primary_id),
            serde_json::json!({"replica_id": id, "confirm": true}),
            Some(&format!("promote-replica-{id}")),
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::REPLICA_PROMOTE,
        "replica",
        Some(&id.to_string()),
        serde_json::json!({}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"operation_id": op})))
}

#[derive(Deserialize)]
struct DeleteReplicaQuery {
    #[serde(default)]
    destroy: bool,
}

async fn delete_replica(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<DeleteReplicaQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    require_write(&auth)?;
    let row = load_replica(&state, id).await?;
    let now = Utc::now().to_rfc3339();

    let mut operation_id = None;
    if q.destroy {
        if let Some(ref rcid) = row.replica_cluster_id {
            let replica_cluster_id =
                Uuid::parse_str(rcid).map_err(|e| AppError(Error::Internal(e.to_string())))?;
            let op = state
                .queue
                .enqueue(
                    JobType::DeleteCluster,
                    Some(replica_cluster_id),
                    serde_json::json!({
                        "mode": "permanently_delete",
                        "confirm_volume_delete": true,
                    }),
                    Some(&format!("destroy-replica-cluster-{replica_cluster_id}")),
                )
                .await
                .map_err(AppError)?;
            operation_id = Some(op);
        }
    }

    sqlx::query(
        "UPDATE cluster_replicas SET enabled = 0, status = 'paused', updated_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::REPLICA_SYNC,
        "replica",
        Some(&id.to_string()),
        serde_json::json!({"action": "pause", "destroy": q.destroy}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "ok": true,
        "operation_id": operation_id,
    })))
}
