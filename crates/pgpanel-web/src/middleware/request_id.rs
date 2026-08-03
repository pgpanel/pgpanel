//! Request ID and client IP extraction.

use crate::state::AppState;
use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;
use tracing::info_span;
use uuid::Uuid;

/// Client IP stored in request extensions.
#[derive(Clone, Debug)]
pub struct ClientIp(pub String);

/// Request ID stored in request extensions.
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

/// Attach request ID and client IP, create tracing span.
pub async fn request_id_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let request_id = Uuid::new_v4().to_string();

    let ip = extract_client_ip(&request, &state);
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));
    request.extensions_mut().insert(ClientIp(ip.clone()));

    let span = info_span!(
        "request",
        request_id = %request_id,
        client_ip = %ip,
        method = %request.method(),
        path = %request.uri().path(),
    );

    let _guard = span.enter();
    next.run(request).await
}

fn extract_client_ip(request: &Request, state: &AppState) -> String {
    let peer_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());

    let proxy_is_trusted = state.config.trusted_proxies.enabled
        && peer_ip.is_some_and(|ip| {
            state
                .config
                .trusted_proxies
                .proxies
                .iter()
                .any(|trusted| trusted.parse::<std::net::IpAddr>().ok() == Some(ip))
        });

    if proxy_is_trusted {
        if state.config.trusted_proxies.prefer_cf_connecting_ip {
            if let Some(cf) = request
                .headers()
                .get("CF-Connecting-IP")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<std::net::IpAddr>().ok())
            {
                return cf.to_string();
            }
        }
        if let Some(xff) = request
            .headers()
            .get("X-Forwarded-For")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse::<std::net::IpAddr>().ok())
        {
            return xff.to_string();
        }
    }
    peer_ip
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".into())
}
