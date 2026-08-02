//! API token management.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use pgpanel_core::audit;
use pgpanel_core::error::Error;
use pgpanel_core::models::*;
use pgpanel_core::validation::validate_display_name;

use crate::auth::{write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tokens", get(list_tokens).post(create_token))
        .route("/api/tokens/{id}", axum::routing::delete(revoke_token))
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

fn generate_api_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("pgp_{}", hex::encode(bytes))
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    id: String,
    name: String,
    token_prefix: String,
    role: String,
    scopes: String,
    expires_at: Option<String>,
    last_used_at: Option<String>,
    revoked_at: Option<String>,
    created_at: String,
    user_id: String,
}

impl TokenRow {
    fn into_info(self) -> Result<ApiTokenInfo, Error> {
        Ok(ApiTokenInfo {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            name: self.name,
            token_prefix: self.token_prefix,
            role: self.role,
            scopes: self.scopes,
            expires_at: self.expires_at.as_ref().map(|s| parse_dt(s)),
            last_used_at: self.last_used_at.as_ref().map(|s| parse_dt(s)),
            revoked_at: self.revoked_at.as_ref().map(|s| parse_dt(s)),
            created_at: parse_dt(&self.created_at),
        })
    }
}

async fn list_tokens(
    State(state): State<AppState>,
    auth: AuthUser,
) -> ApiResult<Json<Vec<ApiTokenInfo>>> {
    let rows = if auth.user.role.can_admin() {
        sqlx::query_as::<_, TokenRow>(
            r#"
            SELECT id, name, token_prefix, role, scopes, expires_at, last_used_at,
                   revoked_at, created_at, user_id
            FROM api_tokens
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, TokenRow>(
            r#"
            SELECT id, name, token_prefix, role, scopes, expires_at, last_used_at,
                   revoked_at, created_at, user_id
            FROM api_tokens
            WHERE user_id = ?
            ORDER BY created_at DESC
            "#,
        )
        .bind(auth.user.id.to_string())
        .fetch_all(&state.pool)
        .await
    }
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_info().ok())
            .collect(),
    ))
}

async fn create_token(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateApiTokenRequest>,
) -> ApiResult<Json<ApiTokenCreated>> {
    validate_display_name(&req.name).map_err(AppError)?;

    let role = req.role.trim().to_lowercase();
    if !matches!(role.as_str(), "admin" | "operator" | "viewer") {
        return Err(AppError(Error::Validation(
            "token role must be admin, operator, or viewer".into(),
        )));
    }

    let token = generate_api_token();
    let token_hash = hash_token(&token);
    let token_prefix: String = token.chars().take(12).collect();
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let expires_at = req
        .expires_days
        .map(|d| (Utc::now() + Duration::days(d as i64)).to_rfc3339());

    sqlx::query(
        r#"
        INSERT INTO api_tokens (
            id, name, token_hash, token_prefix, user_id, role, scopes,
            expires_at, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(id.to_string())
    .bind(req.name.trim())
    .bind(&token_hash)
    .bind(&token_prefix)
    .bind(auth.user.id.to_string())
    .bind(&role)
    .bind(req.scopes.trim())
    .bind(&expires_at)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::TOKEN_CREATE,
        "api_token",
        Some(&id.to_string()),
        serde_json::json!({"name": req.name, "role": role}),
        None,
        None,
    )
    .await;

    Ok(Json(ApiTokenCreated {
        id,
        name: req.name.trim().to_string(),
        token,
        token_prefix,
        role,
        scopes: req.scopes.trim().to_string(),
        expires_at: expires_at.as_ref().map(|s| parse_dt(s)),
    }))
}

async fn revoke_token(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = sqlx::query_as::<_, TokenRow>(
        r#"
        SELECT id, name, token_prefix, role, scopes, expires_at, last_used_at,
               revoked_at, created_at, user_id
        FROM api_tokens WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?
    .ok_or_else(|| AppError(Error::NotFound("api token".into())))?;

    if !auth.user.role.can_admin() && row.user_id != auth.user.id.to_string() {
        return Err(AppError(Error::Forbidden("cannot revoke this token".into())));
    }

    if row.revoked_at.is_some() {
        return Ok(Json(serde_json::json!({"ok": true, "already_revoked": true})));
    }

    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE api_tokens SET revoked_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::TOKEN_REVOKE,
        "api_token",
        Some(&id.to_string()),
        serde_json::json!({"name": row.name}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"ok": true})))
}
