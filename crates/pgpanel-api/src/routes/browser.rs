use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use secrecy::SecretString;
use uuid::Uuid;

use pgpanel_core::audit;
use pgpanel_core::crypto::decrypt_secret;
use pgpanel_core::error::Error;
use pgpanel_core::models::*;
use pgpanel_core::validation::validate_safe_name;
use pgpanel_postgres::{BrowserService, PgClient, SqlConsole};

use crate::auth::{write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::routes::clusters::load_cluster;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/clusters/{id}/schemas", get(list_schemas))
        .route("/api/clusters/{id}/tables", get(list_tables))
        .route(
            "/api/clusters/{id}/tables/{schema}/{table}",
            get(table_details),
        )
        .route(
            "/api/clusters/{id}/tables/{schema}/{table}/rows",
            get(table_rows),
        )
        .route("/api/clusters/{id}/query", post(sql_query))
}

#[derive(serde::Deserialize)]
struct DbQuery {
    database: Option<String>,
    schema: Option<String>,
}

async fn connect_db(
    state: &AppState,
    cluster_id: Uuid,
    database: &str,
) -> Result<(Cluster, PgClient), AppError> {
    let cluster = load_cluster(state, cluster_id).await?;
    if !matches!(
        cluster.status,
        ClusterStatus::Healthy | ClusterStatus::HealthyWithBackupWarning | ClusterStatus::Degraded
    ) {
        return Err(AppError(Error::ClusterState(
            "cluster is not available for queries".into(),
        )));
    }

    validate_safe_name(database, "database").map_err(AppError)?;

    let enc: String = sqlx::query_scalar(
        "SELECT password_encrypted FROM cluster_credentials WHERE cluster_id = ? AND role_name = 'postgres'",
    )
    .bind(cluster_id.to_string())
    .fetch_one(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let password = decrypt_secret(&state.config.master_encryption_key, &enc).map_err(AppError)?;

    let client = try_connect(state, &cluster, database, &password).await?;
    Ok((cluster, client))
}

async fn try_connect(
    state: &AppState,
    cluster: &Cluster,
    database: &str,
    password: &SecretString,
) -> Result<PgClient, AppError> {
    match PgClient::connect(
        &cluster.internal_hostname,
        5432,
        "postgres",
        password,
        database,
        state.config.default_statement_timeout_ms,
        state.config.default_lock_timeout_ms,
    )
    .await
    {
        Ok(c) => Ok(c),
        Err(e1) => {
            if let Some(port) = cluster.public_port {
                PgClient::connect(
                    "127.0.0.1",
                    port,
                    "postgres",
                    password,
                    database,
                    state.config.default_statement_timeout_ms,
                    state.config.default_lock_timeout_ms,
                )
                .await
                .map_err(AppError)
            } else {
                Err(AppError(e1))
            }
        }
    }
}

async fn list_schemas(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<DbQuery>,
) -> ApiResult<Json<Vec<SchemaInfo>>> {
    let database = q.database.as_deref().unwrap_or("postgres");
    let (_c, client) = connect_db(&state, id, database).await?;
    let schemas = BrowserService::new(&client)
        .list_schemas()
        .await
        .map_err(AppError)?;
    Ok(Json(schemas))
}

async fn list_tables(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<DbQuery>,
) -> ApiResult<Json<Vec<TableInfo>>> {
    let database = q.database.as_deref().unwrap_or("postgres");
    let (_c, client) = connect_db(&state, id, database).await?;
    let tables = BrowserService::new(&client)
        .list_tables(q.schema.as_deref())
        .await
        .map_err(AppError)?;
    Ok(Json(tables))
}

async fn table_details(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path((id, schema, table)): Path<(Uuid, String, String)>,
    Query(q): Query<DbQuery>,
) -> ApiResult<Json<TableDetails>> {
    let database = q.database.as_deref().unwrap_or("postgres");
    let (_c, client) = connect_db(&state, id, database).await?;
    let details = BrowserService::new(&client)
        .table_details(&schema, &table)
        .await
        .map_err(AppError)?;
    Ok(Json(details))
}

async fn table_rows(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path((id, schema, table)): Path<(Uuid, String, String)>,
    Query(q): Query<TableRowsQuery>,
) -> ApiResult<Json<TableRowsResponse>> {
    let (_c, client) = connect_db(&state, id, &q.database).await?;
    let rows = BrowserService::new(&client)
        .table_rows(&schema, &table, &q)
        .await
        .map_err(AppError)?;
    Ok(Json(rows))
}

async fn sql_query(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<SqlQueryRequest>,
) -> ApiResult<Json<SqlQueryResponse>> {
    let (_c, client) = connect_db(&state, id, &req.database).await?;
    let console = SqlConsole::new(
        &client,
        state.config.sql_console_max_rows,
        state.config.default_statement_timeout_ms,
        state.config.default_lock_timeout_ms,
        state.config.sql_admin_mode_enabled,
    );
    let result = console
        .execute(&req.sql, req.admin_mode)
        .await
        .map_err(AppError)?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::SQL_QUERY,
        "cluster",
        Some(&id.to_string()),
        serde_json::json!({
            "database": req.database,
            "sql_preview": req.sql.chars().take(200).collect::<String>(),
            "admin_mode": req.admin_mode,
            "row_count": result.row_count,
        }),
        None,
        None,
    )
    .await;

    Ok(Json(result))
}
