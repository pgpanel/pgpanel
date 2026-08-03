//! Databasus integration — public health probe only.
//!
//! Documented public endpoint: `GET /api/v1/system/health` (unauthenticated).
//! Backup listing/trigger APIs require `database_id` and vary by version; PgPanel
//! has no database mapping and does not call those endpoints.

use futures::StreamExt;
use pgpanel_core::config::DatabasusConfig;
use pgpanel_core::CoreError;
use pgpanel_core::CoreResult;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Relative path joined onto the configured base URL.
pub const HEALTH_PATH: &str = "api/v1/system/health";

/// Cap health response bodies so a misbehaving peer cannot exhaust memory.
pub const MAX_HEALTH_RESPONSE_BYTES: usize = 16_384;

/// Truncate any operator-facing message extracted from a response.
const MAX_SAFE_MESSAGE_CHARS: usize = 200;

/// Databasus HTTP client (health probe).
#[derive(Clone)]
pub struct DatabasusClient {
    http: reqwest::Client,
    health_url: Url,
}

/// Result of probing Databasus health.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unavailable,
}

impl HealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Health probe outcome for the Backups page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub status: HealthStatus,
    /// Short, safe operator message (never includes secrets).
    pub message: Option<String>,
    /// Parsed JSON body when present and within size limits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

impl HealthCheckResult {
    fn healthy(body: Option<Value>, message: Option<String>) -> Self {
        Self {
            status: HealthStatus::Healthy,
            message,
            body,
        }
    }

    fn degraded(body: Option<Value>, message: Option<String>) -> Self {
        Self {
            status: HealthStatus::Degraded,
            message,
            body,
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Unavailable,
            message: Some(safe_message(&message.into())),
            body: None,
        }
    }
}

impl DatabasusClient {
    /// Build a client when integration is enabled and `base_url` is set.
    ///
    /// Does not require an API key — the public health endpoint is unauthenticated.
    pub fn new(config: &DatabasusConfig) -> CoreResult<Self> {
        if !config.enabled {
            return Err(CoreError::Config("databasus not enabled".into()));
        }
        let base = config
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| CoreError::Config("databasus base_url not set".into()))?;

        let health_url = build_health_url(base)?;

        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs.max(1)))
            .danger_accept_invalid_certs(!config.tls_verify)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| CoreError::Internal(format!("http client: {e}")))?;

        Ok(Self { http, health_url })
    }

    /// Probe `GET /api/v1/system/health`. Never sends the API key.
    ///
    /// Network and parse failures map to [`HealthStatus::Unavailable`] so the
    /// panel stays usable when Databasus is down.
    pub async fn check_health(&self) -> HealthCheckResult {
        let resp = match self.http.get(self.health_url.clone()).send().await {
            Ok(r) => r,
            Err(e) => {
                return HealthCheckResult::unavailable(format!("request failed: {e}"));
            }
        };

        let status = resp.status();
        let body_bytes = match read_body_limited(resp, MAX_HEALTH_RESPONSE_BYTES).await {
            Ok(b) => b,
            Err(msg) => return HealthCheckResult::unavailable(msg),
        };

        let (json_body, message) = parse_health_payload(&body_bytes);

        match status.as_u16() {
            200 => HealthCheckResult::healthy(json_body, message),
            503 => HealthCheckResult::degraded(
                json_body,
                message.or_else(|| Some("Databasus reported degraded".into())),
            ),
            code => HealthCheckResult::unavailable(format!(
                "unexpected HTTP {code} from health endpoint"
            )),
        }
    }

    /// Configured health URL (for tests / diagnostics).
    pub fn health_url(&self) -> &Url {
        &self.health_url
    }
}

/// Join `base_url` with [`HEALTH_PATH`], enforcing `http`/`https` only.
pub fn build_health_url(base_url: &str) -> CoreResult<Url> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err(CoreError::Config("databasus base_url is empty".into()));
    }

    let mut base = Url::parse(trimmed)
        .map_err(|e| CoreError::Config(format!("invalid databasus base_url: {e}")))?;

    match base.scheme() {
        "http" | "https" => {}
        other => {
            return Err(CoreError::Config(format!(
                "databasus base_url must use http or https, got '{other}'"
            )));
        }
    }

    if base.host_str().is_none() {
        return Err(CoreError::Config(
            "databasus base_url must include a host".into(),
        ));
    }

    // Ensure a directory-style base so relative join keeps any path prefix.
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }

    base.join(HEALTH_PATH)
        .map_err(|e| CoreError::Config(format!("failed to join health path: {e}")))
}

/// Map HTTP status codes used by the documented health endpoint.
pub fn map_health_http_status(status: u16) -> HealthStatus {
    match status {
        200 => HealthStatus::Healthy,
        503 => HealthStatus::Degraded,
        _ => HealthStatus::Unavailable,
    }
}

async fn read_body_limited(resp: reqwest::Response, max_bytes: usize) -> Result<Vec<u8>, String> {
    if let Some(len) = resp.content_length() {
        if len > max_bytes as u64 {
            return Err(format!(
                "health response too large (Content-Length {len} > {max_bytes})"
            ));
        }
    }

    let mut buf = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("failed reading health body: {e}"))?;
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!("health response exceeded {max_bytes} bytes"));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

fn parse_health_payload(bytes: &[u8]) -> (Option<Value>, Option<String>) {
    if bytes.is_empty() {
        return (None, None);
    }
    match serde_json::from_slice::<Value>(bytes) {
        Ok(value) => {
            let message = extract_safe_message(&value);
            (Some(value), message)
        }
        Err(_) => {
            let text = String::from_utf8_lossy(bytes);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                (None, None)
            } else {
                (None, Some(safe_message(trimmed)))
            }
        }
    }
}

fn extract_safe_message(value: &Value) -> Option<String> {
    for key in ["message", "status", "detail", "error"] {
        if let Some(s) = value.get(key).and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(safe_message(t));
            }
        }
    }
    None
}

fn safe_message(raw: &str) -> String {
    let collapsed: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.chars().count() <= MAX_SAFE_MESSAGE_CHARS {
        collapsed
    } else {
        let truncated: String = collapsed.chars().take(MAX_SAFE_MESSAGE_CHARS).collect();
        format!("{truncated}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::routing::get;
    use axum::{Json, Router};
    use serde_json::json;
    use tokio::net::TcpListener;

    #[test]
    fn build_health_url_joins_origin() {
        let url = build_health_url("https://databasus.example").unwrap();
        assert_eq!(
            url.as_str(),
            "https://databasus.example/api/v1/system/health"
        );
    }

    #[test]
    fn build_health_url_preserves_path_prefix() {
        let url = build_health_url("https://databasus.example/prefix").unwrap();
        assert_eq!(
            url.as_str(),
            "https://databasus.example/prefix/api/v1/system/health"
        );
    }

    #[test]
    fn build_health_url_trims_and_handles_trailing_slash() {
        let url = build_health_url("  http://127.0.0.1:8080/  ").unwrap();
        assert_eq!(url.as_str(), "http://127.0.0.1:8080/api/v1/system/health");
    }

    #[test]
    fn build_health_url_rejects_non_http_schemes() {
        let err = build_health_url("ftp://example.com").unwrap_err();
        assert!(err.to_string().contains("http or https"));
    }

    #[test]
    fn build_health_url_rejects_missing_host() {
        assert!(build_health_url("https://").is_err());
        assert!(build_health_url("http:///").is_err());
    }

    #[test]
    fn map_health_http_status_documented_codes() {
        assert_eq!(map_health_http_status(200), HealthStatus::Healthy);
        assert_eq!(map_health_http_status(503), HealthStatus::Degraded);
        assert_eq!(map_health_http_status(404), HealthStatus::Unavailable);
        assert_eq!(map_health_http_status(500), HealthStatus::Unavailable);
    }

    #[test]
    fn safe_message_strips_controls_and_truncates() {
        let msg = safe_message("ok\nstatus\tready");
        assert_eq!(msg, "ok status ready");
        let long = "x".repeat(400);
        let truncated = safe_message(&long);
        assert!(truncated.ends_with('…'));
        assert!(truncated.chars().count() <= MAX_SAFE_MESSAGE_CHARS + 1);
    }

    async fn spawn_mock(app: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn test_config(base_url: &str) -> DatabasusConfig {
        DatabasusConfig {
            enabled: true,
            base_url: Some(base_url.to_string()),
            api_key: Some("must-not-be-sent".into()),
            timeout_secs: 5,
            tls_verify: true,
        }
    }

    #[tokio::test]
    async fn check_health_maps_200_json() {
        let app = Router::new().route(
            "/api/v1/system/health",
            get(|| async { (StatusCode::OK, Json(json!({ "status": "ok" }))) }),
        );
        let base = spawn_mock(app).await;
        let client = DatabasusClient::new(&test_config(&base)).unwrap();
        let result = client.check_health().await;
        assert_eq!(result.status, HealthStatus::Healthy);
        assert_eq!(result.message.as_deref(), Some("ok"));
        assert!(result.body.is_some());
    }

    #[tokio::test]
    async fn check_health_maps_503_to_degraded() {
        let app = Router::new().route(
            "/api/v1/system/health",
            get(|| async {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "message": "storage down" })),
                )
            }),
        );
        let base = spawn_mock(app).await;
        let client = DatabasusClient::new(&test_config(&base)).unwrap();
        let result = client.check_health().await;
        assert_eq!(result.status, HealthStatus::Degraded);
        assert_eq!(result.message.as_deref(), Some("storage down"));
    }

    #[tokio::test]
    async fn check_health_unavailable_on_connection_refused() {
        let client = DatabasusClient::new(&test_config("http://127.0.0.1:1")).unwrap();
        let result = client.check_health().await;
        assert_eq!(result.status, HealthStatus::Unavailable);
        assert!(result.message.is_some());
    }

    #[tokio::test]
    async fn check_health_rejects_oversized_body() {
        let app = Router::new().route(
            "/api/v1/system/health",
            get(|| async {
                let body = "a".repeat(MAX_HEALTH_RESPONSE_BYTES + 64);
                (StatusCode::OK, body)
            }),
        );
        let base = spawn_mock(app).await;
        let client = DatabasusClient::new(&test_config(&base)).unwrap();
        let result = client.check_health().await;
        assert_eq!(result.status, HealthStatus::Unavailable);
        assert!(
            result.message.as_deref().unwrap_or("").contains("exceeded")
                || result
                    .message
                    .as_deref()
                    .unwrap_or("")
                    .contains("too large")
        );
    }

    #[tokio::test]
    async fn check_health_does_not_require_api_key() {
        let app = Router::new().route("/api/v1/system/health", get(|| async { StatusCode::OK }));
        let base = spawn_mock(app).await;
        let mut cfg = test_config(&base);
        cfg.api_key = None;
        let client = DatabasusClient::new(&cfg).unwrap();
        let result = client.check_health().await;
        assert_eq!(result.status, HealthStatus::Healthy);
    }
}
