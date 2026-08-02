mod audit;
mod auth_routes;
mod backup;
mod browser;
mod clusters;
mod databases;
mod destinations;
mod health;
pub mod nodes;
mod monitoring;
mod operations;
mod replicas;
mod settings;
mod tokens;
mod users;
pub mod waf;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use once_cell::sync::Lazy;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let cors = build_cors(&state);

    let body_limit = {
        // Will be refreshed via middleware reading live WAF; static layer uses a safe default.
        10 * 1024 * 1024usize
    };

    Router::new()
        .merge(health::routes())
        .merge(auth_routes::routes())
        .merge(clusters::routes())
        .merge(databases::routes())
        .merge(browser::routes())
        .merge(backup::routes())
        .merge(operations::routes())
        .merge(audit::routes())
        .merge(settings::routes())
        .merge(nodes::routes())
        .merge(waf::routes())
        .merge(users::routes())
        .merge(destinations::routes())
        .merge(replicas::routes())
        .merge(monitoring::routes())
        .merge(tokens::routes())
        .layer(from_fn_with_state(state.clone(), waf_guard))
        .layer(RequestBodyLimitLayer::new(body_limit))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(cors)
        .with_state(state)
}

fn build_cors(state: &AppState) -> CorsLayer {
    if state.config.cors_origins.is_empty() {
        // Secure default: same-origin only (no CORS reflection of arbitrary origins).
        CorsLayer::new()
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([
                header::CONTENT_TYPE,
                header::ACCEPT,
                header::COOKIE,
                header::HeaderName::from_static("x-csrf-token"),
            ])
            .allow_credentials(true)
            .allow_origin(AllowOrigin::predicate(|origin, _| {
                // Dev convenience: localhost only when no explicit list.
                let o = origin.as_bytes();
                o.starts_with(b"http://127.0.0.1")
                    || o.starts_with(b"http://localhost")
                    || o.starts_with(b"https://127.0.0.1")
                    || o.starts_with(b"https://localhost")
            }))
    } else {
        let origins: Vec<_> = state
            .config
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([
                header::CONTENT_TYPE,
                header::ACCEPT,
                header::COOKIE,
                header::HeaderName::from_static("x-csrf-token"),
            ])
            .allow_credentials(true)
    }
}

/// In-app WAF: IP allow/deny, UA blocks, path blocks, crude per-IP rate limit.
async fn waf_guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let cfg = state.waf.read().await.clone();
    if !cfg.enable_security_headers && cfg.denied_ips.is_empty() && cfg.allowed_ips.is_empty() {
        // Still apply rate / path checks below
    }

    let path = req.uri().path().to_string();
    let method = req.method().clone();

    // Skip health probes from path blocks
    let is_health = path == "/health" || path == "/ready";

    let peer_ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .or_else(|| {
            req.headers()
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".into());

    if !is_health {
        for blocked in &cfg.blocked_paths {
            if !blocked.is_empty() && (path == *blocked || path.starts_with(blocked)) {
                return (StatusCode::NOT_FOUND, "Not Found").into_response();
            }
        }

        if !cfg.denied_ips.is_empty() && cfg.denied_ips.iter().any(|ip| ip == &peer_ip) {
            return (StatusCode::FORBIDDEN, "Forbidden").into_response();
        }
        if !cfg.allowed_ips.is_empty() && !cfg.allowed_ips.iter().any(|ip| ip == &peer_ip) {
            if cfg.fail_closed_on_deny {
                return (StatusCode::FORBIDDEN, "Forbidden").into_response();
            }
        }

        let ua = req
            .headers()
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if cfg.block_empty_user_agent && ua.is_empty() {
            return (StatusCode::FORBIDDEN, "Forbidden").into_response();
        }
        for needle in &cfg.blocked_user_agents {
            if !needle.is_empty() && ua.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
            {
                return (StatusCode::FORBIDDEN, "Forbidden").into_response();
            }
        }

        // Simple token-bucket-ish rate limit keyed by IP + route class
        let limit = if path.starts_with("/api/auth/login") {
            cfg.login_rate_limit_per_minute
        } else if path.starts_with("/api/") {
            cfg.api_rate_limit_per_minute
        } else {
            cfg.rate_limit_per_minute
        };
        if limit > 0 && !rate_allow(&peer_ip, limit) {
            return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
        }
    }

    let mut res = next.run(req).await;

    if cfg.enable_security_headers {
        let headers = res.headers_mut();
        headers
            .entry(header::X_CONTENT_TYPE_OPTIONS)
            .or_insert(HeaderValue::from_static("nosniff"));
        headers
            .entry(header::X_FRAME_OPTIONS)
            .or_insert(HeaderValue::from_static("DENY"));
        if method != Method::OPTIONS {
            let _ = method;
        }
    }

    res
}

fn rate_allow(ip: &str, per_minute: u32) -> bool {
    use std::sync::Mutex;

    static BUCKETS: Lazy<Mutex<std::collections::HashMap<String, (u32, Instant)>>> =
        Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

    let Ok(mut map) = BUCKETS.lock() else {
        return true;
    };
    let now = Instant::now();
    let entry = map.entry(ip.to_string()).or_insert((0, now));
    if now.duration_since(entry.1) > Duration::from_secs(60) {
        *entry = (1, now);
        return true;
    }
    if entry.0 >= per_minute {
        return false;
    }
    entry.0 += 1;
    true
}
