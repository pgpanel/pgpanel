//! WAF policy management with full change-history tracking.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use pgpanel_core::audit;
use pgpanel_core::error::Error;
use pgpanel_core::models::*;

use crate::auth::{write_audit, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/waf/policies", get(list_policies).post(create_policy))
        .route(
            "/api/waf/policies/{id}",
            get(get_policy).put(update_policy),
        )
        .route("/api/waf/policies/{id}/activate", axum::routing::post(activate_policy))
        .route("/api/waf/active", get(get_active))
        .route("/api/waf/changes", get(list_changes))
        .route("/api/waf/policies/{id}/changes", get(list_policy_changes))
        .route("/api/waf/caddy-snippet", get(caddy_snippet))
}

#[derive(sqlx::FromRow)]
struct PolicyRow {
    id: String,
    name: String,
    enabled: i64,
    is_active: i64,
    version: i64,
    config_json: String,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

impl PolicyRow {
    fn into_policy(self) -> Result<WafPolicy, Error> {
        let config: WafPolicyConfig =
            serde_json::from_str(&self.config_json).unwrap_or_default();
        Ok(WafPolicy {
            id: Uuid::parse_str(&self.id).map_err(|e| Error::Internal(e.to_string()))?,
            name: self.name,
            enabled: self.enabled != 0,
            is_active: self.is_active != 0,
            version: self.version.max(0) as u32,
            config,
            notes: self.notes,
            created_at: parse_dt(&self.created_at),
            updated_at: parse_dt(&self.updated_at),
        })
    }
}

async fn list_policies(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<WafPolicy>>> {
    let rows = sqlx::query_as::<_, PolicyRow>(
        "SELECT id, name, enabled, is_active, version, config_json, notes, created_at, updated_at FROM waf_policies ORDER BY is_active DESC, name",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_policy().ok())
            .collect(),
    ))
}

async fn get_policy(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<WafPolicy>> {
    let row = sqlx::query_as::<_, PolicyRow>(
        "SELECT id, name, enabled, is_active, version, config_json, notes, created_at, updated_at FROM waf_policies WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?
    .ok_or_else(|| AppError(Error::NotFound("waf policy".into())))?;
    Ok(Json(row.into_policy().map_err(AppError)?))
}

async fn get_active(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<WafPolicy>> {
    let row = sqlx::query_as::<_, PolicyRow>(
        "SELECT id, name, enabled, is_active, version, config_json, notes, created_at, updated_at FROM waf_policies WHERE is_active = 1 LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?
    .ok_or_else(|| AppError(Error::NotFound("active waf policy".into())))?;
    Ok(Json(row.into_policy().map_err(AppError)?))
}

#[derive(Deserialize)]
struct CreatePolicyBody {
    name: String,
    #[serde(default)]
    config: WafPolicyConfig,
    notes: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

async fn create_policy(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreatePolicyBody>,
) -> ApiResult<Json<WafPolicy>> {
    let name = body.name.trim();
    if name.len() < 2 || name.len() > 64 {
        return Err(AppError(Error::Validation(
            "policy name must be 2-64 characters".into(),
        )));
    }
    validate_config(&body.config)?;

    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let config_json = serde_json::to_string(&body.config).unwrap_or_else(|_| "{}".into());

    sqlx::query(
        r#"
        INSERT INTO waf_policies (id, name, enabled, is_active, version, config_json, notes, created_at, updated_at)
        VALUES (?, ?, 1, 0, 1, ?, ?, ?, ?)
        "#,
    )
    .bind(id.to_string())
    .bind(name)
    .bind(&config_json)
    .bind(body.notes.as_deref())
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError(Error::Conflict("policy name already exists".into()))
        } else {
            AppError(Error::Internal(e.to_string()))
        }
    })?;

    record_change(
        &state,
        id,
        None,
        1,
        Some(&auth.user),
        "Created WAF policy",
        None,
        &config_json,
        body.reason.as_deref(),
        None,
    )
    .await?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::WAF_UPDATE,
        "waf_policy",
        Some(&id.to_string()),
        serde_json::json!({"action": "create", "name": name}),
        None,
        None,
    )
    .await;

    get_policy(State(state), auth, Path(id)).await
}

async fn update_policy(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateWafPolicyRequest>,
) -> ApiResult<Json<WafPolicy>> {
    validate_config(&body.config)?;
    let existing = sqlx::query_as::<_, PolicyRow>(
        "SELECT id, name, enabled, is_active, version, config_json, notes, created_at, updated_at FROM waf_policies WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?
    .ok_or_else(|| AppError(Error::NotFound("waf policy".into())))?;

    let new_version = existing.version + 1;
    let after = serde_json::to_string(&body.config).unwrap_or_else(|_| "{}".into());
    let enabled = body.enabled.map(|b| if b { 1 } else { 0 }).unwrap_or(existing.enabled);
    let notes = body.notes.or(existing.notes);
    let now = Utc::now().to_rfc3339();

    let summary = summarize_diff(&existing.config_json, &after);

    sqlx::query(
        "UPDATE waf_policies SET config_json = ?, enabled = ?, version = ?, notes = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&after)
    .bind(enabled)
    .bind(new_version)
    .bind(&notes)
    .bind(&now)
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    record_change(
        &state,
        id,
        Some(existing.version as u32),
        new_version as u32,
        Some(&auth.user),
        &summary,
        Some(&existing.config_json),
        &after,
        body.reason.as_deref(),
        None,
    )
    .await?;

    // Hot-reload in-memory WAF
    if let Ok(cfg) = serde_json::from_str::<WafPolicyConfig>(&after) {
        *state.waf.write().await = cfg;
    }

    write_audit(
        &state,
        Some(&auth.user),
        audit::WAF_UPDATE,
        "waf_policy",
        Some(&id.to_string()),
        serde_json::json!({"action": "update", "version": new_version, "summary": summary}),
        None,
        None,
    )
    .await;

    if body.activate {
        return activate_policy(State(state), auth, Path(id)).await;
    }

    get_policy(State(state), auth, Path(id)).await
}

async fn activate_policy(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<WafPolicy>> {
    let row = sqlx::query_as::<_, PolicyRow>(
        "SELECT id, name, enabled, is_active, version, config_json, notes, created_at, updated_at FROM waf_policies WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?
    .ok_or_else(|| AppError(Error::NotFound("waf policy".into())))?;

    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE waf_policies SET is_active = 0")
        .execute(&state.pool)
        .await
        .ok();
    sqlx::query("UPDATE waf_policies SET is_active = 1, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES ('waf.active_policy_id', ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(id.to_string())
    .bind(&now)
    .execute(&state.pool)
    .await
    .ok();

    if let Ok(cfg) = serde_json::from_str::<WafPolicyConfig>(&row.config_json) {
        *state.waf.write().await = cfg;
    }

    record_change(
        &state,
        id,
        Some(row.version as u32),
        row.version as u32,
        Some(&auth.user),
        "Activated WAF policy",
        Some(&row.config_json),
        &row.config_json,
        Some("activate"),
        None,
    )
    .await?;

    write_audit(
        &state,
        Some(&auth.user),
        audit::WAF_ACTIVATE,
        "waf_policy",
        Some(&id.to_string()),
        serde_json::json!({"name": row.name}),
        None,
        None,
    )
    .await;

    get_policy(State(state), auth, Path(id)).await
}

#[derive(Deserialize)]
struct ChangesQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    100
}

#[derive(sqlx::FromRow)]
struct ChangeRow {
    id: i64,
    policy_id: String,
    version_from: Option<i64>,
    version_to: i64,
    actor_username: Option<String>,
    change_summary: String,
    before_json: Option<String>,
    after_json: String,
    reason: Option<String>,
    ip_address: Option<String>,
    created_at: String,
}

impl ChangeRow {
    fn into_entry(self) -> Result<WafChangeEntry, Error> {
        Ok(WafChangeEntry {
            id: self.id,
            policy_id: Uuid::parse_str(&self.policy_id)
                .map_err(|e| Error::Internal(e.to_string()))?,
            version_from: self.version_from.map(|v| v as u32),
            version_to: self.version_to.max(0) as u32,
            actor_username: self.actor_username,
            change_summary: self.change_summary,
            before_json: self
                .before_json
                .and_then(|s| serde_json::from_str(&s).ok()),
            after_json: serde_json::from_str(&self.after_json)
                .unwrap_or_else(|_| serde_json::json!({})),
            reason: self.reason,
            ip_address: self.ip_address,
            created_at: parse_dt(&self.created_at),
        })
    }
}

async fn list_changes(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<ChangesQuery>,
) -> ApiResult<Json<Vec<WafChangeEntry>>> {
    let rows = sqlx::query_as::<_, ChangeRow>(
        r#"
        SELECT id, policy_id, version_from, version_to, actor_username, change_summary,
               before_json, after_json, reason, ip_address, created_at
        FROM waf_change_log ORDER BY created_at DESC LIMIT ?
        "#,
    )
    .bind(q.limit.clamp(1, 500))
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_entry().ok())
            .collect(),
    ))
}

async fn list_policy_changes(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<ChangesQuery>,
) -> ApiResult<Json<Vec<WafChangeEntry>>> {
    let rows = sqlx::query_as::<_, ChangeRow>(
        r#"
        SELECT id, policy_id, version_from, version_to, actor_username, change_summary,
               before_json, after_json, reason, ip_address, created_at
        FROM waf_change_log WHERE policy_id = ? ORDER BY created_at DESC LIMIT ?
        "#,
    )
    .bind(id.to_string())
    .bind(q.limit.clamp(1, 500))
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|r| r.into_entry().ok())
            .collect(),
    ))
}

async fn caddy_snippet(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    let cfg = state.waf.read().await.clone();
    let snippet = render_caddy_snippet(&cfg);
    Ok(Json(serde_json::json!({
        "snippet": snippet,
        "config": cfg,
        "note": "Paste under your site block in Caddyfile, then reload Caddy. Changes are tracked in /api/waf/changes."
    })))
}

fn render_caddy_snippet(cfg: &WafPolicyConfig) -> String {
    let mut lines = vec![
        "# Generated by PgPanel WAF — do not hand-edit without recording a change".to_string(),
        format!("request_body {{ max_size {} }}", cfg.max_body_bytes),
    ];
    if cfg.enable_security_headers {
        lines.push("header {".into());
        lines.push(format!(
            "\tStrict-Transport-Security \"max-age={}; includeSubDomains; preload\"",
            cfg.hsts_max_age
        ));
        lines.push("\tX-Content-Type-Options nosniff".into());
        lines.push("\tX-Frame-Options DENY".into());
        lines.push("\tReferrer-Policy no-referrer".into());
        if cfg.csp_mode == "strict" {
            lines.push("\tContent-Security-Policy \"default-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'\"".into());
        }
        lines.push("}".into());
    }
    for ip in &cfg.denied_ips {
        if !ip.is_empty() {
            lines.push(format!("@deny_{} remote_ip {}", ip.replace(['.', ':'], "_"), ip));
            lines.push(format!("respond @deny_{} \"Forbidden\" 403", ip.replace(['.', ':'], "_")));
        }
    }
    if !cfg.allowed_ips.is_empty() {
        let list = cfg.allowed_ips.join(" ");
        lines.push(format!("@not_allow not remote_ip {list}"));
        lines.push("respond @not_allow \"Forbidden\" 403".into());
    }
    for path in &cfg.blocked_paths {
        if !path.is_empty() {
            lines.push(format!("@block_path path {path}"));
            lines.push("respond @block_path \"Not Found\" 404".into());
        }
    }
    lines.push(format!(
        "# rate hint: {} req/min global, {} login/min, {} api/min (enforced in-panel)",
        cfg.rate_limit_per_minute, cfg.login_rate_limit_per_minute, cfg.api_rate_limit_per_minute
    ));
    lines.join("\n")
}

fn validate_config(cfg: &WafPolicyConfig) -> ApiResult<()> {
    if cfg.rate_limit_per_minute == 0 || cfg.rate_limit_per_minute > 100_000 {
        return Err(AppError(Error::Validation(
            "rate_limit_per_minute out of range".into(),
        )));
    }
    if cfg.max_body_bytes < 1024 || cfg.max_body_bytes > 100 * 1024 * 1024 {
        return Err(AppError(Error::Validation(
            "max_body_bytes must be between 1 KiB and 100 MiB".into(),
        )));
    }
    Ok(())
}

fn summarize_diff(before: &str, after: &str) -> String {
    let b: serde_json::Value = serde_json::from_str(before).unwrap_or_default();
    let a: serde_json::Value = serde_json::from_str(after).unwrap_or_default();
    let mut changed = Vec::new();
    if let (Some(bo), Some(ao)) = (b.as_object(), a.as_object()) {
        for (k, av) in ao {
            if bo.get(k) != Some(av) {
                changed.push(k.clone());
            }
        }
        for k in bo.keys() {
            if !ao.contains_key(k) {
                changed.push(k.clone());
            }
        }
    }
    if changed.is_empty() {
        "No field changes detected".into()
    } else {
        format!("Updated fields: {}", changed.join(", "))
    }
}

#[allow(clippy::too_many_arguments)]
async fn record_change(
    state: &AppState,
    policy_id: Uuid,
    version_from: Option<u32>,
    version_to: u32,
    actor: Option<&User>,
    summary: &str,
    before: Option<&str>,
    after: &str,
    reason: Option<&str>,
    ip: Option<&str>,
) -> ApiResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO waf_change_log (
            policy_id, version_from, version_to, actor_id, actor_username,
            change_summary, before_json, after_json, reason, ip_address, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(policy_id.to_string())
    .bind(version_from.map(|v| v as i64))
    .bind(version_to as i64)
    .bind(actor.map(|u| u.id.to_string()))
    .bind(actor.map(|u| u.username.as_str()))
    .bind(summary)
    .bind(before)
    .bind(after)
    .bind(reason)
    .bind(ip)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(())
}

/// Load active WAF config into memory at startup.
pub async fn load_active_waf(pool: &sqlx::SqlitePool) -> WafPolicyConfig {
    let json: Option<String> = sqlx::query_scalar(
        "SELECT config_json FROM waf_policies WHERE is_active = 1 LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    json.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
