//! Multi-node Docker host management.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use secrecy::ExposeSecret;
use uuid::Uuid;

use pgpanel_core::audit;
use pgpanel_core::crypto::encrypt_secret;
use pgpanel_core::error::Error;
use pgpanel_core::models::*;
use pgpanel_core::validation::{slugify, validate_display_name, validate_safe_name};
use pgpanel_docker::LOCAL_NODE_ID;

use crate::auth::{write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/nodes", get(list_nodes).post(create_node))
        .route("/api/nodes/{id}", get(get_node).put(update_node).delete(delete_node))
        .route("/api/nodes/{id}/ping", post(ping_node))
}

#[derive(sqlx::FromRow)]
struct NodeRow {
    id: String,
    name: String,
    slug: String,
    kind: String,
    docker_host: Option<String>,
    docker_host_encrypted: i64,
    labels: String,
    status: String,
    last_seen_at: Option<String>,
    last_error: Option<String>,
    max_clusters: Option<i64>,
    notes: Option<String>,
    is_default: i64,
    created_at: String,
    updated_at: String,
    cluster_count: i64,
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn display_host(raw: &Option<String>, encrypted: bool) -> Option<String> {
    let Some(h) = raw else {
        return None;
    };
    if encrypted || h.len() > 80 {
        // Don't leak encrypted blobs; show kind only
        if h.starts_with("unix://") {
            return Some("unix://***".into());
        }
        if let Some(rest) = h.strip_prefix("tcp://") {
            let host = rest.split('/').next().unwrap_or("***");
            let host = host.split('@').next_back().unwrap_or(host);
            return Some(format!("tcp://{host}"));
        }
        return Some("***configured***".into());
    }
    Some(h.clone())
}

impl NodeRow {
    fn into_node(self) -> Result<Node, Error> {
        Ok(Node {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            name: self.name,
            slug: self.slug,
            kind: NodeKind::parse(&self.kind),
            docker_host_display: display_host(&self.docker_host, self.docker_host_encrypted != 0),
            status: NodeStatus::parse(&self.status),
            last_seen_at: self.last_seen_at.as_ref().map(|s| parse_dt(s)),
            last_error: self.last_error,
            max_clusters: self.max_clusters.map(|n| n as u32),
            cluster_count: self.cluster_count.max(0) as u32,
            notes: self.notes,
            is_default: self.is_default != 0,
            labels: serde_json::from_str(&self.labels).unwrap_or_else(|_| serde_json::json!({})),
            created_at: parse_dt(&self.created_at),
            updated_at: parse_dt(&self.updated_at),
        })
    }
}

const NODE_SELECT: &str = r#"
    SELECT n.id, n.name, n.slug, n.kind, n.docker_host, n.docker_host_encrypted, n.labels,
           n.status, n.last_seen_at, n.last_error, n.max_clusters, n.notes, n.is_default,
           n.created_at, n.updated_at,
           (SELECT COUNT(*) FROM clusters c WHERE c.node_id = n.id) AS cluster_count
    FROM nodes n
"#;

async fn list_nodes(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<Node>>> {
    let rows = sqlx::query_as::<_, NodeRow>(&format!("{NODE_SELECT} ORDER BY n.is_default DESC, n.name"))
        .fetch_all(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_node().ok())
            .collect(),
    ))
}

async fn get_node(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Node>> {
    let row = sqlx::query_as::<_, NodeRow>(&format!("{NODE_SELECT} WHERE n.id = ?"))
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?
        .ok_or_else(|| AppError(Error::NotFound("node".into())))?;
    Ok(Json(row.into_node().map_err(AppError)?))
}

async fn create_node(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateNodeRequest>,
) -> ApiResult<Json<Node>> {
    validate_display_name(&req.name).map_err(AppError)?;
    let slug = slugify(&req.name);
    validate_safe_name(&slug, "node slug").map_err(AppError)?;

    let host = req.docker_host.trim();
    if !(host.starts_with("unix://")
        || host.starts_with("tcp://")
        || host.starts_with("http://")
        || host.starts_with("https://"))
    {
        return Err(AppError(Error::Validation(
            "docker_host must be unix://, tcp://, http:// or https://".into(),
        )));
    }

    // Probe before saving
    state
        .nodes
        .ping_host(Some(host))
        .await
        .map_err(|e| AppError(Error::Validation(format!("Docker host unreachable: {e}"))))?;

    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let enc = encrypt_secret(
        &state.config.master_encryption_key,
        &secrecy::SecretString::from(host.to_string()),
    )
    .map_err(AppError)?;
    let labels = req
        .labels
        .unwrap_or_else(|| serde_json::json!({}))
        .to_string();

    if req.set_default {
        sqlx::query("UPDATE nodes SET is_default = 0")
            .execute(&state.pool)
            .await
            .ok();
    }

    sqlx::query(
        r#"
        INSERT INTO nodes (
            id, name, slug, kind, docker_host, docker_host_encrypted, labels,
            status, last_seen_at, max_clusters, notes, is_default, created_at, updated_at
        ) VALUES (?, ?, ?, 'remote', ?, 1, ?, 'online', ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(id.to_string())
    .bind(req.name.trim())
    .bind(&slug)
    .bind(&enc)
    .bind(&labels)
    .bind(&now)
    .bind(req.max_clusters.map(|n| n as i64))
    .bind(req.notes.as_deref())
    .bind(if req.set_default { 1 } else { 0 })
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError(Error::Conflict("node name or slug already exists".into()))
        } else {
            AppError(Error::Internal(e.to_string()))
        }
    })?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::NODE_CREATE,
        "node",
        Some(&id.to_string()),
        serde_json::json!({"name": req.name, "slug": slug}),
        None,
        None,
    )
    .await;

    get_node(State(state), auth, Path(id)).await
}

async fn update_node(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateNodeRequest>,
) -> ApiResult<Json<Node>> {
    let existing = sqlx::query_as::<_, NodeRow>(&format!("{NODE_SELECT} WHERE n.id = ?"))
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?
        .ok_or_else(|| AppError(Error::NotFound("node".into())))?;

    if existing.kind == "local" && req.docker_host.is_some() {
        return Err(AppError(Error::Validation(
            "cannot change docker_host on the built-in local node".into(),
        )));
    }

    let now = Utc::now().to_rfc3339();
    let name = req
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&existing.name);
    if req.name.is_some() {
        validate_display_name(name).map_err(AppError)?;
    }

    let mut docker_host = existing.docker_host.clone();
    let mut encrypted = existing.docker_host_encrypted;
    if let Some(host) = req.docker_host.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !(host.starts_with("unix://")
            || host.starts_with("tcp://")
            || host.starts_with("http://")
            || host.starts_with("https://"))
        {
            return Err(AppError(Error::Validation(
                "docker_host must be unix://, tcp://, http:// or https://".into(),
            )));
        }
        state
            .nodes
            .ping_host(Some(host))
            .await
            .map_err(|e| AppError(Error::Validation(format!("Docker host unreachable: {e}"))))?;
        let enc = encrypt_secret(
            &state.config.master_encryption_key,
            &secrecy::SecretString::from(host.to_string()),
        )
        .map_err(AppError)?;
        docker_host = Some(enc);
        encrypted = 1;
        state.nodes.invalidate(id).await;
    }

    if req.set_default {
        sqlx::query("UPDATE nodes SET is_default = 0")
            .execute(&state.pool)
            .await
            .ok();
    }

    let status = req
        .status
        .map(|s| s.as_str().to_string())
        .unwrap_or(existing.status);
    let labels = req
        .labels
        .map(|v| v.to_string())
        .unwrap_or(existing.labels);
    let notes = req.notes.or(existing.notes);
    let max_clusters = req
        .max_clusters
        .map(|n| n as i64)
        .or(existing.max_clusters);
    let is_default = if req.set_default {
        1
    } else {
        existing.is_default
    };

    sqlx::query(
        r#"
        UPDATE nodes SET name = ?, docker_host = ?, docker_host_encrypted = ?, labels = ?,
            status = ?, max_clusters = ?, notes = ?, is_default = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(name)
    .bind(&docker_host)
    .bind(encrypted)
    .bind(&labels)
    .bind(&status)
    .bind(max_clusters)
    .bind(&notes)
    .bind(is_default)
    .bind(&now)
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::NODE_UPDATE,
        "node",
        Some(&id.to_string()),
        serde_json::json!({"name": name}),
        None,
        None,
    )
    .await;

    get_node(State(state), auth, Path(id)).await
}

async fn delete_node(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    if id.to_string() == LOCAL_NODE_ID {
        return Err(AppError(Error::Validation(
            "cannot delete the built-in local node".into(),
        )));
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM clusters WHERE node_id = ?")
        .bind(id.to_string())
        .fetch_one(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if count > 0 {
        return Err(AppError(Error::Conflict(format!(
            "node still has {count} cluster(s); move or delete them first"
        ))));
    }

    let res = sqlx::query("DELETE FROM nodes WHERE id = ? AND kind = 'remote'")
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if res.rows_affected() == 0 {
        return Err(AppError(Error::NotFound("node".into())));
    }
    state.nodes.invalidate(id).await;

    write_audit(
        &state,
        Some(&auth.user),
        audit::NODE_DELETE,
        "node",
        Some(&id.to_string()),
        serde_json::json!({}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"ok": true})))
}

async fn ping_node(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = sqlx::query_as::<_, (String, Option<String>, i64)>(
        "SELECT kind, docker_host, docker_host_encrypted FROM nodes WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?
    .ok_or_else(|| AppError(Error::NotFound("node".into())))?;

    let host = if row.0 == "local" {
        None
    } else if row.2 != 0 {
        let enc = row.1.ok_or_else(|| AppError(Error::Internal("missing docker_host".into())))?;
        let plain = pgpanel_core::crypto::decrypt_secret(&state.config.master_encryption_key, &enc)
            .map_err(AppError)?;
        Some(plain.expose_secret().to_string())
    } else {
        row.1
    };

    let now = Utc::now().to_rfc3339();
    match state.nodes.ping_host(host.as_deref()).await {
        Ok(()) => {
            sqlx::query(
                "UPDATE nodes SET status = 'online', last_seen_at = ?, last_error = NULL, updated_at = ? WHERE id = ?",
            )
            .bind(&now)
            .bind(&now)
            .bind(id.to_string())
            .execute(&state.pool)
            .await
            .ok();
            Ok(Json(serde_json::json!({"ok": true, "status": "online"})))
        }
        Err(e) => {
            sqlx::query(
                "UPDATE nodes SET status = 'offline', last_error = ?, updated_at = ? WHERE id = ?",
            )
            .bind(e.to_string())
            .bind(&now)
            .bind(id.to_string())
            .execute(&state.pool)
            .await
            .ok();
            let _ = auth;
            Err(AppError(Error::Validation(format!("ping failed: {e}"))))
        }
    }
}

/// Resolve Docker endpoint for a cluster's node (used by jobs / API).
#[allow(dead_code)]
pub async fn resolve_node_docker_host(
    state: &AppState,
    node_id: Option<Uuid>,
) -> Result<Option<String>, AppError> {
    let id = node_id
        .unwrap_or_else(pgpanel_docker::NodeRegistry::local_node_id)
        .to_string();
    let row = sqlx::query_as::<_, (String, Option<String>, i64)>(
        "SELECT kind, docker_host, docker_host_encrypted FROM nodes WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let Some((kind, host, enc)) = row else {
        return Ok(None);
    };
    if kind == "local" {
        return Ok(None);
    }
    if enc != 0 {
        let enc = host.ok_or_else(|| AppError(Error::Internal("missing docker_host".into())))?;
        let plain = pgpanel_core::crypto::decrypt_secret(&state.config.master_encryption_key, &enc)
            .map_err(AppError)?;
        Ok(Some(plain.expose_secret().to_string()))
    } else {
        Ok(host)
    }
}
