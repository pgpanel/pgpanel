//! Update management via the privileged updater daemon client.

use pgpanel_core::config::Config;
use pgpanel_core::version::VERSION;
use pgpanel_updater::protocol::{DaemonPhase, OperationOutcome, ProgressRecord, ReleaseMetadata};
use pgpanel_updater::{DeploySlot, DeployStatus, UpdaterClient, UpdaterError, UpdaterStatus};
use std::sync::Arc;
use std::time::Duration;

/// Safe release metadata for the UI.
#[derive(Debug, Clone)]
pub struct ReleaseView {
    pub version: String,
    pub tag: String,
    pub published_at: String,
}

/// Completed operation summary for the UI.
#[derive(Debug, Clone)]
pub struct OperationView {
    pub operation_id: u64,
    pub version: Option<String>,
    pub finished_at: String,
}

/// Bounded progress row for the UI (no raw detail payloads).
#[derive(Debug, Clone)]
pub struct ProgressRow {
    pub id: u64,
    pub operation_id: u64,
    pub phase: String,
    pub message: String,
    pub progress_pct: Option<u8>,
}

/// Page/fragment status model sourced from config + daemon.
#[derive(Debug, Clone)]
pub struct UpdatePageStatus {
    pub daemon_available: bool,
    pub daemon_message: Option<String>,
    pub installed_version: String,
    pub channel: String,
    pub auto_install: bool,
    pub check_interval_secs: u64,
    pub check_interval_display: String,
    pub schedule_window: Option<String>,
    pub phase: String,
    pub busy: bool,
    pub latest: Option<ReleaseView>,
    pub last_check: Option<String>,
    pub last_success: Option<OperationView>,
    pub last_error: Option<String>,
    pub active_slot: Option<String>,
    pub previous_slot: Option<String>,
    pub current_version: Option<String>,
    pub previous_version: Option<String>,
    pub progress: Vec<ProgressRow>,
}

/// Updates service wrapping [`UpdaterClient`].
#[derive(Clone)]
pub struct UpdatesService {
    config: Arc<Config>,
    client: UpdaterClient,
}

impl UpdatesService {
    /// Create a client bound to `config.paths.updater_socket`.
    pub fn new(config: Arc<Config>) -> Self {
        let timeout = Duration::from_secs(config.updates.timeout_secs.clamp(5, 120));
        let client = UpdaterClient::new(&config.paths.updater_socket).with_timeout(timeout);
        Self { config, client }
    }

    /// Config-derived defaults plus daemon status when reachable.
    pub async fn status(&self) -> UpdatePageStatus {
        match self.client.status().await {
            Ok(status) => self.map_status(status, true, None),
            Err(err) => self.map_status(
                empty_daemon_status(),
                false,
                Some(safe_daemon_message(&err)),
            ),
        }
    }

    /// Ask the daemon to check for releases; returns refreshed status.
    pub async fn check(&self) -> Result<UpdatePageStatus, String> {
        match self.client.check().await {
            Ok(status) => Ok(self.map_status(status, true, None)),
            Err(err) => Err(safe_daemon_message(&err)),
        }
    }

    /// Start install of latest or a validated exact version. Returns operation id.
    pub async fn install(&self, version: Option<&str>) -> Result<u64, String> {
        match version {
            Some(v) => {
                let trimmed = v.trim();
                if trimmed.is_empty() {
                    return self.install_latest().await;
                }
                if !valid_version_request(trimmed) {
                    return Err("version must be a safe semver-like value".into());
                }
                self.client
                    .install_version(trimmed)
                    .await
                    .map_err(|e| safe_daemon_message(&e))
            }
            None => self.install_latest().await,
        }
    }

    async fn install_latest(&self) -> Result<u64, String> {
        self.client
            .install_latest()
            .await
            .map_err(|e| safe_daemon_message(&e))
    }

    /// Start rollback. Returns operation id.
    pub async fn rollback(&self) -> Result<u64, String> {
        self.client
            .rollback()
            .await
            .map_err(|e| safe_daemon_message(&e))
    }

    fn map_status(
        &self,
        status: UpdaterStatus,
        daemon_available: bool,
        daemon_message: Option<String>,
    ) -> UpdatePageStatus {
        let busy = !matches!(status.phase, DaemonPhase::Idle);
        let (active_slot, previous_slot, current_version, previous_version) =
            map_deployment(status.deployment.as_ref());

        UpdatePageStatus {
            daemon_available,
            daemon_message,
            installed_version: VERSION.to_string(),
            channel: self.config.updates.channel.clone(),
            auto_install: self.config.updates.auto_install,
            check_interval_secs: self.config.updates.check_interval_secs,
            check_interval_display: format_interval(self.config.updates.check_interval_secs),
            schedule_window: self.config.updates.schedule_window.clone(),
            phase: phase_label(status.phase),
            busy,
            latest: status.latest_available.as_ref().map(map_release),
            last_check: status.last_check.map(format_unix),
            last_success: status.last_success.as_ref().map(map_operation),
            last_error: status.last_error.as_deref().map(sanitize_user_message),
            active_slot,
            previous_slot,
            current_version,
            previous_version,
            progress: status.progress.iter().map(map_progress).collect(),
        }
    }
}

fn empty_daemon_status() -> UpdaterStatus {
    UpdaterStatus {
        phase: DaemonPhase::Idle,
        latest_available: None,
        last_check: None,
        last_success: None,
        last_error: None,
        progress: Vec::new(),
        deployment: None,
    }
}

fn map_release(meta: &ReleaseMetadata) -> ReleaseView {
    ReleaseView {
        version: meta.version.clone(),
        tag: meta.tag.clone(),
        published_at: meta.published_at.clone(),
    }
}

fn map_operation(outcome: &OperationOutcome) -> OperationView {
    OperationView {
        operation_id: outcome.operation_id,
        version: outcome.version.clone(),
        finished_at: format_unix(outcome.finished_at),
    }
}

fn map_progress(record: &ProgressRecord) -> ProgressRow {
    use pgpanel_updater::progress::UpdatePhase;
    let phase = match record.phase {
        UpdatePhase::Locking => "locking",
        UpdatePhase::CheckingRelease => "checking_release",
        UpdatePhase::Downloading => "downloading",
        UpdatePhase::Verifying => "verifying",
        UpdatePhase::ValidatingManifest => "validating_manifest",
        UpdatePhase::Extracting => "extracting",
        UpdatePhase::SettingPermissions => "setting_permissions",
        UpdatePhase::BackingUpDatabase => "backing_up_database",
        UpdatePhase::StartingSlot => "starting_slot",
        UpdatePhase::HealthCheck => "health_check",
        UpdatePhase::SmokeTest => "smoke_test",
        UpdatePhase::SwitchingTraffic => "switching_traffic",
        UpdatePhase::Draining => "draining",
        UpdatePhase::StoppingOldSlot => "stopping_old_slot",
        UpdatePhase::Finalizing => "finalizing",
        UpdatePhase::Complete => "complete",
        UpdatePhase::Rollback => "rollback",
        UpdatePhase::Failed => "failed",
    };
    ProgressRow {
        id: record.id,
        operation_id: record.operation_id,
        phase: phase.into(),
        message: record.message.clone(),
        progress_pct: record
            .progress
            .map(|p| ((p.clamp(0.0, 1.0)) * 100.0).round() as u8),
    }
}

fn map_deployment(
    deployment: Option<&DeployStatus>,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let Some(d) = deployment else {
        return (None, None, None, None);
    };
    (
        Some(d.active_slot.as_str().to_string()),
        Some(opposite_slot(d.active_slot).as_str().to_string()),
        d.current_version.clone(),
        d.previous_version.clone(),
    )
}

fn opposite_slot(slot: DeploySlot) -> DeploySlot {
    slot.opposite()
}

fn phase_label(phase: DaemonPhase) -> String {
    match phase {
        DaemonPhase::Idle => "idle".into(),
        DaemonPhase::Checking => "checking".into(),
        DaemonPhase::Installing => "installing".into(),
        DaemonPhase::RollingBack => "rolling_back".into(),
    }
}

fn format_unix(ts: u64) -> String {
    use chrono::{TimeZone, Utc};
    match Utc.timestamp_opt(ts as i64, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        _ => ts.to_string(),
    }
}

fn format_interval(secs: u64) -> String {
    if secs >= 86_400 && secs % 86_400 == 0 {
        let days = secs / 86_400;
        if days == 1 {
            "every day".into()
        } else {
            format!("every {days} days")
        }
    } else if secs >= 3_600 && secs % 3_600 == 0 {
        let hours = secs / 3_600;
        if hours == 1 {
            "every hour".into()
        } else {
            format!("every {hours} hours")
        }
    } else if secs >= 60 && secs % 60 == 0 {
        let mins = secs / 60;
        format!("every {mins} minutes")
    } else {
        format!("every {secs} seconds")
    }
}

/// Mirror daemon `valid_version_request` without exposing internals.
pub fn valid_version_request(version: &str) -> bool {
    let value = version.strip_prefix('v').unwrap_or(version);
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

fn safe_daemon_message(err: &UpdaterError) -> String {
    match err {
        UpdaterError::Protocol(msg) => sanitize_user_message(msg),
        UpdaterError::Io(_) => "Updater daemon is unavailable".into(),
        UpdaterError::Network(_) => "Update check failed due to a network error".into(),
        UpdaterError::Release(msg) | UpdaterError::Version(msg) => sanitize_user_message(msg),
        UpdaterError::Lock(_) => "Another update operation is already in progress".into(),
        _ => "Updater request failed".into(),
    }
}

fn sanitize_user_message(msg: &str) -> String {
    let trimmed = msg.trim();
    if trimmed.is_empty() {
        return "Updater request failed".into();
    }
    // Drop anything that looks like an absolute path or socket path.
    if trimmed.contains("/run/")
        || trimmed.contains("/opt/")
        || trimmed.contains("/etc/")
        || trimmed.contains("/var/")
        || trimmed.contains(".sock")
    {
        return "Updater request failed".into();
    }
    trimmed.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgpanel_updater::progress::UpdatePhase;

    #[test]
    fn rejects_unsafe_versions() {
        assert!(!valid_version_request(""));
        assert!(!valid_version_request("../etc/passwd"));
        assert!(!valid_version_request("1.0.0;rm"));
        assert!(!valid_version_request(&"a".repeat(200)));
    }

    #[test]
    fn accepts_semver_like_versions() {
        assert!(valid_version_request("1.2.3"));
        assert!(valid_version_request("v0.2.0"));
        assert!(valid_version_request("1.0.0-rc.1"));
        assert!(valid_version_request("1.0.0+build.1"));
    }

    #[test]
    fn maps_idle_status_from_daemon() {
        let config = Arc::new(Config::dev_default());
        let service = UpdatesService::new(config);
        let status = service.map_status(
            UpdaterStatus {
                phase: DaemonPhase::Installing,
                latest_available: Some(ReleaseMetadata {
                    version: "1.2.3".into(),
                    tag: "v1.2.3".into(),
                    published_at: "2026-01-01T00:00:00Z".into(),
                }),
                last_check: Some(1_700_000_000),
                last_success: Some(OperationOutcome {
                    operation_id: 9,
                    version: Some("1.2.2".into()),
                    finished_at: 1_700_000_100,
                }),
                last_error: None,
                progress: vec![ProgressRecord {
                    id: 1,
                    operation_id: 9,
                    phase: UpdatePhase::Downloading,
                    message: "Downloading".into(),
                    detail: Some(serde_json::json!({"path": "/opt/secret"})),
                    progress: Some(0.5),
                }],
                deployment: Some(DeployStatus {
                    active_slot: DeploySlot::Blue,
                    current_version: Some("1.2.2".into()),
                    previous_version: Some("1.2.1".into()),
                    installed_versions: vec!["1.2.1".into(), "1.2.2".into()],
                    update_in_progress: true,
                }),
            },
            true,
            None,
        );

        assert!(status.daemon_available);
        assert!(status.busy);
        assert_eq!(status.phase, "installing");
        assert_eq!(status.active_slot.as_deref(), Some("blue"));
        assert_eq!(status.previous_slot.as_deref(), Some("green"));
        assert_eq!(status.latest.as_ref().unwrap().tag, "v1.2.3");
        assert_eq!(status.progress.len(), 1);
        assert_eq!(status.progress[0].progress_pct, Some(50));
        assert!(!status.progress[0].message.contains("/opt/"));
    }

    #[test]
    fn unavailable_status_preserves_config_channel() {
        let config = Arc::new(Config::dev_default());
        let channel = config.updates.channel.clone();
        let service = UpdatesService::new(config);
        let status = service.map_status(
            empty_daemon_status(),
            false,
            Some("Updater daemon is unavailable".into()),
        );
        assert!(!status.daemon_available);
        assert_eq!(status.channel, channel);
        assert_eq!(
            status.daemon_message.as_deref(),
            Some("Updater daemon is unavailable")
        );
        assert!(!status.busy);
    }

    #[test]
    fn sanitizes_path_bearing_errors() {
        let msg = safe_daemon_message(&UpdaterError::Protocol(
            "connect /run/pgpanel/updater.sock failed".into(),
        ));
        assert_eq!(msg, "Updater request failed");
        assert_eq!(
            safe_daemon_message(&UpdaterError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "refused"
            ))),
            "Updater daemon is unavailable"
        );
    }
}
