//! Rate limiting middleware.

use crate::error::AppError;
use crate::state::AppState;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use chrono::Utc;

/// Simple sliding-window rate limit per IP.
pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let ip = request
        .extensions()
        .get::<super::request_id::ClientIp>()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| "unknown".into());

    let path = request.uri().path();
    let is_login = path == "/auth/login" && request.method() == axum::http::Method::POST;

    let (limit, window_secs) = if is_login {
        (
            state.config.rate_limit.login_per_ip,
            state.config.rate_limit.login_window_secs,
        )
    } else {
        (state.config.rate_limit.api_per_ip_per_minute, 60)
    };

    let key = if is_login {
        format!("login:{ip}")
    } else {
        format!("api:{ip}")
    };

    let now = Utc::now();
    let window_start = now - chrono::Duration::seconds(window_secs as i64);

    let row: Option<(i64, String)> =
        sqlx::query_as("SELECT count, window_start FROM rate_limit_buckets WHERE key = ?")
            .bind(&key)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::Db)?;

    let (count, should_block) = if let Some((count, ws_str)) = row {
        let ws = chrono::DateTime::parse_from_rfc3339(&ws_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or(now);
        if ws < window_start {
            (1, false)
        } else if count >= limit as i64 {
            (count, true)
        } else {
            (count + 1, false)
        }
    } else {
        (1, false)
    };

    if should_block {
        return Err(AppError::RateLimited("too many requests".into()));
    }

    sqlx::query(
        "INSERT INTO rate_limit_buckets (key, count, window_start) VALUES (?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET count = excluded.count, window_start = excluded.window_start",
    )
    .bind(&key)
    .bind(count)
    .bind(now.to_rfc3339())
    .execute(&state.db)
    .await
    .map_err(AppError::Db)?;

    Ok(next.run(request).await)
}
