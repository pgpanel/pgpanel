use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use uuid::Uuid;

use pgpanel_core::audit;
use pgpanel_core::error::Error;
use pgpanel_core::models::*;
use pgpanel_core::validation::validate_safe_name;

use crate::auth::{write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::routes::clusters::load_cluster;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/clusters/{id}/databases",
            get(list_databases).post(create_database),
        )
        .route(
            "/api/clusters/{id}/databases/{database}",
            axum::routing::delete(delete_database),
        )
        .route(
            "/api/clusters/{id}/roles",
            get(list_roles).post(create_role),
        )
        .route(
            "/api/clusters/{id}/roles/{role}/password",
            axum::routing::put(change_password),
        )
        .route(
            "/api/clusters/{id}/roles/{role}",
            axum::routing::delete(delete_role),
        )
}

async fn list_databases(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<DatabaseRecord>>> {
    let _ = load_cluster(&state, id).await?;
    let rows = sqlx::query_as::<_, DbRow>(
        "SELECT id, cluster_id, name, owner_role, connection_limit, created_at FROM databases WHERE cluster_id = ? ORDER BY name",
    )
    .bind(id.to_string())
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_record().ok())
            .collect(),
    ))
}

async fn create_database(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateDatabaseRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let cluster = load_cluster(&state, id).await?;
    if !cluster.status.is_running() && cluster.status != ClusterStatus::Healthy {
        // Allow HealthyWithBackupWarning etc via is_running
        if !matches!(
            cluster.status,
            ClusterStatus::Healthy
                | ClusterStatus::HealthyWithBackupWarning
                | ClusterStatus::Degraded
        ) {
            return Err(AppError(Error::ClusterState(format!(
                "cluster is {:?}, must be healthy",
                cluster.status
            ))));
        }
    }

    validate_safe_name(&req.database_name, "database_name").map_err(AppError)?;
    validate_safe_name(&req.role_name, "role_name").map_err(AppError)?;

    let payload = serde_json::to_value(&req).unwrap_or_default();
    let op = state
        .queue
        .enqueue(
            JobType::CreateDatabase,
            Some(id),
            payload,
            Some(&format!("create-db-{}-{}", id, req.database_name)),
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::DATABASE_CREATE,
        "database",
        Some(&req.database_name),
        serde_json::json!({"cluster_id": id, "role": req.role_name}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "operation_id": op,
        "message": "Database creation queued. Password will be in operation result once complete."
    })))
}

async fn delete_database(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, database)): Path<(Uuid, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    validate_safe_name(&database, "database").map_err(AppError)?;

    let op = state
        .queue
        .enqueue(
            JobType::DeleteDatabase,
            Some(id),
            serde_json::json!({"database_name": database}),
            None,
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::DATABASE_DELETE,
        "database",
        Some(&database),
        serde_json::json!({"cluster_id": id}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"operation_id": op})))
}

async fn list_roles(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<RoleRecord>>> {
    let _ = load_cluster(&state, id).await?;
    let rows = sqlx::query_as::<_, RoleRow>(
        "SELECT id, cluster_id, name, is_superuser, can_login, connection_limit, created_at FROM database_roles WHERE cluster_id = ? ORDER BY name",
    )
    .bind(id.to_string())
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_record().ok())
            .collect(),
    ))
}

async fn create_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateRoleRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    // Create role without DB via create_database path is awkward;
    // enqueue as create_database is not right. For MVP, create role through
    // a password rotate-style job isn't available — use CreateDatabase job
    // pattern with a dedicated payload by reusing CreateDatabase with same names
    // is wrong. We'll enqueue RotatePassword-style via CreateDatabase minimal.
    // Simpler: treat as create_database without creating DB using job type CreateDatabase
    // is not ideal. For MVP map to create with temp approach:

    validate_safe_name(&req.role_name, "role_name").map_err(AppError)?;
    let _ = load_cluster(&state, id).await?;

    let role_name = req.role_name.clone();
    let payload = serde_json::json!({
        "database_name": format!("{}_db", role_name),
        "role_name": role_name,
        "generate_password": req.generate_password,
        "password": req.password,
        "connection_limit": req.connection_limit,
    });

    // MVP: creates role plus companion database `{role}_db`.
    let op = state
        .queue
        .enqueue(JobType::CreateDatabase, Some(id), payload, None)
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::ROLE_CREATE,
        "role",
        Some(&role_name),
        serde_json::json!({"cluster_id": id}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "operation_id": op,
        "note": "MVP creates role with companion database {role}_db"
    })))
}

async fn change_password(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, role)): Path<(Uuid, String)>,
    Json(req): Json<ChangePasswordRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let _ = load_cluster(&state, id).await?;
    validate_safe_name(&role, "role").map_err(AppError)?;

    let payload = serde_json::json!({
        "role_name": role,
        "generate_password": req.generate_password,
        "password": req.password,
    });

    let op = state
        .queue
        .enqueue(JobType::RotatePassword, Some(id), payload, None)
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::ROLE_PASSWORD_CHANGE,
        "role",
        Some(&role),
        serde_json::json!({"cluster_id": id}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "operation_id": op,
        "warning": "Update application connection strings after password rotation completes."
    })))
}

async fn delete_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, role)): Path<(Uuid, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    validate_safe_name(&role, "role").map_err(AppError)?;
    let cluster = load_cluster(&state, id).await?;

    // Synchronous drop for MVP (short operation)
    use pgpanel_core::crypto::decrypt_secret;
    use pgpanel_postgres::{PgClient, RoleService};
    use secrecy::SecretString;

    let enc: String = sqlx::query_scalar(
        "SELECT password_encrypted FROM cluster_credentials WHERE cluster_id = ? AND role_name = 'postgres'",
    )
    .bind(id.to_string())
    .fetch_one(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let password = decrypt_secret(&state.config.master_encryption_key, &enc).map_err(AppError)?;
    let host = &cluster.internal_hostname;
    let client = match PgClient::connect(
        host,
        5432,
        "postgres",
        &password,
        "postgres",
        state.config.default_statement_timeout_ms,
        state.config.default_lock_timeout_ms,
    )
    .await
    {
        Ok(c) => c,
        Err(_) => {
            if let Some(port) = cluster.public_port {
                PgClient::connect(
                    "127.0.0.1",
                    port,
                    "postgres",
                    &password,
                    "postgres",
                    state.config.default_statement_timeout_ms,
                    state.config.default_lock_timeout_ms,
                )
                .await
                .map_err(AppError)?
            } else {
                return Err(AppError(Error::Postgres(
                    "cannot connect to cluster".into(),
                )));
            }
        }
    };

    RoleService::new(&client)
        .drop_role(&role)
        .await
        .map_err(AppError)?;

    sqlx::query("DELETE FROM database_roles WHERE cluster_id = ? AND name = ?")
        .bind(id.to_string())
        .bind(&role)
        .execute(&state.pool)
        .await
        .ok();
    sqlx::query("DELETE FROM cluster_credentials WHERE cluster_id = ? AND role_name = ?")
        .bind(id.to_string())
        .bind(&role)
        .execute(&state.pool)
        .await
        .ok();

    write_audit(
        &state,
        Some(&auth.user),
        audit::ROLE_DELETE,
        "role",
        Some(&role),
        serde_json::json!({"cluster_id": id}),
        None,
        None,
    )
    .await;

    let _ = SecretString::from(String::new());
    Ok(Json(serde_json::json!({"deleted": role})))
}

#[derive(sqlx::FromRow)]
struct DbRow {
    id: String,
    cluster_id: String,
    name: String,
    owner_role: String,
    connection_limit: Option<i64>,
    created_at: String,
}

impl DbRow {
    fn into_record(self) -> Result<DatabaseRecord, Error> {
        Ok(DatabaseRecord {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            cluster_id: Uuid::parse_str(&self.cluster_id)
                .map_err(|e| Error::Internal(e.to_string()))?,
            name: self.name,
            owner_role: self.owner_role,
            connection_limit: self.connection_limit.map(|n| n as i32),
            created_at: chrono::DateTime::parse_from_rfc3339(&self.created_at)
                .map(|d| d.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
        })
    }
}

#[derive(sqlx::FromRow)]
struct RoleRow {
    id: String,
    cluster_id: String,
    name: String,
    is_superuser: i64,
    can_login: i64,
    connection_limit: Option<i64>,
    created_at: String,
}

impl RoleRow {
    fn into_record(self) -> Result<RoleRecord, Error> {
        Ok(RoleRecord {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            cluster_id: Uuid::parse_str(&self.cluster_id)
                .map_err(|e| Error::Internal(e.to_string()))?,
            name: self.name,
            is_superuser: self.is_superuser != 0,
            can_login: self.can_login != 0,
            connection_limit: self.connection_limit.map(|n| n as i32),
            created_at: chrono::DateTime::parse_from_rfc3339(&self.created_at)
                .map(|d| d.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
        })
    }
}
