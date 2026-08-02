use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use secrecy::ExposeSecret;
use uuid::Uuid;

use pgpanel_core::audit;
use pgpanel_core::crypto::{encrypt_secret, generate_password};
use pgpanel_core::error::Error;
use pgpanel_core::models::*;
use pgpanel_core::validation::validate_display_name;
use pgpanel_docker::ClusterProvisioner;
use pgpanel_jobs::JobQueue;

use crate::auth::{write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/clusters", get(list_clusters).post(create_cluster))
        .route(
            "/api/clusters/{id}",
            get(get_cluster).delete(delete_cluster),
        )
        .route("/api/clusters/{id}/start", post(start_cluster))
        .route("/api/clusters/{id}/stop", post(stop_cluster))
        .route("/api/clusters/{id}/restart", post(restart_cluster))
        .route("/api/dashboard", get(dashboard))
}

async fn list_clusters(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<Cluster>>> {
    let rows = sqlx::query_as::<_, ClusterRow>(
        r#"
        SELECT id, name, slug, postgres_version, docker_container_id, docker_container_name,
               docker_volume_name, docker_network_name, internal_hostname, public_port,
               cpu_limit, memory_mb, storage_limit_gb, status, health, databasus_status,
               delete_protection, enable_backup, last_error, node_id, created_at, updated_at
        FROM clusters ORDER BY created_at DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_cluster().ok())
            .collect(),
    ))
}

async fn get_cluster(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Cluster>> {
    let cluster = load_cluster(&state, id).await?;
    Ok(Json(cluster))
}

async fn create_cluster(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateClusterRequest>,
) -> ApiResult<Json<ClusterCreatedResponse>> {
    validate_display_name(&req.name).map_err(AppError)?;
    let slug = ClusterProvisioner::validate_create_request(&req).map_err(AppError)?;

    // Unique slug check
    let exists: Option<String> = sqlx::query_scalar("SELECT id FROM clusters WHERE slug = ?")
        .bind(&slug)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if exists.is_some() {
        return Err(AppError(Error::Conflict(format!(
            "cluster slug '{slug}' already exists"
        ))));
    }

    let id = Uuid::new_v4();
    let names = state.provisioner.resource_names(&slug);
    let admin_password = generate_password();
    let enc =
        encrypt_secret(&state.config.master_encryption_key, &admin_password).map_err(AppError)?;
    let now = Utc::now().to_rfc3339();
    let public_port = if req.expose_publicly {
        req.optional_public_port.map(|p| p as i64)
    } else {
        None
    };

    let node_id = if let Some(nid) = req.node_id {
        let exists: Option<String> =
            sqlx::query_scalar("SELECT id FROM nodes WHERE id = ? AND status != 'disabled'")
                .bind(nid.to_string())
                .fetch_optional(&state.pool)
                .await
                .map_err(|e| AppError(Error::Internal(e.to_string())))?;
        if exists.is_none() {
            return Err(AppError(Error::Validation(
                "selected node does not exist or is disabled".into(),
            )));
        }
        // Capacity check — NULL or <= 0 means unlimited
        let max: Option<i64> = sqlx::query_scalar("SELECT max_clusters FROM nodes WHERE id = ?")
            .bind(nid.to_string())
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
        if let Some(max) = max.filter(|m| *m > 0) {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM clusters WHERE node_id = ?")
                .bind(nid.to_string())
                .fetch_one(&state.pool)
                .await
                .unwrap_or(0);
            if count >= max {
                return Err(AppError(Error::Validation(format!(
                    "node is at capacity ({max} clusters)"
                ))));
            }
        }
        nid.to_string()
    } else {
        let def: Option<String> =
            sqlx::query_scalar("SELECT id FROM nodes WHERE is_default = 1 LIMIT 1")
                .fetch_optional(&state.pool)
                .await
                .ok()
                .flatten();
        def.unwrap_or_else(|| pgpanel_docker::LOCAL_NODE_ID.to_string())
    };

    sqlx::query(
        r#"
        INSERT INTO clusters (
            id, name, slug, postgres_version, docker_container_id, docker_container_name,
            docker_volume_name, docker_network_name, internal_hostname, public_port,
            cpu_limit, memory_mb, storage_limit_gb, status, health, databasus_status,
            delete_protection, enable_backup, node_id, created_at, updated_at
        ) VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, 'creating', 'unknown',
                  'not_configured', 0, ?, ?, ?, ?)
        "#,
    )
    .bind(id.to_string())
    .bind(&req.name)
    .bind(&slug)
    .bind(&req.postgres_version)
    .bind(&names.container_name)
    .bind(&names.volume_name)
    .bind(&names.network_name)
    .bind(&names.internal_hostname)
    .bind(public_port)
    .bind(req.cpu_limit)
    .bind(req.memory_mb as i64)
    .bind(req.storage_limit_gb as i64)
    .bind(if req.enable_backup { 1 } else { 0 })
    .bind(&node_id)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    sqlx::query(
        r#"
        INSERT INTO cluster_credentials (id, cluster_id, role_name, username, password_encrypted, kind, created_at, updated_at)
        VALUES (?, ?, 'postgres', 'postgres', ?, 'admin', ?, ?)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(id.to_string())
    .bind(&enc)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let payload = serde_json::to_value(&req).unwrap_or_default();
    let operation_id = state
        .queue
        .enqueue(
            JobType::CreateCluster,
            Some(id),
            payload,
            Some(&format!("create-cluster-{id}")),
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::CLUSTER_CREATE,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"name": req.name, "version": req.postgres_version, "slug": slug}),
        None,
        None,
    )
    .await;

    if req.expose_publicly {
        write_audit(
            &state,
            Some(&auth.user),
            audit::PORT_PUBLISH,
            "cluster",
            Some(&id.to_string()),
            serde_json::json!({"port": req.optional_public_port}),
            None,
            None,
        )
        .await;
    }

    let cluster = load_cluster(&state, id).await?;
    let plaintext = admin_password.expose_secret().to_string();

    Ok(Json(ClusterCreatedResponse {
        cluster,
        operation_id,
        admin_password: plaintext.clone(),
        connection_info: ConnectionInfo {
            host: names.internal_hostname,
            port: req.optional_public_port.unwrap_or(5432),
            user: "postgres".into(),
            database: "postgres".into(),
            password: Some(plaintext),
        },
    }))
}

async fn start_cluster(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let op = enqueue_lifecycle(&state.queue, JobType::StartCluster, id).await?;
    write_audit(
        &state,
        Some(&auth.user),
        audit::CLUSTER_START,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({}),
        None,
        None,
    )
    .await;
    Ok(Json(serde_json::json!({"operation_id": op})))
}

async fn stop_cluster(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let op = enqueue_lifecycle(&state.queue, JobType::StopCluster, id).await?;
    write_audit(
        &state,
        Some(&auth.user),
        audit::CLUSTER_STOP,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({}),
        None,
        None,
    )
    .await;
    Ok(Json(serde_json::json!({"operation_id": op})))
}

async fn restart_cluster(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    let op = enqueue_lifecycle(&state.queue, JobType::RestartCluster, id).await?;
    write_audit(
        &state,
        Some(&auth.user),
        audit::CLUSTER_RESTART,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({}),
        None,
        None,
    )
    .await;
    Ok(Json(serde_json::json!({"operation_id": op})))
}

async fn delete_cluster(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<DeleteClusterRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let cluster = load_cluster(&state, id).await?;

    if cluster.delete_protection && req.mode == DeleteMode::PermanentlyDelete {
        return Err(AppError(Error::DeleteProtection));
    }

    if req.mode == DeleteMode::PermanentlyDelete {
        match &req.confirm_name {
            Some(n) if n == &cluster.name => {}
            _ => {
                return Err(AppError(Error::ConfirmationRequired(
                    "confirm_name must match cluster name exactly".into(),
                )));
            }
        }
        if !req.confirm_volume_delete {
            return Err(AppError(Error::ConfirmationRequired(
                "confirm_volume_delete must be true for permanent delete".into(),
            )));
        }
    }

    let payload = serde_json::to_value(&req).unwrap_or_default();
    let op = state
        .queue
        .enqueue(
            JobType::DeleteCluster,
            Some(id),
            payload,
            Some(&format!("delete-cluster-{id}-{:?}", req.mode)),
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::CLUSTER_DELETE,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({"mode": format!("{:?}", req.mode), "name": cluster.name}),
        None,
        None,
    )
    .await;

    if req.mode == DeleteMode::PermanentlyDelete {
        write_audit(
            &state,
            Some(&auth.user),
            audit::VOLUME_DELETE,
            "volume",
            Some(&cluster.docker_volume_name),
            serde_json::json!({"cluster_id": id}),
            None,
            None,
        )
        .await;
    }

    Ok(Json(serde_json::json!({"operation_id": op})))
}

async fn dashboard(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<DashboardStats>> {
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
    let degraded_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM clusters WHERE status IN ('degraded', 'failed', 'healthy_with_backup_warning')",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    let database_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM databases")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    let active_operations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operations WHERE status IN ('queued', 'running', 'waiting')",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    let failed_operations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM operations WHERE status = 'failed'")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

    Ok(Json(DashboardStats {
        cluster_count,
        healthy_count,
        degraded_count,
        database_count,
        active_operations,
        failed_operations,
        node_count: sqlx::query_scalar("SELECT COUNT(*) FROM nodes")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0),
        open_alerts: sqlx::query_scalar("SELECT COUNT(*) FROM alerts WHERE status = 'open'")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0),
        replica_count: sqlx::query_scalar(
            "SELECT COUNT(*) FROM cluster_replicas WHERE enabled = 1",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0),
        backup_destinations: sqlx::query_scalar(
            "SELECT COUNT(*) FROM backup_destinations WHERE enabled = 1",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0),
    }))
}

async fn enqueue_lifecycle(queue: &JobQueue, jt: JobType, id: Uuid) -> Result<Uuid, AppError> {
    queue
        .enqueue(
            jt,
            Some(id),
            serde_json::json!({}),
            Some(&format!("{}-{id}", jt.as_str())),
        )
        .await
        .map_err(AppError)
}

pub async fn load_cluster(state: &AppState, id: Uuid) -> Result<Cluster, AppError> {
    let row = sqlx::query_as::<_, ClusterRow>(
        r#"
        SELECT id, name, slug, postgres_version, docker_container_id, docker_container_name,
               docker_volume_name, docker_network_name, internal_hostname, public_port,
               cpu_limit, memory_mb, storage_limit_gb, status, health, databasus_status,
               delete_protection, enable_backup, last_error, node_id, created_at, updated_at
        FROM clusters WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?
    .ok_or_else(|| AppError(Error::NotFound("cluster".into())))?;
    row.into_cluster().map_err(AppError)
}

#[derive(sqlx::FromRow)]
struct ClusterRow {
    id: String,
    name: String,
    slug: String,
    postgres_version: String,
    docker_container_id: Option<String>,
    docker_container_name: String,
    docker_volume_name: String,
    docker_network_name: String,
    internal_hostname: String,
    public_port: Option<i64>,
    cpu_limit: f64,
    memory_mb: i64,
    storage_limit_gb: i64,
    status: String,
    health: String,
    databasus_status: String,
    delete_protection: i64,
    enable_backup: i64,
    last_error: Option<String>,
    node_id: Option<String>,
    created_at: String,
    updated_at: String,
}

impl ClusterRow {
    fn into_cluster(self) -> Result<Cluster, Error> {
        Ok(Cluster {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            name: self.name,
            slug: self.slug,
            postgres_version: self.postgres_version,
            docker_container_id: self.docker_container_id,
            docker_container_name: self.docker_container_name,
            docker_volume_name: self.docker_volume_name,
            docker_network_name: self.docker_network_name,
            internal_hostname: self.internal_hostname,
            public_port: self.public_port.map(|p| p as u16),
            cpu_limit: self.cpu_limit,
            memory_mb: self.memory_mb as u32,
            storage_limit_gb: self.storage_limit_gb as u32,
            status: ClusterStatus::parse(&self.status).unwrap_or(ClusterStatus::Failed),
            health: HealthStatus::parse(&self.health),
            databasus_status: DatabasusIntegrationStatus::parse(&self.databasus_status),
            delete_protection: self.delete_protection != 0,
            enable_backup: self.enable_backup != 0,
            last_error: self.last_error,
            node_id: self
                .node_id
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok()),
            created_at: parse_dt(&self.created_at),
            updated_at: parse_dt(&self.updated_at),
        })
    }
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
