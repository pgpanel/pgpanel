//! GitHub Releases API client.

use crate::error::{UpdaterError, UpdaterResult};
use crate::manifest::ReleaseManifest;
use pgpanel_core::config::UpdatesConfig;
use serde::Deserialize;
use std::path::Path;

/// CPU architecture for release artifact selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetArch {
    /// x86_64 Linux.
    Amd64,
    /// aarch64 Linux.
    Arm64,
}

impl TargetArch {
    /// Detect from compile-time target or runtime override.
    pub fn detect() -> Self {
        #[cfg(target_arch = "aarch64")]
        {
            Self::Arm64
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            Self::Amd64
        }
    }

    /// Parse from CLI string.
    pub fn parse(s: &str) -> UpdaterResult<Self> {
        match s.to_lowercase().as_str() {
            "amd64" | "x86_64" => Ok(Self::Amd64),
            "arm64" | "aarch64" => Ok(Self::Arm64),
            other => Err(UpdaterError::Config(format!("unknown arch: {other}"))),
        }
    }

    /// Artifact basename.
    pub fn artifact_name(&self) -> &'static str {
        match self {
            Self::Amd64 => "pgpanel-linux-amd64.tar.gz",
            Self::Arm64 => "pgpanel-linux-arm64.tar.gz",
        }
    }

    /// Manifest platform key.
    pub fn manifest_key(&self) -> &'static str {
        match self {
            Self::Amd64 => "linux-amd64",
            Self::Arm64 => "linux-arm64",
        }
    }
}

/// GitHub release metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct GhRelease {
    /// Tag name (e.g. v1.2.3).
    pub tag_name: String,
    /// Whether this is a prerelease.
    pub prerelease: bool,
    /// Published timestamp.
    pub published_at: String,
    /// Release assets.
    pub assets: Vec<GhAsset>,
}

/// GitHub release asset.
#[derive(Debug, Clone, Deserialize)]
pub struct GhAsset {
    /// Asset filename.
    pub name: String,
    /// Browser download URL (HTTPS).
    pub browser_download_url: String,
    /// Size in bytes.
    pub size: u64,
}

/// Release check result.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    /// Normalized version (without leading v).
    pub version: String,
    /// GitHub tag.
    pub tag: String,
    /// Full release metadata.
    pub release: GhRelease,
    /// Selected architecture.
    pub arch: TargetArch,
}

impl ReleaseInfo {
    /// Find asset by name.
    pub fn asset(&self, name: &str) -> UpdaterResult<&GhAsset> {
        self.release
            .assets
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| UpdaterError::Release(format!("asset not found: {name}")))
    }

    /// Primary tarball asset for selected arch.
    pub fn tarball_asset(&self) -> UpdaterResult<&GhAsset> {
        self.asset(self.arch.artifact_name())
    }
}

/// GitHub Releases client.
#[derive(Debug, Clone)]
pub struct GitHubClient {
    client: reqwest::Client,
    owner: String,
    repo: String,
    channel: String,
}

impl GitHubClient {
    /// Create client from update config.
    pub fn new(updates: &UpdatesConfig) -> UpdaterResult<Self> {
        let client = reqwest::Client::builder()
            .user_agent(format!("pgpanel-updater/{}", pgpanel_core::VERSION))
            .https_only(true)
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()
            .map_err(|e| UpdaterError::Network(e.to_string()))?;
        Ok(Self {
            client,
            owner: updates.github_owner.clone(),
            repo: updates.github_repo.clone(),
            channel: updates.channel.clone(),
        })
    }

    fn releases_url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/releases",
            self.owner, self.repo
        )
    }

    fn release_by_tag_url(&self, tag: &str) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/releases/tags/{}",
            self.owner, self.repo, tag
        )
    }

    async fn fetch_releases(&self) -> UpdaterResult<Vec<GhRelease>> {
        let resp = self
            .client
            .get(self.releases_url())
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| UpdaterError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(UpdaterError::Release(format!(
                "GitHub API error: {}",
                resp.status()
            )));
        }
        resp.json()
            .await
            .map_err(|e| UpdaterError::Release(e.to_string()))
    }

    async fn fetch_release_by_tag(&self, tag: &str) -> UpdaterResult<GhRelease> {
        let resp = self
            .client
            .get(self.release_by_tag_url(tag))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| UpdaterError::Network(e.to_string()))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(UpdaterError::NotFound(format!("release tag {tag}")));
        }
        if !resp.status().is_success() {
            return Err(UpdaterError::Release(format!(
                "GitHub API error: {}",
                resp.status()
            )));
        }
        resp.json()
            .await
            .map_err(|e| UpdaterError::Release(e.to_string()))
    }

    fn filter_channel(releases: Vec<GhRelease>, channel: &str) -> Vec<GhRelease> {
        releases
            .into_iter()
            .filter(|r| match channel {
                "stable" => !r.prerelease,
                "prerelease" => true,
                _ => !r.prerelease,
            })
            .collect()
    }

    /// Check for the latest release matching channel and arch.
    pub async fn check_latest(&self, arch: TargetArch) -> UpdaterResult<Option<ReleaseInfo>> {
        let releases = Self::filter_channel(self.fetch_releases().await?, &self.channel);
        Ok(releases
            .into_iter()
            .next()
            .map(|r| Self::to_release_info(r, arch)))
    }

    /// Resolve a specific version or tag.
    pub async fn resolve_release(
        &self,
        version: &str,
        arch: TargetArch,
    ) -> UpdaterResult<ReleaseInfo> {
        let tag = if version.starts_with('v') {
            version.to_string()
        } else {
            format!("v{version}")
        };
        let release = self.fetch_release_by_tag(&tag).await?;
        if self.channel == "stable" && release.prerelease {
            return Err(UpdaterError::Release(
                "prerelease not allowed on stable channel".into(),
            ));
        }
        Ok(Self::to_release_info(release, arch))
    }

    fn to_release_info(release: GhRelease, arch: TargetArch) -> ReleaseInfo {
        let version = release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name)
            .to_string();
        ReleaseInfo {
            version,
            tag: release.tag_name.clone(),
            release,
            arch,
        }
    }

    /// Download asset bytes with maximum size enforcement.
    pub async fn download_asset(&self, asset: &GhAsset, max_bytes: u64) -> UpdaterResult<Vec<u8>> {
        if asset.size > max_bytes {
            return Err(UpdaterError::Download(format!(
                "asset {} exceeds max download size ({} > {})",
                asset.name, asset.size, max_bytes
            )));
        }
        let resp = self
            .client
            .get(&asset.browser_download_url)
            .send()
            .await
            .map_err(|e| UpdaterError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(UpdaterError::Download(format!(
                "download failed for {}: {}",
                asset.name,
                resp.status()
            )));
        }
        if resp.url().scheme() != "https" {
            return Err(UpdaterError::Download(
                "refusing non-HTTPS download redirect".into(),
            ));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| UpdaterError::Download(e.to_string()))?;
        if bytes.len() as u64 > max_bytes {
            return Err(UpdaterError::Download(format!(
                "downloaded {} exceeds max {}",
                bytes.len(),
                max_bytes
            )));
        }
        Ok(bytes.to_vec())
    }

    /// Download release artifacts needed for update into `dest_dir`.
    pub async fn download_release_bundle(
        &self,
        info: &ReleaseInfo,
        dest_dir: &Path,
        max_bytes: u64,
    ) -> UpdaterResult<ReleaseManifest> {
        std::fs::create_dir_all(dest_dir)?;
        let required = [
            info.arch.artifact_name(),
            "SHA256SUMS",
            "SHA256SUMS.sig",
            "manifest.json",
        ];
        for name in required {
            let asset = info.asset(name)?;
            let data = self.download_asset(asset, max_bytes).await?;
            std::fs::write(dest_dir.join(name), data)?;
        }
        let manifest_bytes = std::fs::read(dest_dir.join("manifest.json"))?;
        let manifest: ReleaseManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| UpdaterError::Manifest(e.to_string()))?;
        manifest.validate(&info.version, info.arch)?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arch_artifact_names() {
        assert_eq!(
            TargetArch::Amd64.artifact_name(),
            "pgpanel-linux-amd64.tar.gz"
        );
        assert_eq!(
            TargetArch::Arm64.artifact_name(),
            "pgpanel-linux-arm64.tar.gz"
        );
    }

    #[test]
    fn arch_parse() {
        assert_eq!(TargetArch::parse("amd64").unwrap(), TargetArch::Amd64);
        assert_eq!(TargetArch::parse("arm64").unwrap(), TargetArch::Arm64);
    }
}
