//! Atomic Caddy upstream switching and reload.

use crate::deploy::DeploySlot;
use crate::error::{UpdaterError, UpdaterResult};
use pgpanel_core::config::CaddyConfig;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Caddy upstream manager.
#[derive(Debug, Clone)]
pub struct CaddyManager {
    config: CaddyConfig,
}

impl CaddyManager {
    /// Create from application config.
    pub fn new(config: CaddyConfig) -> Self {
        Self { config }
    }

    /// Path to upstream snippet file.
    pub fn upstream_path(&self) -> &Path {
        &self.config.upstream_config_path
    }

    /// Upstream address for a slot.
    pub fn upstream_for_slot(&self, slot: DeploySlot) -> &str {
        match slot {
            DeploySlot::Blue => &self.config.blue_upstream,
            DeploySlot::Green => &self.config.green_upstream,
        }
    }

    /// Generate Caddy snippet content for active slot.
    pub fn render_upstream(&self, active: DeploySlot) -> String {
        let upstream = self.upstream_for_slot(active);
        format!(
            "# Managed by pgpanel-updater — do not edit manually\n\
             # Active slot: {}\n\
             reverse_proxy {upstream} {{\n\
             \thealth_uri /health/live\n\
             \thealth_interval 10s\n\
             \thealth_timeout 5s\n\
             }}\n",
            active.as_str()
        )
    }

    /// Atomically write upstream config and reload Caddy.
    pub fn switch_upstream(&self, active: DeploySlot) -> UpdaterResult<()> {
        let content = self.render_upstream(active);
        atomic_write(&self.config.upstream_config_path, content.as_bytes())?;
        self.reload()
    }

    /// Reload Caddy using configured command.
    pub fn reload(&self) -> UpdaterResult<()> {
        if self.config.reload_argv.is_empty() {
            return Err(UpdaterError::Caddy(
                "reload_argv is empty in configuration".into(),
            ));
        }
        let program = &self.config.reload_argv[0];
        let args = &self.config.reload_argv[1..];
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| UpdaterError::Caddy(format!("failed to execute reload: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(UpdaterError::Caddy(format!(
                "caddy reload failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            )));
        }
        Ok(())
    }

    /// Read currently configured active upstream (best-effort parse).
    pub fn read_active_slot(&self) -> UpdaterResult<Option<DeploySlot>> {
        let path = &self.config.upstream_config_path;
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)?;
        if content.contains(&self.config.blue_upstream) {
            return Ok(Some(DeploySlot::Blue));
        }
        if content.contains(&self.config.green_upstream) {
            return Ok(Some(DeploySlot::Green));
        }
        Ok(None)
    }
}

/// Write `path` atomically via temp file + rename in same directory.
pub fn atomic_write(path: &Path, content: &[u8]) -> UpdaterResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| UpdaterError::Caddy("upstream path has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .ok_or_else(|| UpdaterError::Caddy("upstream path has no filename".into()))?;
    let tmp_path: PathBuf = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgpanel_core::config::CaddyConfig;
    use tempfile::TempDir;

    fn test_caddy(dir: &Path) -> CaddyManager {
        let reload = if cfg!(target_os = "macos") {
            vec!["/usr/bin/true".into()]
        } else {
            vec!["/bin/true".into()]
        };
        CaddyManager::new(CaddyConfig {
            upstream_config_path: dir.join("upstream.caddy"),
            blue_upstream: "127.0.0.1:8081".into(),
            green_upstream: "127.0.0.1:8082".into(),
            admin_endpoint: None,
            reload_argv: reload,
        })
    }

    #[test]
    fn render_upstream_contains_slot() {
        let dir = TempDir::new().unwrap();
        let mgr = test_caddy(dir.path());
        let content = mgr.render_upstream(DeploySlot::Green);
        assert!(content.contains("127.0.0.1:8082"));
        assert!(content.contains("green"));
    }

    #[test]
    fn atomic_write_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.caddy");
        atomic_write(&path, b"test content").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "test content");
    }

    #[test]
    fn switch_upstream_writes_and_parses() {
        let dir = TempDir::new().unwrap();
        let mgr = test_caddy(dir.path());
        mgr.switch_upstream(DeploySlot::Blue).unwrap();
        assert_eq!(mgr.read_active_slot().unwrap(), Some(DeploySlot::Blue));
    }
}
