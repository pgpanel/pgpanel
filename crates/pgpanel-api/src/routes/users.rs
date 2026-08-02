//! User administration (RBAC).

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use secrecy::SecretString;
use uuid::Uuid;

use pgpanel_core::audit;
use pgpanel_core::crypto::{hash_password, validate_password_strength};
use pgpanel_core::error::Error;
use pgpanel_core::models::*;

use crate::auth::{require_admin, write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/users", get(list_users).post(create_user))
        .route(
            "/api/users/{id}",
            get(get_user).put(update_user).delete(delete_user),
        )
}

const USER_SELECT: &str = r#"
    SELECT id, username, email, role, display_name, enabled, created_at, last_login_at
    FROM users
"#;

#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    username: String,
    email: String,
    role: String,
    display_name: String,
    enabled: i64,
    created_at: String,
    last_login_at: Option<String>,
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

impl UserRow {
    fn into_user(self) -> Result<User, Error> {
        Ok(User {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            username: self.username,
            email: self.email,
            role: UserRole::parse(&self.role),
            display_name: self.display_name,
            enabled: self.enabled != 0,
            created_at: parse_dt(&self.created_at),
            last_login_at: self.last_login_at.as_ref().map(|s| parse_dt(s)),
        })
    }
}

fn is_valid_email(email: &str) -> bool {
    let email = email.trim();
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.chars().any(char::is_whitespace)
}

async fn count_owners(state: &AppState) -> Result<i64, AppError> {
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'owner' AND enabled = 1")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(n)
}

fn ensure_role_assignable(actor: &User, role: UserRole) -> Result<(), AppError> {
    if role == UserRole::Owner && actor.role != UserRole::Owner {
        return Err(AppError(Error::Forbidden(
            "only an owner can assign the owner role".into(),
        )));
    }
    Ok(())
}

async fn list_users(State(state): State<AppState>, auth: AuthUser) -> ApiResult<Json<Vec<User>>> {
    require_admin(&auth)?;
    let rows = sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} ORDER BY username"))
        .fetch_all(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_user().ok())
            .collect(),
    ))
}

async fn get_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<User>> {
    require_admin(&auth)?;
    let row = sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} WHERE id = ?"))
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?
        .ok_or_else(|| AppError(Error::NotFound("user".into())))?;
    Ok(Json(row.into_user().map_err(AppError)?))
}

async fn create_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateUserRequest>,
) -> ApiResult<Json<User>> {
    require_admin(&auth)?;
    ensure_role_assignable(&auth.user, req.role)?;

    let username = req.username.trim();
    if username.len() < 3 || username.len() > 64 {
        return Err(AppError(Error::Validation(
            "username must be 3-64 characters".into(),
        )));
    }
    if !is_valid_email(&req.email) {
        return Err(AppError(Error::Validation(
            "a valid e-mail address is required".into(),
        )));
    }
    validate_password_strength(&req.password).map_err(AppError)?;

    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let hash = hash_password(&SecretString::from(req.password.clone()))?;
    let display_name = if req.display_name.trim().is_empty() {
        username.to_string()
    } else {
        req.display_name.trim().to_string()
    };

    sqlx::query(
        r#"
        INSERT INTO users (
            id, username, email, password_hash, failed_login_attempts, role,
            display_name, enabled, created_at, updated_at, created_by
        ) VALUES (?, ?, ?, ?, 0, ?, ?, 1, ?, ?, ?)
        "#,
    )
    .bind(id.to_string())
    .bind(username)
    .bind(req.email.trim())
    .bind(&hash)
    .bind(req.role.as_str())
    .bind(&display_name)
    .bind(&now)
    .bind(&now)
    .bind(auth.user.id.to_string())
    .execute(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError(Error::Conflict("username or email already exists".into()))
        } else {
            AppError(Error::Internal(e.to_string()))
        }
    })?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::USER_CREATE,
        "user",
        Some(&id.to_string()),
        serde_json::json!({"username": username, "role": req.role.as_str()}),
        None,
        None,
    )
    .await;

    get_user(State(state), auth, Path(id)).await
}

async fn update_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateUserRequest>,
) -> ApiResult<Json<User>> {
    require_admin(&auth)?;

    let existing = sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} WHERE id = ?"))
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?
        .ok_or_else(|| AppError(Error::NotFound("user".into())))?;

    if let Some(role) = req.role {
        ensure_role_assignable(&auth.user, role)?;
        if existing.role == "owner"
            && role != UserRole::Owner
            && auth.user.id != id
            && count_owners(&state).await? <= 1
        {
            return Err(AppError(Error::Conflict(
                "cannot demote the last owner".into(),
            )));
        }
    }

    let email = req
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&existing.email);
    if req.email.is_some() && !is_valid_email(email) {
        return Err(AppError(Error::Validation(
            "a valid e-mail address is required".into(),
        )));
    }

    let existing_role = existing.role.clone();
    let role = req
        .role
        .map(|r| r.as_str().to_string())
        .unwrap_or_else(|| existing_role.clone());
    let display_name = req
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&existing.display_name);
    let enabled = req.enabled.unwrap_or(existing.enabled != 0);

    if existing_role == "owner" && !enabled && count_owners(&state).await? <= 1 {
        return Err(AppError(Error::Conflict(
            "cannot disable the last owner".into(),
        )));
    }

    let now = Utc::now().to_rfc3339();
    let mut password_hash: Option<String> = None;
    if let Some(pw) = req.password.as_deref().filter(|s| !s.is_empty()) {
        validate_password_strength(pw).map_err(AppError)?;
        password_hash = Some(hash_password(&SecretString::from(pw.to_string()))?);
    }

    if password_hash.is_some() {
        sqlx::query(
            r#"
            UPDATE users SET email = ?, role = ?, display_name = ?, enabled = ?,
                password_hash = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(email)
        .bind(&role)
        .bind(display_name)
        .bind(if enabled { 1 } else { 0 })
        .bind(password_hash.as_ref().unwrap())
        .bind(&now)
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    } else {
        sqlx::query(
            r#"
            UPDATE users SET email = ?, role = ?, display_name = ?, enabled = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(email)
        .bind(&role)
        .bind(display_name)
        .bind(if enabled { 1 } else { 0 })
        .bind(&now)
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    }

    write_audit(
        &state,
        Some(&auth.user),
        audit::USER_UPDATE,
        "user",
        Some(&id.to_string()),
        serde_json::json!({"email": email, "role": role, "enabled": enabled}),
        None,
        None,
    )
    .await;

    get_user(State(state), auth, Path(id)).await
}

async fn delete_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&auth)?;

    if auth.user.id == id {
        return Err(AppError(Error::Validation(
            "cannot delete your own account".into(),
        )));
    }

    let existing = sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} WHERE id = ?"))
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?
        .ok_or_else(|| AppError(Error::NotFound("user".into())))?;

    if existing.role == "owner" && count_owners(&state).await? <= 1 {
        return Err(AppError(Error::Conflict(
            "cannot delete the last owner".into(),
        )));
    }

    let res = sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if res.rows_affected() == 0 {
        return Err(AppError(Error::NotFound("user".into())));
    }

    write_audit(
        &state,
        Some(&auth.user),
        audit::USER_DELETE,
        "user",
        Some(&id.to_string()),
        serde_json::json!({"username": existing.username}),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"ok": true})))
}
