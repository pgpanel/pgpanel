//! Blue-green deployment orchestration.

use crate::archive::{extract_tar_gz, set_release_permissions};
use crate::caddy::CaddyManager;
use crate::error::{UpdaterError, UpdaterResult};
use crate::github::{GitHubClient, TargetArch};
use crate::health::{HealthChecker, HealthConfig};
use crate::progress::{ProgressReporter, ProgressSink, UpdatePhase};
use crate::verify::{verify_artifact_checksum, verify_sha256sums, verify_sha256sums_signature};
use pgpanel_core::config::Config;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Blue or green deployment slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeploySlot {
    /// Blue slot (default port 8081).
    Blue,
    /// Green slot (default port 8082).
    Green,
}

impl DeploySlot {
    /// Parse slot name.
    pub fn parse(s: &str) -> UpdaterResult<Self> {
        match s.to_lowercase().as_str() {
            "blue" => Ok(Self::Blue),
            "green" => Ok(Self::Green),
            other => Err(UpdaterError::Config(format!("invalid slot: {other}"))),
        }
    }

    /// Opposite slot for blue-green switch.
    pub fn opposite(self) -> Self {
        match self {
            Self::Blue => Self::Green,
            Self::Green => Self::Blue,
        }
    }

    /// String representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Green => "green",
        }
    }
}

/// Deployment status snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeployStatus {
    /// Currently active slot serving traffic.
    pub active_slot: DeploySlot,
    /// Version at `current` symlink.
    pub current_version: Option<String>,
    /// Version at `previous` symlink (rollback target).
    pub previous_version: Option<String>,
    /// All installed release versions.
    pub installed_versions: Vec<String>,
    /// Whether an update lock file exists (best-effort).
    pub update_in_progress: bool,
}

/// Compare two semver-like version strings.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split(['.', '-'])
            .filter_map(|p| p.parse::<u64>().ok())
            .collect()
    };
    let va = parse(a);
    let vb = parse(b);
    let len = va.len().max(vb.len());
    for i in 0..len {
        let ai = va.get(i).copied().unwrap_or(0);
        let bi = vb.get(i).copied().unwrap_or(0);
        match ai.cmp(&bi) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// Returns true if `candidate` is strictly newer than `current`.
pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current) == std::cmp::Ordering::Greater
}

/// Global update lock via `flock(2)`.
pub struct UpdateLock {
    _flock: nix::fcntl::Flock<std::fs::File>,
    path: PathBuf,
}

impl UpdateLock {
    /// Acquire exclusive non-blocking lock.
    pub fn try_acquire(lock_path: &Path) -> UpdaterResult<Self> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(lock_path)?;
        writeln!(file, "pid={}", std::process::id())?;
        let flock = nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock)
            .map_err(|(_, e)| UpdaterError::Lock(format!("failed to acquire update lock: {e}")))?;
        Ok(Self {
            _flock: flock,
            path: lock_path.to_path_buf(),
        })
    }

    /// Lock file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Blue-green deployment manager.
#[derive(Debug)]
pub struct DeployManager {
    config: Config,
    install_root: PathBuf,
    releases_dir: PathBuf,
    current_link: PathBuf,
    previous_link: PathBuf,
    slots_dir: PathBuf,
    lock_path: PathBuf,
    staging_dir: PathBuf,
    backups_dir: PathBuf,
}

impl DeployManager {
    /// Create from loaded application config.
    pub fn new(config: Config) -> Self {
        let install_root = config.updates.install_root.clone();
        let releases_dir = install_root.join("releases");
        let current_link = install_root.join("current");
        let previous_link = install_root.join("previous");
        let slots_dir = install_root.join("slots");
        let lock_path = install_root.join("state").join("update.lock");
        let staging_dir = install_root.join("state").join("staging");
        let backups_dir = install_root.join("state").join("backups");
        Self {
            config,
            install_root,
            releases_dir,
            current_link,
            previous_link,
            slots_dir,
            lock_path,
            staging_dir,
            backups_dir,
        }
    }

    /// Install root path.
    pub fn install_root(&self) -> &Path {
        &self.install_root
    }

    /// Read deployment status.
    pub fn status(&self) -> UpdaterResult<DeployStatus> {
        let active_slot = self.active_slot()?;
        let current_version = self.read_link_version(&self.current_link)?;
        let previous_version = self.read_link_version(&self.previous_link)?;
        let installed_versions = self.list_installed_versions()?;
        let update_in_progress = lock_is_held(&self.lock_path);
        Ok(DeployStatus {
            active_slot,
            current_version,
            previous_version,
            installed_versions,
            update_in_progress,
        })
    }

    /// Determine active slot from config / Caddy.
    pub fn active_slot(&self) -> UpdaterResult<DeploySlot> {
        let caddy = CaddyManager::new(self.config.caddy.clone());
        if let Some(slot) = caddy.read_active_slot()? {
            return Ok(slot);
        }
        DeploySlot::parse(&self.config.app.active_slot)
    }

    fn read_link_version(&self, link: &Path) -> UpdaterResult<Option<String>> {
        if !link.exists() {
            return Ok(None);
        }
        let target = fs::read_link(link).map_err(UpdaterError::Io)?;
        Ok(target
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string()))
    }

    fn list_installed_versions(&self) -> UpdaterResult<Vec<String>> {
        if !self.releases_dir.exists() {
            return Ok(Vec::new());
        }
        let mut versions = Vec::new();
        for entry in fs::read_dir(&self.releases_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    versions.push(name.to_string());
                }
            }
        }
        versions.sort_by(|a, b| compare_versions(b, a));
        Ok(versions)
    }

    /// Path to a specific release directory.
    pub fn release_dir(&self, version: &str) -> PathBuf {
        self.releases_dir.join(version)
    }

    /// Path to the systemd-managed `current` link for a slot.
    pub fn slot_current_link(&self, slot: DeploySlot) -> PathBuf {
        self.slots_dir.join(slot.as_str()).join("current")
    }

    fn read_link_target(&self, link: &Path) -> UpdaterResult<Option<PathBuf>> {
        match fs::symlink_metadata(link) {
            Ok(_) => Ok(Some(fs::read_link(link)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn read_slot_version(&self, slot: DeploySlot) -> UpdaterResult<Option<String>> {
        let link = self.slot_current_link(slot);
        Ok(self.read_link_target(&link)?.and_then(|target| {
            target
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        }))
    }

    /// Point one slot at a release, returning its prior target for rollback.
    pub fn update_slot_symlink(
        &self,
        slot: DeploySlot,
        version: &str,
    ) -> UpdaterResult<Option<PathBuf>> {
        let target = self.release_dir(version);
        if !target.is_dir() {
            return Err(UpdaterError::Deploy(format!(
                "release directory missing: {}",
                target.display()
            )));
        }
        let link = self.slot_current_link(slot);
        let prior = self.read_link_target(&link)?;
        atomic_symlink(&target, &link)?;
        Ok(prior)
    }

    /// Restore a slot link captured by [`update_slot_symlink`].
    pub fn restore_slot_symlink(
        &self,
        slot: DeploySlot,
        prior_target: Option<&Path>,
    ) -> UpdaterResult<()> {
        let link = self.slot_current_link(slot);
        match prior_target {
            Some(target) => atomic_symlink(target, &link),
            None => remove_symlink(&link),
        }
    }

    /// Update `current` and `previous` symlinks.
    pub fn update_symlinks(
        &self,
        new_version: &str,
        old_version: Option<&str>,
    ) -> UpdaterResult<()> {
        let new_target = self.release_dir(new_version);
        if !new_target.exists() {
            return Err(UpdaterError::Deploy(format!(
                "release directory missing: {}",
                new_target.display()
            )));
        }
        if let Some(old) = old_version {
            let old_target = self.release_dir(old);
            if !old_target.is_dir() {
                return Err(UpdaterError::Deploy(format!(
                    "previous release directory missing: {}",
                    old_target.display()
                )));
            }
            atomic_symlink(&old_target, &self.previous_link)?;
        } else {
            remove_symlink(&self.previous_link)?;
        }
        atomic_symlink(&new_target, &self.current_link)?;
        Ok(())
    }

    /// Backup SQLite database before update.
    pub fn backup_sqlite(&self) -> UpdaterResult<PathBuf> {
        fs::create_dir_all(&self.backups_dir)?;
        let src = &self.config.paths.sqlite;
        if !src.exists() {
            tracing::warn!(path = %src.display(), "sqlite database does not exist, skipping backup");
            return Ok(self.backups_dir.join("skipped.no-db"));
        }
        let dest = self
            .backups_dir
            .join(format!("pgpanel-{}.db.bak", timestamp_secs()));
        let sqlite = sqlite_program()?;
        let temp_dest = dest.with_extension("db.bak.tmp");
        let _ = fs::remove_file(&temp_dest);
        let escaped_dest = temp_dest.to_string_lossy().replace('\'', "''");
        let backup_command = format!(".backup '{escaped_dest}'");
        let output = Command::new(sqlite)
            .arg(src)
            .arg(backup_command)
            .output()
            .map_err(|e| UpdaterError::Deploy(format!("failed to execute sqlite3 backup: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = fs::remove_file(&temp_dest);
            return Err(UpdaterError::Deploy(format!(
                "sqlite3 online backup failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            )));
        }
        if !temp_dest.is_file() {
            return Err(UpdaterError::Deploy(
                "sqlite3 backup completed without creating a backup file".into(),
            ));
        }
        fs::rename(&temp_dest, &dest)?;
        Ok(dest)
    }

    /// Full update flow.
    pub async fn update<S: ProgressSink>(
        &self,
        github: &GitHubClient,
        arch: TargetArch,
        target_version: Option<&str>,
        allow_downgrade: bool,
        progress: &mut ProgressReporter<S>,
    ) -> UpdaterResult<String> {
        let _lock = {
            progress.emit(UpdatePhase::Locking, "acquiring update lock");
            UpdateLock::try_acquire(&self.lock_path)?
        };

        progress.emit(UpdatePhase::CheckingRelease, "checking GitHub releases");
        let release_info = if let Some(v) = target_version {
            github.resolve_release(v, arch).await?
        } else if let Some(info) = github.check_latest(arch).await? {
            info
        } else {
            return Err(UpdaterError::NotFound("no suitable release found".into()));
        };

        let current_version = self.read_link_version(&self.current_link)?;
        if let Some(ref current) = current_version {
            if !allow_downgrade && !is_newer_version(&release_info.version, current) {
                return Err(UpdaterError::Version(format!(
                    "refusing to install {} — not newer than current {current}",
                    release_info.version
                )));
            }
        }

        progress.emit_detail(
            UpdatePhase::Downloading,
            format!("downloading release {}", release_info.version),
            serde_json::json!({ "version": release_info.version, "tag": release_info.tag }),
        );
        if self.staging_dir.exists() {
            fs::remove_dir_all(&self.staging_dir)?;
        }
        fs::create_dir_all(&self.staging_dir)?;
        let manifest = github
            .download_release_bundle(
                &release_info,
                &self.staging_dir,
                self.config.updates.max_download_bytes,
            )
            .await?;

        if let Some(ref current) = current_version {
            if !manifest.allows_upgrade_from(current) {
                return Err(UpdaterError::Version(format!(
                    "current version {current} is below minimum upgrade version"
                )));
            }
        }

        progress.emit(UpdatePhase::Verifying, "verifying checksums and signature");
        let sums_path = self.staging_dir.join("SHA256SUMS");
        let sig_path = self.staging_dir.join("SHA256SUMS.sig");
        let sums_content = fs::read_to_string(&sums_path)?;
        let sig_content = fs::read(&sig_path)?;
        verify_sha256sums_signature(
            &self.config.paths.signing_public_key,
            sums_content.as_bytes(),
            &sig_content,
        )?;
        let tarball = self.staging_dir.join(release_info.arch.artifact_name());
        verify_sha256sums(
            &sums_content,
            &self.staging_dir,
            &[release_info.arch.artifact_name(), "manifest.json"],
        )?;
        verify_artifact_checksum(&tarball, &sums_content, release_info.arch.artifact_name())?;

        progress.emit(
            UpdatePhase::ValidatingManifest,
            "validating release manifest",
        );
        manifest.validate(&release_info.version, release_info.arch)?;

        if !is_safe_version_component(&release_info.version) {
            return Err(UpdaterError::Version(
                "release version contains unsafe path characters".into(),
            ));
        }
        let version = release_info.version.clone();
        let release_path = self.release_dir(&version);
        if release_path.exists() {
            fs::remove_dir_all(&release_path)?;
        }
        fs::create_dir_all(&release_path)?;

        progress.emit(UpdatePhase::Extracting, "extracting release archive");
        extract_tar_gz(&tarball, &release_path)?;

        progress.emit(
            UpdatePhase::SettingPermissions,
            "setting release permissions",
        );
        set_release_permissions(&release_path)?;

        progress.emit(UpdatePhase::BackingUpDatabase, "backing up SQLite database");
        self.backup_sqlite()?;

        let active = self.active_slot()?;
        let target_slot = active.opposite();
        self.deploy_version_to_slot(
            &version,
            target_slot,
            active,
            current_version.as_deref(),
            progress,
        )
        .await
    }

    /// Roll back to the `previous` release.
    pub async fn rollback<S: ProgressSink>(
        &self,
        progress: &mut ProgressReporter<S>,
    ) -> UpdaterResult<String> {
        let _lock = {
            progress.emit(UpdatePhase::Locking, "acquiring update lock");
            UpdateLock::try_acquire(&self.lock_path)?
        };

        let previous_version = self
            .read_link_version(&self.previous_link)?
            .ok_or_else(|| UpdaterError::Rollback("no previous release available".into()))?;
        let current_version = self.read_link_version(&self.current_link)?;
        let release_path = self.release_dir(&previous_version);
        if !release_path.exists() {
            return Err(UpdaterError::Rollback(format!(
                "previous release directory missing: {}",
                release_path.display()
            )));
        }

        progress.emit_detail(
            UpdatePhase::Rollback,
            format!("rolling back to {previous_version}"),
            serde_json::json!({ "previous": previous_version }),
        );

        let active = self.active_slot()?;
        let target_slot = active.opposite();
        self.deploy_version_to_slot(
            &previous_version,
            target_slot,
            active,
            current_version.as_deref(),
            progress,
        )
        .await
    }

    async fn deploy_version_to_slot<S: ProgressSink>(
        &self,
        version: &str,
        target_slot: DeploySlot,
        active_slot: DeploySlot,
        old_version: Option<&str>,
        progress: &mut ProgressReporter<S>,
    ) -> UpdaterResult<String> {
        let health = HealthChecker::new()?;
        let upstream = match target_slot {
            DeploySlot::Blue => &self.config.caddy.blue_upstream,
            DeploySlot::Green => &self.config.caddy.green_upstream,
        };
        let mut health_cfg = HealthConfig::from_upstream(
            upstream,
            Duration::from_secs(self.config.updates.timeout_secs.min(30)),
        );
        health_cfg.expected_version = Some(version.to_string());
        health_cfg.expected_slot = Some(target_slot.as_str().to_string());
        let prior_target = self.read_link_target(&self.slot_current_link(target_slot))?;
        let prior_current = self.read_link_target(&self.current_link)?;
        let prior_previous = self.read_link_target(&self.previous_link)?;
        let active_version = self.read_slot_version(active_slot)?;
        let effective_old_version = old_version.or(active_version.as_deref());
        let mut traffic_switched = false;
        let mut globals_updated = false;

        let result: UpdaterResult<String> = async {
            progress.emit_detail(
                UpdatePhase::StartingSlot,
                format!("starting {} slot", target_slot.as_str()),
                serde_json::json!({ "slot": target_slot.as_str(), "version": version }),
            );
            self.update_slot_symlink(target_slot, version)?;
            restart_slot_service(target_slot)?;

            progress.emit(UpdatePhase::HealthCheck, "running health checks");
            if !health.check_all(&health_cfg).await.is_healthy() {
                return Err(UpdaterError::Health(
                    "inactive slot failed health checks".into(),
                ));
            }

            progress.emit(UpdatePhase::SmokeTest, "running smoke tests");
            health.smoke_test(&health_cfg).await?;

            progress.emit(UpdatePhase::SwitchingTraffic, "switching Caddy upstream");
            let caddy = CaddyManager::new(self.config.caddy.clone());
            // Treat a successful config write as switched even if reload
            // fails; rollback must restore the persisted Caddy selection too.
            traffic_switched = true;
            caddy.switch_upstream(target_slot)?;

            progress.emit(UpdatePhase::SmokeTest, "checking traffic through Caddy");
            let mut edge_cfg = HealthConfig::from_upstream(
                "127.0.0.1:8080",
                Duration::from_secs(self.config.updates.timeout_secs.min(30)),
            );
            edge_cfg.expected_version = Some(version.to_string());
            edge_cfg.expected_slot = Some(target_slot.as_str().to_string());
            health.post_switch_check(&edge_cfg, 3).await?;

            progress.emit(UpdatePhase::Finalizing, "updating global release symlinks");
            globals_updated = true;
            self.update_symlinks(version, effective_old_version)?;

            progress.emit(UpdatePhase::Draining, "draining old slot connections");
            tokio::time::sleep(Duration::from_secs(self.config.updates.drain_secs)).await;

            progress.emit(UpdatePhase::StoppingOldSlot, "stopping old deployment slot");
            stop_slot_service(active_slot)?;

            progress.emit_detail(
                UpdatePhase::Complete,
                format!("deployment complete: {version}"),
                serde_json::json!({ "version": version, "active_slot": target_slot.as_str() }),
            );
            Ok(version.to_string())
        }
        .await;

        if result.is_err() {
            progress.emit_detail(
                UpdatePhase::Rollback,
                if traffic_switched {
                    "deployment failed after traffic switch — restoring old slot"
                } else {
                    "deployment failed before traffic switch — restoring inactive slot"
                },
                serde_json::json!({ "error": result.as_ref().err().map(|e| e.to_string()) }),
            );
            let caddy = CaddyManager::new(self.config.caddy.clone());
            if traffic_switched {
                if let Err(error) = caddy.switch_upstream(active_slot) {
                    tracing::error!(%error, "failed to restore Caddy traffic");
                }
                if let Err(error) = start_slot_service(active_slot) {
                    tracing::error!(%error, "failed to restart old deployment slot");
                }
            }
            if let Err(error) = stop_slot_service(target_slot) {
                tracing::error!(%error, "failed to stop failed deployment slot");
            }
            if let Err(error) = self.restore_slot_symlink(target_slot, prior_target.as_deref()) {
                tracing::error!(%error, "failed to restore inactive slot symlink");
            }
            if globals_updated {
                if let Err(error) = restore_link(&self.current_link, prior_current.as_deref()) {
                    tracing::error!(%error, "failed to restore current symlink");
                }
                if let Err(error) = restore_link(&self.previous_link, prior_previous.as_deref()) {
                    tracing::error!(%error, "failed to restore previous symlink");
                }
            }
        }

        result
    }
}

/// Check whether another process currently owns the update flock.
///
/// The lock file is deliberately persistent so its existence is not a useful
/// indication of an active update. A non-blocking flock is the authoritative
/// check; a successful probe is immediately released.
fn lock_is_held(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "could not inspect update flock");
            return false;
        }
    };
    match nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock) {
        Ok(_flock) => false,
        Err((_file, nix::errno::Errno::EAGAIN)) => true,
        Err((_file, error)) => {
            tracing::warn!(%error, path = %path.display(), "unexpected update flock probe error");
            false
        }
    }
}

/// Map a deployment slot to its fixed systemd unit.
pub fn service_name(slot: DeploySlot) -> &'static str {
    match slot {
        DeploySlot::Blue => "pgpanel-blue.service",
        DeploySlot::Green => "pgpanel-green.service",
    }
}

fn systemctl_program() -> UpdaterResult<&'static str> {
    for candidate in ["/usr/bin/systemctl", "/bin/systemctl"] {
        if Path::new(candidate).is_file() {
            return Ok(candidate);
        }
    }
    Err(UpdaterError::Deploy(
        "systemctl not found at a fixed system path".into(),
    ))
}

fn run_systemctl(action: &str, slot: DeploySlot) -> UpdaterResult<()> {
    let output = Command::new(systemctl_program()?)
        .arg(action)
        .arg(service_name(slot))
        .output()
        .map_err(|error| {
            UpdaterError::Deploy(format!(
                "failed to execute systemctl {action} {}: {error}",
                service_name(slot)
            ))
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(UpdaterError::Deploy(format!(
            "systemctl {action} {} failed (exit {}): {}",
            service_name(slot),
            output.status.code().unwrap_or(-1),
            stderr.trim()
        )));
    }
    Ok(())
}

// The helper is deliberately not restarted here. Its lifecycle is independent
// of slot flips; a future helper restart must be a separate guarded operation.
fn start_slot_service(slot: DeploySlot) -> UpdaterResult<()> {
    run_systemctl("start", slot)
}

fn restart_slot_service(slot: DeploySlot) -> UpdaterResult<()> {
    run_systemctl("restart", slot)
}

fn stop_slot_service(slot: DeploySlot) -> UpdaterResult<()> {
    run_systemctl("stop", slot)
}

fn sqlite_program() -> UpdaterResult<&'static str> {
    for candidate in ["/usr/bin/sqlite3", "/bin/sqlite3"] {
        if Path::new(candidate).is_file() {
            return Ok(candidate);
        }
    }
    Err(UpdaterError::Deploy(
        "sqlite3 not found at a fixed system path".into(),
    ))
}

fn remove_symlink(link: &Path) -> UpdaterResult<()> {
    match fs::symlink_metadata(link) {
        Ok(_) => fs::remove_file(link).map_err(UpdaterError::Io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn restore_link(link: &Path, prior_target: Option<&Path>) -> UpdaterResult<()> {
    match prior_target {
        Some(target) => atomic_symlink(target, link),
        None => remove_symlink(link),
    }
}

/// Atomically replace a symlink.
pub fn atomic_symlink(target: &Path, link: &Path) -> UpdaterResult<()> {
    let parent = link
        .parent()
        .ok_or_else(|| UpdaterError::Deploy("symlink has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.symlink.tmp",
        link.file_name().unwrap_or_default().to_string_lossy()
    ));
    if tmp.symlink_metadata().is_ok() {
        fs::remove_file(&tmp)?;
    }
    std::os::unix::fs::symlink(target, &tmp)?;
    // rename(2) replaces the old symlink in one operation; do not unlink it
    // first, since that would leave systemd with a transient missing path.
    fs::rename(&tmp, link)?;
    Ok(())
}

fn timestamp_secs() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn is_safe_version_component(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 128
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::{NullProgress, ProgressReporter};
    use tempfile::TempDir;

    #[test]
    fn compare_versions_orders_correctly() {
        assert_eq!(
            compare_versions("1.2.0", "1.1.9"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(compare_versions("1.0.0", "2.0.0"), std::cmp::Ordering::Less);
        assert_eq!(
            compare_versions("v1.0.0", "1.0.0"),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn is_newer_version_works() {
        assert!(is_newer_version("1.1.0", "1.0.0"));
        assert!(!is_newer_version("1.0.0", "1.0.0"));
        assert!(!is_newer_version("0.9.0", "1.0.0"));
    }

    #[test]
    fn atomic_symlink_creates_link() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("releases").join("1.0.0");
        fs::create_dir_all(&target).unwrap();
        let link = dir.path().join("current");
        atomic_symlink(&target, &link).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn rollback_requires_previous_link() {
        let dir = TempDir::new().unwrap();
        let mut config = pgpanel_core::config::Config::dev_default();
        config.updates.install_root = dir.path().to_path_buf();
        let mgr = DeployManager::new(config);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(mgr.rollback(&mut ProgressReporter::new(NullProgress)))
            .unwrap_err();
        assert!(matches!(err, UpdaterError::Rollback(_)));
    }

    #[test]
    fn deploy_slot_opposite() {
        assert_eq!(DeploySlot::Blue.opposite(), DeploySlot::Green);
        assert_eq!(DeploySlot::Green.opposite(), DeploySlot::Blue);
    }

    #[test]
    fn service_name_maps_slots_without_systemctl() {
        assert_eq!(service_name(DeploySlot::Blue), "pgpanel-blue.service");
        assert_eq!(service_name(DeploySlot::Green), "pgpanel-green.service");
    }

    #[test]
    fn slot_symlink_update_and_restore() {
        let dir = TempDir::new().unwrap();
        let mut config = pgpanel_core::config::Config::dev_default();
        config.updates.install_root = dir.path().to_path_buf();
        let manager = DeployManager::new(config);
        let old = manager.release_dir("1.0.0");
        let new = manager.release_dir("2.0.0");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        let link = manager.slot_current_link(DeploySlot::Green);
        atomic_symlink(&old, &link).unwrap();

        let prior = manager
            .update_slot_symlink(DeploySlot::Green, "2.0.0")
            .unwrap();
        assert_eq!(prior, Some(old.clone()));
        assert_eq!(fs::read_link(&link).unwrap(), new);

        manager
            .restore_slot_symlink(DeploySlot::Green, prior.as_deref())
            .unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), old);
    }
}
