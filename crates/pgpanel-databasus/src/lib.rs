//! Databasus backup service integration adapters.
//!
//! Does NOT write into Databasus internal databases.
//! Real HTTP adapter only after verifying the live Databasus API.
#![forbid(unsafe_code)]

mod http;
mod manual;
mod mock;
mod traits;

pub use http::HttpDatabasusAdapter;
pub use manual::ManualDatabasusAdapter;
pub use mock::MockDatabasusAdapter;
pub use traits::{BackupInfo, ClusterRegistration, DatabasusAdapter, RegisterRequest, WalStatus};

use pgpanel_core::config::Config;
use std::sync::Arc;

/// Build the appropriate adapter from config.
///
/// Default: ManualDatabasusAdapter (PendingManualSetup). Databasus does not
/// expose a stable, documented public provisioning API that we can safely
/// invent paths for. HTTP adapter is opt-in via DATABASUS_HTTP_API=1 after
/// verifying endpoint paths against the installed Databasus version.
pub fn build_adapter(config: &Config) -> Arc<dyn DatabasusAdapter> {
    if !config.databasus_http_api {
        tracing::info!(
            "Databasus HTTP API disabled (default); using Manual adapter — \
             set DATABASUS_HTTP_API=1 only after verifying API paths"
        );
        return Arc::new(ManualDatabasusAdapter::new());
    }

    match (&config.databasus_base_url, &config.databasus_token) {
        (Some(url), Some(token)) if !url.is_empty() && !token.is_empty() => {
            tracing::info!(%url, "using HttpDatabasusAdapter (DATABASUS_HTTP_API=1)");
            Arc::new(HttpDatabasusAdapter::new(url.clone(), token.clone()))
        }
        (Some(url), _) if !url.is_empty() => {
            tracing::warn!("DATABASUS_HTTP_API=1 but no token; using Manual adapter");
            Arc::new(ManualDatabasusAdapter::new())
        }
        _ => {
            tracing::info!("DATABASUS_HTTP_API=1 but no URL; using Manual adapter");
            Arc::new(ManualDatabasusAdapter::new())
        }
    }
}

/// Build a mock adapter for tests.
pub fn build_mock_adapter() -> Arc<dyn DatabasusAdapter> {
    Arc::new(MockDatabasusAdapter::new())
}
