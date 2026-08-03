//! Release manifest JSON schema and validation.

use crate::error::{UpdaterError, UpdaterResult};
use crate::github::TargetArch;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Release manifest shipped as `manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseManifest {
    /// Semantic version (without leading v).
    pub version: String,
    /// RFC3339 release timestamp.
    pub released_at: String,
    /// Minimum version required to upgrade from.
    #[serde(default)]
    pub min_upgrade_version: Option<String>,
    /// Platform → artifact filename mapping.
    pub artifacts: HashMap<String, String>,
    /// Optional changelog summary.
    #[serde(default)]
    pub changelog: Option<String>,
}

impl ReleaseManifest {
    /// Validate manifest against expected version and architecture.
    pub fn validate(&self, expected_version: &str, arch: TargetArch) -> UpdaterResult<()> {
        if self.version != expected_version {
            return Err(UpdaterError::Manifest(format!(
                "manifest version {} does not match release {}",
                self.version, expected_version
            )));
        }
        if self.released_at.is_empty() {
            return Err(UpdaterError::Manifest(
                "released_at must not be empty".into(),
            ));
        }
        let key = arch.manifest_key();
        let expected_artifact = arch.artifact_name();
        match self.artifacts.get(key) {
            Some(name) if name == expected_artifact => {}
            Some(name) => {
                return Err(UpdaterError::Manifest(format!(
                    "artifact for {key} is {name}, expected {expected_artifact}"
                )));
            }
            None => {
                return Err(UpdaterError::Manifest(format!(
                    "missing artifact entry for platform {key}"
                )));
            }
        }
        Ok(())
    }

    /// Check whether current installed version satisfies `min_upgrade_version`.
    pub fn allows_upgrade_from(&self, current_version: &str) -> bool {
        match &self.min_upgrade_version {
            Some(min) => {
                crate::deploy::compare_versions(current_version, min) != std::cmp::Ordering::Less
            }
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> ReleaseManifest {
        let mut artifacts = HashMap::new();
        artifacts.insert("linux-amd64".into(), "pgpanel-linux-amd64.tar.gz".into());
        artifacts.insert("linux-arm64".into(), "pgpanel-linux-arm64.tar.gz".into());
        ReleaseManifest {
            version: "1.2.3".into(),
            released_at: "2026-01-01T00:00:00Z".into(),
            min_upgrade_version: Some("1.0.0".into()),
            artifacts,
            changelog: None,
        }
    }

    #[test]
    fn manifest_validates_matching_version() {
        let m = sample_manifest();
        m.validate("1.2.3", TargetArch::Amd64).unwrap();
    }

    #[test]
    fn manifest_rejects_version_mismatch() {
        let m = sample_manifest();
        let err = m.validate("9.9.9", TargetArch::Amd64).unwrap_err();
        assert!(matches!(err, UpdaterError::Manifest(_)));
    }

    #[test]
    fn allows_upgrade_from_checks_min() {
        let m = sample_manifest();
        assert!(m.allows_upgrade_from("1.1.0"));
        assert!(!m.allows_upgrade_from("0.5.0"));
    }
}
