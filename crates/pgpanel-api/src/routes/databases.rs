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
            "/api/clusters/{id}/roles/{role}/databases",
            get(list_role_databases).put(set_role_databases),
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

    let role_name = match req.role_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(r) => {
            validate_safe_name(r, "role_name").map_err(AppError)?;
            r.to_string()
        }
        None => cluster.slug.clone(),
    };

    let mut payload_req = req;
    payload_req.role_name = Some(role_name.clone());
    // Creating a DB owned by the existing cluster user — never rotate its password.
    payload_req.generate_password = false;
    payload_req.password = None;

    let payload = serde_json::to_value(&payload_req).unwrap_or_default();
    let op = state
        .queue
        .enqueue(
            JobType::CreateDatabase,
            Some(id),
            payload,
            Some(&format!("create-db-{}-{}", id, payload_req.database_name)),
        )
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::DATABASE_CREATE,
        "database",
        Some(&payload_req.database_name),
        serde_json::json!({"cluster_id": id, "role": role_name}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "operation_id": op,
        "message": "Database creation queued."
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
    validate_safe_name(&req.role_name, "role_name").map_err(AppError)?;
    let cluster = load_cluster(&state, id).await?;

    let password = if req.generate_password || req.password.as_ref().map(|p| p.is_empty()).unwrap_or(true) {
        pgpanel_core::crypto::generate_password()
    } else {
        let p = req.password.clone().unwrap_or_default();
        if p.len() < 8 {
            return Err(AppError(Error::Validation(
                "password must be at least 8 characters".into(),
            )));
        }
        secrecy::SecretString::from(p)
    };

    let client = connect_admin(&state, &cluster).await?;
    let roles = pgpanel_postgres::RoleService::new(&client);
    if roles.role_exists(&req.role_name).await.map_err(AppError)? {
        return Err(AppError(Error::Conflict(format!(
            "role {} already exists",
            req.role_name
        ))));
    }
    roles
        .create_role(&req.role_name, &password, req.connection_limit)
        .await
        .map_err(AppError)?;

    let role_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO database_roles (id, cluster_id, name, is_superuser, can_login, connection_limit, created_at) VALUES (?,?,?,?,?,?,?)",
    )
    .bind(&role_id)
    .bind(id.to_string())
    .bind(&req.role_name)
    .bind(0)
    .bind(1)
    .bind(req.connection_limit.map(|n| n as i64))
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let enc = pgpanel_core::crypto::encrypt_secret(
        &state.config.master_encryption_key,
        &password,
    )
    .map_err(AppError)?;
    sqlx::query(
        "INSERT INTO cluster_credentials (id, cluster_id, role_name, username, password_encrypted, kind, created_at, updated_at) VALUES (?,?,?,?,?,?,?,?)
         ON CONFLICT(cluster_id, role_name) DO UPDATE SET password_encrypted=excluded.password_encrypted, updated_at=excluded.updated_at",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(id.to_string())
    .bind(&req.role_name)
    .bind(&req.role_name)
    .bind(enc)
    .bind("app")
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(format!("failed to store role password: {e}"))))?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::ROLE_CREATE,
        "role",
        Some(&req.role_name),
        serde_json::json!({"cluster_id": id}),
        None,
        None,
    )
    .await;

    use secrecy::ExposeSecret;
    Ok(Json(serde_json::json!({
        "id": role_id,
        "role_name": req.role_name,
        "password": password.expose_secret(),
    })))
}

#[derive(serde::Deserialize)]
struct RoleDatabasesBody {
    databases: Vec<String>,
}

async fn list_role_databases(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path((id, role)): Path<(Uuid, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    validate_safe_name(&role, "role").map_err(AppError)?;
    let cluster = load_cluster(&state, id).await?;
    let client = connect_admin(&state, &cluster).await?;
    let dbs = pgpanel_postgres::RoleService::new(&client)
        .list_connectable_databases(&role)
        .await
        .map_err(AppError)?;
    Ok(Json(serde_json::json!({ "databases": dbs })))
}

async fn set_role_databases(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, role)): Path<(Uuid, String)>,
    Json(body): Json<RoleDatabasesBody>,
) -> ApiResult<Json<serde_json::Value>> {
    validate_safe_name(&role, "role").map_err(AppError)?;
    for db in &body.databases {
        validate_safe_name(db, "database").map_err(AppError)?;
    }
    let cluster = load_cluster(&state, id).await?;
    let client = connect_admin(&state, &cluster).await?;
    pgpanel_postgres::RoleService::new(&client)
        .set_database_access(&role, &body.databases)
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::ROLE_UPDATE,
        "role",
        Some(&role),
        serde_json::json!({"cluster_id": id, "databases": body.databases}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "role": role,
        "databases": body.databases,
    })))
}

async fn connect_admin(
    state: &AppState,
    cluster: &pgpanel_core::models::Cluster,
) -> Result<pgpanel_postgres::PgClient, AppError> {
    use pgpanel_core::crypto::decrypt_secret;
    use pgpanel_postgres::PgClient;

    let enc: String = sqlx::query_scalar(
        "SELECT password_encrypted FROM cluster_credentials WHERE cluster_id = ? AND role_name = 'postgres'",
    )
    .bind(cluster.id.to_string())
    .fetch_one(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let password = decrypt_secret(&state.config.master_encryption_key, &enc).map_err(AppError)?;
    let host = &cluster.internal_hostname;
    match PgClient::connect(
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
        Ok(c) => Ok(c),
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
                .map_err(AppError)
            } else {
                Err(AppError(Error::Postgres(
                    "cannot connect to cluster".into(),
                )))
            }
        }
    }
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
