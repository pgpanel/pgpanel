mod audit;
mod auth_routes;
mod backup;
mod browser;
mod clusters;
mod databases;
mod health;
mod operations;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let cors = if state.config.cors_origins.is_empty() {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins: Vec<_> = state
            .config
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_credentials(true)
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
        .layer(cors)
        .with_state(state)
}
