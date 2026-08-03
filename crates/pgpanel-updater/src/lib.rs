//! PgPanel blue-green updater library.

#![deny(unsafe_code)]
#![warn(clippy::all)]

pub mod archive;
pub mod caddy;
#[cfg(unix)]
pub mod client;
#[cfg(unix)]
pub mod daemon;
pub mod deploy;
pub mod github;
pub mod health;
#[cfg(unix)]
pub mod peer;
pub mod progress;
pub mod protocol;
pub mod verify;

mod error;
pub mod manifest;

#[cfg(unix)]
pub use client::UpdaterClient;
#[cfg(unix)]
pub use daemon::UpdaterDaemon;
pub use deploy::{
    atomic_symlink, compare_versions, is_newer_version, service_name, DeployManager, DeploySlot,
    DeployStatus, UpdateLock,
};
pub use error::{UpdaterError, UpdaterResult};
pub use github::{GitHubClient, ReleaseInfo, TargetArch};
pub use manifest::ReleaseManifest;
pub use progress::{
    ChannelProgress, NullProgress, ProgressEvent, ProgressReporter, ProgressSink, UpdatePhase,
};
pub use protocol::{
    DaemonPhase, OperationOutcome, ProgressRecord, ReleaseMetadata, UpdaterRequest,
    UpdaterResponse, UpdaterStatus,
};

use pgpanel_core::config::Config;
use progress::NullProgress as NullProgressSink;

/// High-level updater facade.
#[derive(Debug)]
pub struct Updater {
    config: Config,
    deploy: DeployManager,
    github: GitHubClient,
}

impl Updater {
    /// Load updater from config path.
    pub fn from_config_path(path: &std::path::Path) -> UpdaterResult<Self> {
        let config = Config::load(path).map_err(|e| UpdaterError::Config(e.to_string()))?;
        Self::new(config)
    }

    /// Create from an already-loaded config.
    pub fn new(config: Config) -> UpdaterResult<Self> {
        let github = GitHubClient::new(&config.updates)?;
        let deploy = DeployManager::new(config.clone());
        Ok(Self {
            config,
            deploy,
            github,
        })
    }

    /// Reference to deployment manager.
    pub fn deploy(&self) -> &DeployManager {
        &self.deploy
    }

    /// Reference to loaded config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Check for available updates.
    pub async fn check(&self, arch: TargetArch) -> UpdaterResult<Option<ReleaseInfo>> {
        self.github.check_latest(arch).await
    }

    /// Run update to latest or specific version.
    pub async fn update(
        &self,
        arch: TargetArch,
        version: Option<&str>,
        allow_downgrade: bool,
    ) -> UpdaterResult<String> {
        let mut reporter = ProgressReporter::new(NullProgressSink);
        self.update_with_progress(arch, version, allow_downgrade, &mut reporter)
            .await
    }

    /// Run update with progress reporting.
    pub async fn update_with_progress<S: ProgressSink>(
        &self,
        arch: TargetArch,
        version: Option<&str>,
        allow_downgrade: bool,
        progress: &mut ProgressReporter<S>,
    ) -> UpdaterResult<String> {
        self.deploy
            .update(&self.github, arch, version, allow_downgrade, progress)
            .await
    }

    /// Roll back to previous release.
    pub async fn rollback(&self) -> UpdaterResult<String> {
        let mut reporter = ProgressReporter::new(NullProgressSink);
        self.rollback_with_progress(&mut reporter).await
    }

    /// Roll back with progress reporting.
    pub async fn rollback_with_progress<S: ProgressSink>(
        &self,
        progress: &mut ProgressReporter<S>,
    ) -> UpdaterResult<String> {
        self.deploy.rollback(progress).await
    }

    /// Current deployment status.
    pub fn status(&self) -> UpdaterResult<DeployStatus> {
        self.deploy.status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_public_api() {
        assert!(is_newer_version("2.0.0", "1.9.9"));
        assert!(!is_newer_version("1.0.0", "1.0.1"));
    }
}
