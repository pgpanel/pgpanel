//! Fleet pairing — connect PgPanel instances with a single join link.
//!
//! Flow:
//! 1. Enable node mode on panel A → get a join link
//! 2. Paste that link on panel B → panels are paired
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use rand::RngCore;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::auth::{require_admin, AuthUser};
use crate::error::{ApiResult, AppError};
use crate::state::AppState;
use pgpanel_core::crypto::{decrypt_secret, encrypt_secret};
use pgpanel_core::error::Error;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/fleet/remote-access",
            get(remote_access).put(set_remote_access),
        )
        .route("/api/fleet/remote-access/rotate-token", post(rotate_token))
        .route("/api/fleet/invites", get(list_invites).post(create_invite))
        .route(
            "/api/fleet/invites/{id}",
            axum::routing::delete(revoke_invite),
        )
        .route("/api/fleet/join", post(join))
        .route("/api/fleet/peers", get(list_peers))
        .route("/api/fleet/peers/{id}", axum::routing::delete(remove_peer))
        .route("/api/fleet/peers/{id}/ping", post(ping_peer))
        .route("/api/fleet/agent/info", get(agent_info))
        .route("/api/fleet/agent/accept-invite", post(accept_invite))
}

fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn token() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    format!("fleet_{}", hex::encode(b))
}

fn prefix(s: &str) -> String {
    s.chars().take(12).collect()
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn join_url(base: &str, tok: &str) -> Option<String> {
    let b = base.trim().trim_end_matches('/');
    if b.is_empty() || tok.is_empty() {
        return None;
    }
    Some(format!("{b}/join?token={tok}"))
}

/// Parse a pasted join link or separate base_url + token.
/// Accepts: `https://host/join?token=fleet_…` or bare token with base_url.
fn parse_join_parts(link: Option<&str>, base_url: &str, token_in: &str) -> Result<(String, String), AppError> {
    if let Some(raw) = link.map(str::trim).filter(|s| !s.is_empty()) {
        let token_val = raw
            .split("token=")
            .nth(1)
            .map(|rest| rest.split('&').next().unwrap_or(rest).trim())
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                AppError(Error::Validation(
                    "Join link must include ?token=…".into(),
                ))
            })?;
        let base = if let Some(idx) = raw.find("/join") {
            raw[..idx].trim().trim_end_matches('/').to_string()
        } else if let Some(idx) = raw.find('?') {
            raw[..idx].trim().trim_end_matches('/').to_string()
        } else {
            return Err(AppError(Error::Validation(
                "Could not parse panel URL from join link".into(),
            )));
        };
        if base.is_empty() || !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err(AppError(Error::Validation(
                "Join link must start with http:// or https://".into(),
            )));
        }
        return Ok((base, token_val.to_string()));
    }

    let base = base_url.trim().trim_end_matches('/').to_string();
    let tok = token_in.trim().to_string();
    if base.is_empty() || tok.is_empty() {
        return Err(AppError(Error::Validation(
            "Paste a join link, or provide base_url and token".into(),
        )));
    }
    Ok((base, tok))
}

async fn store_agent_token(s: &AppState, raw: &str) -> Result<(), AppError> {
    let enc = encrypt_secret(
        &s.config.master_encryption_key,
        &SecretString::from(raw.to_string()),
    )
    .map_err(AppError)?;
    sqlx::query(
        "UPDATE panel_remote_access SET agent_token_hash=?, agent_token_prefix=?, agent_token_encrypted=?, updated_at=? WHERE id=1",
    )
    .bind(hash(raw))
    .bind(prefix(raw))
    .bind(enc)
    .bind(now())
    .execute(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(())
}

async fn load_agent_token(s: &AppState) -> Result<Option<String>, AppError> {
    let enc: Option<String> =
        sqlx::query_scalar("SELECT agent_token_encrypted FROM panel_remote_access WHERE id=1")
            .fetch_one(&s.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    match enc.filter(|e| !e.is_empty()) {
        Some(e) => {
            let plain = decrypt_secret(&s.config.master_encryption_key, &e).map_err(AppError)?;
            Ok(Some(plain.expose_secret().to_string()))
        }
        None => Ok(None),
    }
}

async fn access_json(s: &AppState) -> Result<serde_json::Value, AppError> {
    #[derive(sqlx::FromRow)]
    struct RemoteRow {
        enabled: i64,
        public_base_url: String,
        agent_token_prefix: Option<String>,
        updated_at: String,
    }
    let r = sqlx::query_as::<_, RemoteRow>(
        "SELECT enabled, public_base_url, agent_token_prefix, updated_at FROM panel_remote_access WHERE id=1",
    )
    .fetch_one(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let enabled = r.enabled != 0;
    let base = r.public_base_url.clone();
    let raw = if enabled {
        load_agent_token(s).await?
    } else {
        None
    };
    let join = raw.as_ref().and_then(|t| join_url(&base, t));

    let public_name: String =
        sqlx::query_scalar("SELECT value FROM settings WHERE key='fleet.public_name'")
            .fetch_optional(&s.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?
            .unwrap_or_else(|| "PgPanel node".into());

    Ok(serde_json::json!({
        "enabled": enabled,
        "public_base_url": base,
        "public_name": public_name,
        "has_token": r.agent_token_prefix.is_some() || raw.is_some(),
        "token_prefix": r.agent_token_prefix,
        "join_url": join,
        "join_token": raw,
        "updated_at": r.updated_at,
    }))
}

#[derive(serde::Deserialize)]
struct RemoteBody {
    enabled: bool,
    #[serde(default)]
    public_base_url: String,
    #[serde(default)]
    public_name: Option<String>,
}

async fn remote_access(
    State(s): State<AppState>,
    _a: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(access_json(&s).await?))
}

async fn set_remote_access(
    State(s): State<AppState>,
    a: AuthUser,
    Json(b): Json<RemoteBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    let base = b.public_base_url.trim().trim_end_matches('/').to_string();
    if b.enabled && base.is_empty() {
        return Err(AppError(Error::Validation(
            "Public URL is required to enable node mode (e.g. https://db.example.com)".into(),
        )));
    }

    if let Some(name) = b.public_name.as_ref().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES ('fleet.public_name', ?, ?)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
        )
        .bind(name)
        .bind(now())
        .execute(&s.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    }

    sqlx::query(
        "UPDATE panel_remote_access SET enabled=?, public_base_url=?, updated_at=? WHERE id=1",
    )
    .bind(b.enabled as i64)
    .bind(&base)
    .bind(now())
    .execute(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    if b.enabled {
        let existing = load_agent_token(&s).await?;
        if existing.is_none() {
            let t = token();
            store_agent_token(&s, &t).await?;
        }
    }

    Ok(Json(access_json(&s).await?))
}

async fn rotate_token(
    State(s): State<AppState>,
    a: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    let t = token();
    store_agent_token(&s, &t).await?;
    sqlx::query("UPDATE panel_remote_access SET enabled=1, updated_at=? WHERE id=1")
        .bind(now())
        .execute(&s.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(access_json(&s).await?))
}

#[derive(serde::Deserialize)]
struct InviteBody {
    #[serde(default)]
    label: String,
    expires_in_hours: Option<i64>,
}

async fn create_invite(
    State(s): State<AppState>,
    a: AuthUser,
    Json(b): Json<InviteBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    let t = token();
    let id = Uuid::new_v4().to_string();
    let expires = b
        .expires_in_hours
        .map(|h| (Utc::now() + Duration::hours(h.clamp(1, 720))).to_rfc3339());
    let base: String =
        sqlx::query_scalar("SELECT public_base_url FROM panel_remote_access WHERE id=1")
            .fetch_one(&s.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if base.trim().is_empty() {
        return Err(AppError(Error::Validation(
            "Enable node mode and set a public URL first".into(),
        )));
    }
    sqlx::query(
        "INSERT INTO fleet_invite_tokens (id, token_hash, token_prefix, label, expires_at, created_by, created_at) VALUES (?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(hash(&t))
    .bind(prefix(&t))
    .bind(if b.label.trim().is_empty() {
        "invite"
    } else {
        b.label.trim()
    })
    .bind(&expires)
    .bind(a.user.id.to_string())
    .bind(now())
    .execute(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(serde_json::json!({
        "id": id,
        "token": t,
        "join_url": format!("{}/join?token={}", base.trim_end_matches('/'), t),
        "expires_at": expires,
        "created_at": now(),
    })))
}

async fn list_invites(
    State(s): State<AppState>,
    _a: AuthUser,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    #[derive(sqlx::FromRow)]
    struct InviteRow {
        id: String,
        token_prefix: String,
        label: String,
        expires_at: Option<String>,
        used_at: Option<String>,
        revoked_at: Option<String>,
        created_at: String,
    }
    let rows = sqlx::query_as::<_, InviteRow>(
        "SELECT id, token_prefix, label, expires_at, used_at, revoked_at, created_at FROM fleet_invite_tokens ORDER BY created_at DESC",
    )
    .fetch_all(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(
        rows.into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "token_prefix": r.token_prefix,
                    "label": r.label,
                    "expires_at": r.expires_at,
                    "used_at": r.used_at,
                    "revoked_at": r.revoked_at,
                    "created_at": r.created_at,
                    "used": r.used_at.is_some(),
                    "revoked": r.revoked_at.is_some(),
                })
            })
            .collect(),
    ))
}

async fn revoke_invite(
    State(s): State<AppState>,
    a: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    sqlx::query("UPDATE fleet_invite_tokens SET revoked_at=? WHERE id=?")
        .bind(now())
        .bind(id)
        .execute(&s.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn bearer(headers: &HeaderMap, s: &AppState) -> Result<String, AppError> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(AppError(Error::Unauthorized))?;
    let h = hash(raw);
    let agent_ok: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM panel_remote_access WHERE id=1 AND enabled=1 AND agent_token_hash=?",
    )
    .bind(&h)
    .fetch_optional(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if agent_ok.is_some() {
        return Ok(raw.to_string());
    }
    let invite_ok: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM fleet_invite_tokens WHERE token_hash=? AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > datetime('now'))",
    )
    .bind(&h)
    .fetch_optional(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if invite_ok.is_none() {
        return Err(AppError(Error::Unauthorized));
    }
    Ok(raw.to_string())
}

async fn accept_invite(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(AppError(Error::Unauthorized))?;
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, expires_at FROM fleet_invite_tokens WHERE token_hash=? AND used_at IS NULL AND revoked_at IS NULL",
    )
    .bind(hash(raw))
    .fetch_optional(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    // Prefer invite redemption; otherwise treat as persistent agent token.
    if let Some((id, exp)) = row {
        if exp
            .as_deref()
            .and_then(|x| chrono::DateTime::parse_from_rfc3339(x).ok())
            .map(|x| x < Utc::now())
            .unwrap_or(false)
        {
            return Err(AppError(Error::Unauthorized));
        }
        sqlx::query("UPDATE fleet_invite_tokens SET used_at=? WHERE id=?")
            .bind(now())
            .bind(id)
            .execute(&s.pool)
            .await
            .ok();
        // Hand out the persistent agent token when available so peers share one credential.
        let agent = load_agent_token(&s).await?.unwrap_or_else(|| raw.to_string());
        let info = agent_info_inner(&s).await?;
        return Ok(Json(serde_json::json!({"agent_token": agent, "info": info})));
    }

    bearer(&headers, &s).await?;
    let agent = load_agent_token(&s).await?.unwrap_or_else(|| raw.to_string());
    let info = agent_info_inner(&s).await?;
    Ok(Json(serde_json::json!({"agent_token": agent, "info": info})))
}

async fn agent_info_inner(s: &AppState) -> Result<serde_json::Value, AppError> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clusters")
        .fetch_one(&s.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    let max: Option<i64> =
        sqlx::query_scalar("SELECT max_clusters FROM nodes WHERE is_default=1 LIMIT 1")
            .fetch_optional(&s.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    let name: String =
        sqlx::query_scalar("SELECT value FROM settings WHERE key='fleet.public_name'")
            .fetch_optional(&s.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?
            .unwrap_or_else(|| "PgPanel node".into());
    Ok(serde_json::json!({
        "name": name,
        "version": env!("CARGO_PKG_VERSION"),
        "cluster_count": count,
        "max_clusters": max,
        "status": "online",
    }))
}

async fn agent_info(State(s): State<AppState>, h: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    bearer(&h, &s).await?;
    Ok(Json(agent_info_inner(&s).await?))
}

#[derive(serde::Deserialize)]
struct JoinBody {
    /// Full join link from the other panel (preferred).
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    token: String,
    local_name: Option<String>,
}

async fn join(
    State(s): State<AppState>,
    a: AuthUser,
    Json(b): Json<JoinBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    let (base, tok) = parse_join_parts(b.link.as_deref(), &b.base_url, &b.token)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;

    let (peer_token, info) = {
        let accept = client
            .post(format!("{base}/api/fleet/agent/accept-invite"))
            .bearer_auth(&tok)
            .send()
            .await;
        match accept {
            Ok(r) if r.status().is_success() => {
                let v: serde_json::Value = r
                    .json()
                    .await
                    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
                let peer_tok = v
                    .get("agent_token")
                    .and_then(|x| x.as_str())
                    .unwrap_or(&tok)
                    .to_string();
                (peer_tok, v.get("info").cloned().unwrap_or(serde_json::json!({})))
            }
            _ => {
                let info_resp = client
                    .get(format!("{base}/api/fleet/agent/info"))
                    .bearer_auth(&tok)
                    .send()
                    .await
                    .map_err(|e| {
                        AppError(Error::Internal(format!(
                            "Could not reach remote panel: {e}"
                        )))
                    })?;
                if !info_resp.status().is_success() {
                    return Err(AppError(Error::Validation(
                        "Remote panel rejected the join token — check node mode is enabled and the link is current".into(),
                    )));
                }
                let info: serde_json::Value = info_resp
                    .json()
                    .await
                    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
                (tok, info)
            }
        }
    };

    // Avoid duplicate peers for the same base URL.
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM fleet_peers WHERE base_url=? LIMIT 1")
            .bind(&base)
            .fetch_optional(&s.pool)
            .await
            .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    if let Some(id) = existing {
        let enc = encrypt_secret(
            &s.config.master_encryption_key,
            &SecretString::from(peer_token),
        )
        .map_err(AppError)?;
        sqlx::query(
            "UPDATE fleet_peers SET token_encrypted=?, status='online', cluster_count=?, advertised_capacity=?, last_seen_at=?, last_error=NULL, updated_at=? WHERE id=?",
        )
        .bind(enc)
        .bind(info.get("cluster_count").and_then(|x| x.as_i64()).unwrap_or(0))
        .bind(info.get("max_clusters").and_then(|x| x.as_i64()))
        .bind(now())
        .bind(now())
        .bind(&id)
        .execute(&s.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
        return Ok(Json(serde_json::json!({"id": id, "info": info, "updated": true})));
    }

    let enc = encrypt_secret(
        &s.config.master_encryption_key,
        &SecretString::from(peer_token),
    )
    .map_err(AppError)?;
    let id = Uuid::new_v4().to_string();
    let n = b
        .local_name
        .filter(|x| !x.trim().is_empty())
        .unwrap_or_else(|| {
            info.get("name")
                .and_then(|x| x.as_str())
                .unwrap_or(&base)
                .to_string()
        });
    let cluster_count = info
        .get("cluster_count")
        .and_then(|x| x.as_i64())
        .unwrap_or(0);
    let capacity = info.get("max_clusters").and_then(|x| x.as_i64());
    sqlx::query(
        "INSERT INTO fleet_peers (id,name,base_url,token_encrypted,status,advertised_capacity,cluster_count,last_seen_at,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&n)
    .bind(&base)
    .bind(enc)
    .bind("online")
    .bind(capacity)
    .bind(cluster_count)
    .bind(now())
    .bind(now())
    .bind(now())
    .execute(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(serde_json::json!({"id": id, "info": info, "updated": false})))
}

async fn list_peers(State(s): State<AppState>, _a: AuthUser) -> ApiResult<Json<serde_json::Value>> {
    let rows = sqlx::query_as::<_, (String, String, String, String, i64, Option<i64>, Option<String>)>(
        "SELECT id,name,base_url,status,cluster_count,advertised_capacity,last_seen_at FROM fleet_peers ORDER BY name",
    )
    .fetch_all(&s.pool)
    .await
    .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(serde_json::json!(rows
        .into_iter()
        .map(|(id, n, b, st, c, cap, last)| {
            serde_json::json!({
                "id": id,
                "name": n,
                "base_url": b,
                "status": st,
                "cluster_count": c,
                "advertised_capacity": cap,
                "max_clusters": cap,
                "last_seen_at": last,
            })
        })
        .collect::<Vec<_>>())))
}

async fn remove_peer(
    State(s): State<AppState>,
    a: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    sqlx::query("DELETE FROM fleet_peers WHERE id=?")
        .bind(id)
        .execute(&s.pool)
        .await
        .map_err(|e| AppError(Error::Internal(e.to_string())))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn ping_peer(
    State(s): State<AppState>,
    a: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    let row: (String, String) =
        sqlx::query_as("SELECT base_url,token_encrypted FROM fleet_peers WHERE id=?")
            .bind(&id)
            .fetch_one(&s.pool)
            .await
            .map_err(|_e| AppError(Error::NotFound("peer".into())))?;
    let t = decrypt_secret(&s.config.master_encryption_key, &row.1)
        .map_err(AppError)?
        .expose_secret()
        .to_string();
    let started = std::time::Instant::now();
    let r = reqwest::Client::new()
        .get(format!(
            "{}/api/fleet/agent/info",
            row.0.trim_end_matches('/')
        ))
        .bearer_auth(t)
        .send()
        .await;
    let latency_ms = started.elapsed().as_millis() as u64;
    match r {
        Ok(resp) if resp.status().is_success() => {
            let v = resp.json::<serde_json::Value>().await.unwrap_or_default();
            sqlx::query(
                "UPDATE fleet_peers SET status='online',cluster_count=?,advertised_capacity=?,last_seen_at=?,last_error=NULL,updated_at=? WHERE id=?",
            )
            .bind(v["cluster_count"].as_i64().unwrap_or(0))
            .bind(v["max_clusters"].as_i64())
            .bind(now())
            .bind(now())
            .bind(id)
            .execute(&s.pool)
            .await
            .ok();
            Ok(Json(serde_json::json!({
                "status": "online",
                "latency_ms": latency_ms,
                "info": v
            })))
        }
        Ok(resp) => {
            let err = resp.status().to_string();
            sqlx::query(
                "UPDATE fleet_peers SET status='offline', last_error=?, updated_at=? WHERE id=?",
            )
            .bind(&err)
            .bind(now())
            .bind(id)
            .execute(&s.pool)
            .await
            .ok();
            Ok(Json(
                serde_json::json!({"status":"offline","latency_ms": latency_ms, "error": err}),
            ))
        }
        Err(e) => {
            let err = e.to_string();
            sqlx::query(
                "UPDATE fleet_peers SET status='offline', last_error=?, updated_at=? WHERE id=?",
            )
            .bind(&err)
            .bind(now())
            .bind(id)
            .execute(&s.pool)
            .await
            .ok();
            Ok(Json(
                serde_json::json!({"status":"offline","error": err}),
            ))
        }
    }
}
