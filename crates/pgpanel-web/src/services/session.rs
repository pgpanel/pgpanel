//! Session management service.

use chrono::{DateTime, Duration, Utc};
use pgpanel_core::auth::{generate_csrf_token, hash_token, SessionToken};
use pgpanel_core::config::Config;
use pgpanel_core::permissions::Role;
use pgpanel_core::CoreError;
use pgpanel_core::CoreResult;
use sqlx::SqlitePool;

fn db_err(e: sqlx::Error) -> CoreError {
    CoreError::Internal(format!("database error: {e}"))
}

/// Active session data.
#[derive(Debug, Clone)]
pub struct SessionData {
    pub session_id: i64,
    pub user_id: i64,
    pub username: String,
    pub role: Role,
    pub csrf_token: String,
    pub last_auth_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub ip_address: Option<String>,
}

impl SessionData {
    pub fn is_recently_authed(&self, window_secs: u64) -> bool {
        let cutoff = Utc::now() - Duration::seconds(window_secs as i64);
        self.last_auth_at > cutoff
    }
}

/// Session CRUD and validation.
#[derive(Clone)]
pub struct SessionService {
    db: SqlitePool,
    cookie_name: String,
    ttl_secs: u64,
    absolute_ttl_secs: u64,
}

impl SessionService {
    pub fn new(db: SqlitePool, config: &Config) -> Self {
        Self {
            db,
            cookie_name: config.session.cookie_name.clone(),
            ttl_secs: config.session.ttl_secs,
            absolute_ttl_secs: config.session.absolute_ttl_secs,
        }
    }

    pub fn cookie_name(&self) -> &str {
        &self.cookie_name
    }

    /// Create a new session after successful login.
    pub async fn create_session(
        &self,
        user_id: i64,
        ip: Option<&str>,
        user_agent: Option<&str>,
    ) -> CoreResult<(SessionToken, SessionData)> {
        let user = self.get_user(user_id).await?;
        let token = SessionToken::generate();
        let csrf = generate_csrf_token();
        let now = Utc::now();
        let expires = now + Duration::seconds(self.ttl_secs as i64);
        let absolute = now + Duration::seconds(self.absolute_ttl_secs as i64);

        let result = sqlx::query(
            "INSERT INTO sessions (user_id, token_hash, csrf_token, created_at, expires_at, absolute_expires_at, last_seen_at, last_auth_at, ip_address, user_agent)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(token.hash())
        .bind(&csrf)
        .bind(now.to_rfc3339())
        .bind(expires.to_rfc3339())
        .bind(absolute.to_rfc3339())
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .bind(ip)
        .bind(user_agent)
        .execute(&self.db)
        .await
        .map_err(db_err)?;

        let session_id = result.last_insert_rowid();
        Ok((
            token,
            SessionData {
                session_id,
                user_id,
                username: user.0,
                role: user.1,
                csrf_token: csrf,
                last_auth_at: now,
                expires_at: expires,
                ip_address: ip.map(str::to_string),
            },
        ))
    }

    /// Validate session token from cookie.
    pub async fn validate_session(&self, token: &str, ip: Option<&str>) -> CoreResult<SessionData> {
        let hash = hash_token(token);
        let row: Option<SessionRow> = sqlx::query_as(
            "SELECT s.id, s.user_id, u.username, u.role, s.csrf_token, s.last_auth_at, s.expires_at, s.absolute_expires_at, s.ip_address
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ? AND u.disabled = 0",
        )
        .bind(&hash)
        .fetch_optional(&self.db)
        .await
        .map_err(db_err)?;

        let row = row.ok_or_else(|| CoreError::Auth("invalid session".into()))?;
        let now = Utc::now();
        let expires = parse_dt(&row.expires_at)?;
        let absolute = parse_dt(&row.absolute_expires_at)?;

        if now > expires || now > absolute {
            sqlx::query("DELETE FROM sessions WHERE id = ?")
                .bind(row.id)
                .execute(&self.db)
                .await
                .map_err(db_err)?;
            return Err(CoreError::Auth("session expired".into()));
        }

        if let Some(ref bound_ip) = row.ip_address {
            match ip {
                Some(request_ip) if request_ip == bound_ip => {}
                _ => return Err(CoreError::Auth("session IP mismatch".into())),
            }
        }

        // Sliding expiration
        let new_expires = now + Duration::seconds(self.ttl_secs as i64);
        sqlx::query("UPDATE sessions SET last_seen_at = ?, expires_at = ? WHERE id = ?")
            .bind(now.to_rfc3339())
            .bind(new_expires.to_rfc3339())
            .bind(row.id)
            .execute(&self.db)
            .await
            .map_err(db_err)?;

        let role =
            Role::parse(&row.role).ok_or_else(|| CoreError::Internal("invalid role".into()))?;

        Ok(SessionData {
            session_id: row.id,
            user_id: row.user_id,
            username: row.username,
            role,
            csrf_token: row.csrf_token,
            last_auth_at: parse_dt(&row.last_auth_at)?,
            expires_at: new_expires,
            ip_address: row.ip_address,
        })
    }

    /// Destroy session (logout).
    pub async fn destroy_session(&self, token: &str) -> CoreResult<()> {
        let hash = hash_token(token);
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(hash)
            .execute(&self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Touch last_auth_at for reauthentication.
    pub async fn touch_auth(&self, session_id: i64) -> CoreResult<()> {
        let now = Utc::now();
        sqlx::query("UPDATE sessions SET last_auth_at = ? WHERE id = ?")
            .bind(now.to_rfc3339())
            .bind(session_id)
            .execute(&self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn get_user(&self, user_id: i64) -> CoreResult<(String, Role)> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT username, role FROM users WHERE id = ? AND disabled = 0")
                .bind(user_id)
                .fetch_optional(&self.db)
                .await
                .map_err(db_err)?;
        let (username, role_str) =
            row.ok_or_else(|| CoreError::NotFound("user not found".into()))?;
        let role =
            Role::parse(&role_str).ok_or_else(|| CoreError::Internal("invalid role".into()))?;
        Ok((username, role))
    }
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: i64,
    user_id: i64,
    username: String,
    role: String,
    csrf_token: String,
    last_auth_at: String,
    expires_at: String,
    absolute_expires_at: String,
    ip_address: Option<String>,
}

fn parse_dt(s: &str) -> CoreResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| CoreError::Internal(format!("datetime parse: {e}")))
}
