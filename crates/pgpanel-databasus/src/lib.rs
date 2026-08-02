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
/// If DATABASUS_BASE_URL is set, uses HttpDatabasusAdapter.
/// Otherwise uses ManualDatabasusAdapter (pending manual setup).
pub fn build_adapter(config: &Config) -> Arc<dyn DatabasusAdapter> {
    match (&config.databasus_base_url, &config.databasus_token) {
        (Some(url), Some(token)) if !url.is_empty() => {
            tracing::info!(%url, "using HttpDatabasusAdapter");
            Arc::new(HttpDatabasusAdapter::new(url.clone(), token.clone()))
        }
        (Some(url), _) if !url.is_empty() => {
            tracing::warn!("DATABASUS_BASE_URL set but no token; using Manual adapter");
            Arc::new(ManualDatabasusAdapter::new())
        }
        _ => {
            tracing::info!("no Databasus URL configured; using Manual adapter");
            Arc::new(ManualDatabasusAdapter::new())
        }
    }
}

/// Build a mock adapter for tests.
pub fn build_mock_adapter() -> Arc<dyn DatabasusAdapter> {
    Arc::new(MockDatabasusAdapter::new())
}
