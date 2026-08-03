//! Health checks for deployment slots.

use crate::error::{UpdaterError, UpdaterResult};
use std::time::Duration;

/// Health endpoint paths.
pub const HEALTH_LIVE: &str = "/health/live";
pub const HEALTH_READY: &str = "/health/ready";
pub const HEALTH_VERSION: &str = "/health/version";

/// Configuration for health probing.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    /// Base URL of the slot (e.g. http://127.0.0.1:8081).
    pub base_url: String,
    /// Per-request timeout.
    pub timeout: Duration,
    /// Number of attempts per endpoint.
    pub retries: u32,
    /// Delay between retries.
    pub retry_delay: Duration,
    /// Expected release version, when validating a deployment.
    pub expected_version: Option<String>,
    /// Expected blue/green slot, when validating a deployment.
    pub expected_slot: Option<String>,
}

impl HealthConfig {
    /// Build from host:port upstream string.
    pub fn from_upstream(upstream: &str, timeout: Duration) -> Self {
        let base_url = if upstream.starts_with("http://") || upstream.starts_with("https://") {
            upstream.to_string()
        } else {
            format!("http://{upstream}")
        };
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            timeout,
            retries: 10,
            retry_delay: Duration::from_secs(2),
            expected_version: None,
            expected_slot: None,
        }
    }
}

/// Aggregated health status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthStatus {
    /// Live probe succeeded.
    pub live: bool,
    /// Ready probe succeeded.
    pub ready: bool,
}

impl HealthStatus {
    /// Both probes passed.
    pub fn is_healthy(&self) -> bool {
        self.live && self.ready
    }
}

/// HTTP health checker.
#[derive(Debug, Clone)]
pub struct HealthChecker {
    client: reqwest::Client,
}

/// Version and slot returned by `/health/version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthVersion {
    /// Application release version.
    pub version: String,
    /// Application deployment slot.
    pub slot: String,
}

/// Parse the JSON returned by `/health/version`.
pub fn parse_version_response(body: &[u8]) -> UpdaterResult<HealthVersion> {
    #[derive(serde::Deserialize)]
    struct Response {
        version: String,
        slot: String,
    }
    let response: Response = serde_json::from_slice(body).map_err(|error| {
        UpdaterError::Health(format!("invalid /health/version response: {error}"))
    })?;
    Ok(HealthVersion {
        version: response.version,
        slot: response.slot,
    })
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new().expect("health client")
    }
}

impl HealthChecker {
    /// Create a new health checker.
    pub fn new() -> UpdaterResult<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| UpdaterError::Health(e.to_string()))?;
        Ok(Self { client })
    }

    async fn probe(&self, url: &str, timeout: Duration) -> bool {
        match tokio::time::timeout(timeout, self.client.get(url).send()).await {
            Ok(Ok(resp)) => resp.status().is_success(),
            _ => false,
        }
    }

    async fn probe_with_retries(&self, url: &str, config: &HealthConfig) -> bool {
        for attempt in 0..config.retries {
            if self.probe(url, config.timeout).await {
                return true;
            }
            if attempt + 1 < config.retries {
                tokio::time::sleep(config.retry_delay).await;
            }
        }
        false
    }

    async fn fetch_version(&self, config: &HealthConfig) -> UpdaterResult<HealthVersion> {
        let url = format!("{}{}", config.base_url, HEALTH_VERSION);
        let response = tokio::time::timeout(config.timeout, self.client.get(&url).send())
            .await
            .map_err(|_| UpdaterError::Health(format!("timed out requesting {url}")))?
            .map_err(|error| UpdaterError::Health(format!("request to {url} failed: {error}")))?;
        if !response.status().is_success() {
            return Err(UpdaterError::Health(format!(
                "{url} returned HTTP {}",
                response.status()
            )));
        }
        let body = tokio::time::timeout(config.timeout, response.bytes())
            .await
            .map_err(|_| UpdaterError::Health(format!("timed out reading {url}")))?
            .map_err(|error| UpdaterError::Health(format!("failed reading {url}: {error}")))?;
        parse_version_response(&body)
    }

    fn verify_expected(version: &HealthVersion, config: &HealthConfig) -> UpdaterResult<()> {
        if let Some(expected) = &config.expected_version {
            if &version.version != expected {
                return Err(UpdaterError::Health(format!(
                    "health version mismatch: expected {expected}, got {}",
                    version.version
                )));
            }
        }
        if let Some(expected) = &config.expected_slot {
            if &version.slot != expected {
                return Err(UpdaterError::Health(format!(
                    "health slot mismatch: expected {expected}, got {}",
                    version.slot
                )));
            }
        }
        Ok(())
    }

    /// Check `/health/live`.
    pub async fn check_live(&self, config: &HealthConfig) -> bool {
        let url = format!("{}{}", config.base_url, HEALTH_LIVE);
        self.probe_with_retries(&url, config).await
    }

    /// Check `/health/ready`.
    pub async fn check_ready(&self, config: &HealthConfig) -> bool {
        let url = format!("{}{}", config.base_url, HEALTH_READY);
        self.probe_with_retries(&url, config).await
    }

    /// Run live + ready checks.
    pub async fn check_all(&self, config: &HealthConfig) -> HealthStatus {
        let live = self.check_live(config).await;
        let ready = if live {
            self.check_ready(config).await
        } else {
            false
        };
        HealthStatus { live, ready }
    }

    /// Smoke test: GET health endpoints and verify `/health/version`.
    pub async fn smoke_test(&self, config: &HealthConfig) -> UpdaterResult<()> {
        let status = self.check_all(config).await;
        if !status.is_healthy() {
            return Err(UpdaterError::Health(format!(
                "smoke test failed: live={}, ready={}",
                status.live, status.ready
            )));
        }
        let version = self.fetch_version(config).await?;
        Self::verify_expected(&version, config)
    }

    /// Perform several requests through the Caddy edge after switching.
    pub async fn post_switch_check(&self, config: &HealthConfig, rounds: u32) -> UpdaterResult<()> {
        for round in 0..rounds.max(1) {
            let live_url = format!("{}{}", config.base_url, HEALTH_LIVE);
            if !self.probe(&live_url, config.timeout).await {
                return Err(UpdaterError::Health(format!(
                    "post-switch live probe failed on round {}",
                    round + 1
                )));
            }
            let ready_url = format!("{}{}", config.base_url, HEALTH_READY);
            if !self.probe(&ready_url, config.timeout).await {
                return Err(UpdaterError::Health(format!(
                    "post-switch ready probe failed on round {}",
                    round + 1
                )));
            }
            let version = self.fetch_version(config).await?;
            Self::verify_expected(&version, config)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_config_from_upstream() {
        let cfg = HealthConfig::from_upstream("127.0.0.1:8081", Duration::from_secs(5));
        assert_eq!(cfg.base_url, "http://127.0.0.1:8081");
        assert!(cfg.expected_version.is_none());
    }

    #[test]
    fn health_status_requires_both() {
        assert!(!HealthStatus {
            live: true,
            ready: false
        }
        .is_healthy());
        assert!(HealthStatus {
            live: true,
            ready: true
        }
        .is_healthy());
    }

    #[test]
    fn parses_health_version() {
        let parsed = parse_version_response(br#"{"version":"1.2.3","slot":"green"}"#).unwrap();
        assert_eq!(
            parsed,
            HealthVersion {
                version: "1.2.3".into(),
                slot: "green".into()
            }
        );
    }

    #[test]
    fn rejects_malformed_health_version() {
        assert!(parse_version_response(br#"{"version":"1.2.3"}"#).is_err());
    }
}
