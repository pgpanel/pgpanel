//! Static file serving via rust-embed.

use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::Embed;
use sha2::{Digest, Sha256};

#[derive(Embed)]
#[folder = "../../static/"]
struct Assets;

const CACHE_CONTROL_IMMUTABLE: &str = "public, max-age=31536000, immutable";

pub fn router() -> Router<crate::state::AppState> {
    Router::new().route("/static/*path", get(serve_static))
}

fn is_versionable_asset(path: &str) -> bool {
    path.ends_with(".css") || path.ends_with(".js")
}

fn etag_for(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    format!("\"{}\"", hex::encode(hash))
}

async fn serve_static(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    match Assets::get(path.as_str()) {
        Some(content) => {
            let data = content.data.into_owned();
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref());

            if is_versionable_asset(&path) {
                builder = builder
                    .header(header::CACHE_CONTROL, CACHE_CONTROL_IMMUTABLE)
                    .header(
                        header::ETAG,
                        HeaderValue::from_str(&etag_for(&data)).unwrap(),
                    );
            }

            builder.body(Body::from(data)).unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap(),
    }
}
