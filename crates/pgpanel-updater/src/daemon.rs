//! Privileged updater daemon and its Unix-socket server.

#![cfg(unix)]

use crate::error::{UpdaterError, UpdaterResult};
use crate::peer::{is_peer_allowed, peer_credentials};
use crate::progress::{ProgressEvent, ProgressReporter, ProgressSink, UpdatePhase};
use crate::protocol::{
    DaemonPhase, OperationOutcome, ProgressRecord, ReleaseMetadata, UpdaterRequest,
    UpdaterResponse, UpdaterStatus, MAX_FRAME_BYTES,
};
use crate::{TargetArch, Updater};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tracing::{debug, warn};

const MAX_PROGRESS_EVENTS: usize = 100;

/// Root daemon serving requests from the web process.
#[derive(Debug)]
pub struct UpdaterDaemon {
    updater: Arc<Updater>,
    state: Arc<Mutex<UpdaterStatus>>,
    operation: Arc<AsyncMutex<()>>,
    next_operation: Arc<AtomicU64>,
    next_event: Arc<AtomicU64>,
    state_path: PathBuf,
    arch: TargetArch,
}

impl UpdaterDaemon {
    /// Construct a daemon and load its non-secret status if available.
    pub fn new(updater: Updater, arch: TargetArch) -> UpdaterResult<Arc<Self>> {
        let state_path = state_path(updater.config());
        let mut status = load_status(&state_path).unwrap_or_else(default_status);
        status.phase = DaemonPhase::Idle;
        status.deployment = updater.status().ok();
        let next_event = status
            .progress
            .iter()
            .map(|event| event.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let next_operation = status
            .last_success
            .as_ref()
            .map(|result| result.operation_id.saturating_add(1))
            .unwrap_or(1)
            .max(
                status
                    .progress
                    .iter()
                    .map(|event| event.operation_id.saturating_add(1))
                    .max()
                    .unwrap_or(1),
            );
        let daemon = Arc::new(Self {
            updater: Arc::new(updater),
            state: Arc::new(Mutex::new(status)),
            operation: Arc::new(AsyncMutex::new(())),
            next_operation: Arc::new(AtomicU64::new(next_operation)),
            next_event: Arc::new(AtomicU64::new(next_event)),
            state_path,
            arch,
        });
        daemon.persist();
        Ok(daemon)
    }

    /// Return a status snapshot, including the current deployment.
    pub fn status(&self) -> UpdaterStatus {
        let deployment = self.updater.status().ok();
        let mut status = self
            .state
            .lock()
            .expect("daemon state lock poisoned")
            .clone();
        status.deployment = deployment;
        status
    }

    /// Start the Unix socket server.
    pub async fn serve(
        self: Arc<Self>,
        socket_path: &Path,
        allowed_uid: u32,
        dev_mode: bool,
    ) -> UpdaterResult<()> {
        prepare_socket(socket_path)?;
        let listener = UnixListener::bind(socket_path)?;
        fs::set_permissions(socket_path, fs::Permissions::from_mode(0o660))?;

        let automatic = self.clone();
        tokio::spawn(async move {
            automatic.automatic_loop().await;
        });

        loop {
            tokio::select! {
                result = listener.accept() => {
                    let (stream, _) = result?;
                    let daemon = self.clone();
                    tokio::spawn(async move {
                        if let Err(error) = daemon.handle_connection(stream, allowed_uid, dev_mode).await {
                            debug!(%error, "updater client connection failed");
                        }
                    });
                }
                _ = tokio::signal::ctrl_c() => {
                    debug!("updater daemon shutting down");
                    return Ok(());
                }
            }
        }
    }

    async fn handle_connection(
        &self,
        stream: UnixStream,
        allowed_uid: u32,
        dev_mode: bool,
    ) -> UpdaterResult<()> {
        let credentials = peer_credentials(&stream)?;
        if !is_peer_allowed(credentials.uid, allowed_uid, dev_mode) {
            return write_response(
                stream,
                UpdaterResponse::Error {
                    message: "peer credentials are not permitted".into(),
                },
            )
            .await;
        }

        let mut reader = BufReader::new(stream);
        let mut frame = Vec::new();
        reader.read_until(b'\n', &mut frame).await?;
        if frame.is_empty() {
            return Ok(());
        }
        if frame.len() > MAX_FRAME_BYTES {
            return write_response(
                reader.into_inner(),
                UpdaterResponse::Error {
                    message: "request frame too large".into(),
                },
            )
            .await;
        }
        let request = match serde_json::from_slice(frame.trim_ascii()) {
            Ok(request) => request,
            Err(error) => {
                return write_response(
                    reader.into_inner(),
                    UpdaterResponse::Error {
                        message: format!("invalid request: {error}"),
                    },
                )
                .await;
            }
        };
        let response = self.dispatch(request).await;
        write_response(reader.into_inner(), response).await
    }

    async fn dispatch(&self, request: UpdaterRequest) -> UpdaterResponse {
        match request {
            UpdaterRequest::Status => UpdaterResponse::Status {
                status: self.status(),
            },
            UpdaterRequest::Check => match self.check_once().await {
                Ok(latest) => UpdaterResponse::Check {
                    latest: latest.as_ref().map(ReleaseMetadata::from),
                    status: self.status(),
                },
                Err(error) => UpdaterResponse::Error {
                    message: error.to_string(),
                },
            },
            UpdaterRequest::InstallLatest => self.start_mutation(DaemonPhase::Installing, None),
            UpdaterRequest::InstallVersion { version } => {
                if !valid_version_request(&version) {
                    UpdaterResponse::Error {
                        message: "version must be a safe semver-like value".into(),
                    }
                } else {
                    self.start_mutation(DaemonPhase::Installing, Some(version))
                }
            }
            UpdaterRequest::Rollback => self.start_mutation(DaemonPhase::RollingBack, None),
        }
    }

    async fn check_once(&self) -> UpdaterResult<Option<crate::github::ReleaseInfo>> {
        let _guard = self.try_operation()?;
        self.set_phase(DaemonPhase::Checking);
        let result = self.updater.check(self.arch).await;
        let now = now_secs();
        let mut status = self.state.lock().expect("daemon state lock poisoned");
        status.last_check = Some(now);
        status.phase = DaemonPhase::Idle;
        match &result {
            Ok(Some(info)) => {
                status.latest_available = Some(ReleaseMetadata::from(info));
                status.last_error = None;
            }
            Ok(None) => {
                status.latest_available = None;
                status.last_error = None;
            }
            Err(error) => status.last_error = Some(error.to_string()),
        }
        drop(status);
        self.persist();
        result
    }

    fn start_mutation(&self, phase: DaemonPhase, version: Option<String>) -> UpdaterResponse {
        let guard = match self.try_operation() {
            Ok(guard) => guard,
            Err(_) => {
                return UpdaterResponse::Error {
                    message: "another updater operation is already in progress".into(),
                }
            }
        };
        let operation_id = self.next_operation.fetch_add(1, Ordering::Relaxed);
        self.set_phase(phase);
        let daemon = Arc::new(self.clone_for_task());
        tokio::spawn(async move {
            daemon
                .run_mutation(operation_id, phase, version, guard)
                .await;
        });
        UpdaterResponse::Accepted {
            operation_id,
            status: self.status(),
        }
    }

    async fn run_mutation(
        &self,
        operation_id: u64,
        phase: DaemonPhase,
        version: Option<String>,
        _guard: OwnedMutexGuard<()>,
    ) {
        let sink = DaemonProgressSink {
            state: self.state.clone(),
            state_path: self.state_path.clone(),
            next_event: self.next_event.clone(),
            operation_id,
        };
        let mut reporter = ProgressReporter::new(sink);
        let result = match phase {
            DaemonPhase::Installing => {
                self.updater
                    .update_with_progress(self.arch, version.as_deref(), false, &mut reporter)
                    .await
            }
            DaemonPhase::RollingBack => self.updater.rollback_with_progress(&mut reporter).await,
            DaemonPhase::Idle | DaemonPhase::Checking => {
                Err(UpdaterError::Config("invalid mutation phase".into()))
            }
        };
        if let Err(error) = &result {
            reporter.emit(UpdatePhase::Failed, error.to_string());
        }
        let mut status = self.state.lock().expect("daemon state lock poisoned");
        status.phase = DaemonPhase::Idle;
        match result {
            Ok(installed) => {
                status.last_success = Some(OperationOutcome {
                    operation_id,
                    version: Some(installed),
                    finished_at: now_secs(),
                });
                status.last_error = None;
            }
            Err(error) => status.last_error = Some(error.to_string()),
        }
        status.deployment = self.updater.status().ok();
        drop(status);
        self.persist();
    }

    async fn automatic_loop(&self) {
        let interval_secs = self.updater.config().updates.check_interval_secs.max(1);
        let mut interval = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(interval_secs),
            Duration::from_secs(interval_secs),
        );
        loop {
            interval.tick().await;
            let latest = match self.check_once().await {
                Ok(latest) => latest,
                Err(error) => {
                    warn!(%error, "automatic updater check failed");
                    continue;
                }
            };
            if !self.updater.config().updates.auto_install {
                continue;
            }
            let current = self.status().deployment.and_then(|d| d.current_version);
            if let Some(latest) = latest {
                if self.should_auto_install(&latest.version, current.as_deref()) {
                    let _ = self.start_mutation(DaemonPhase::Installing, None);
                }
            }
        }
    }

    fn should_auto_install(&self, latest: &str, current: Option<&str>) -> bool {
        self.updater.config().updates.auto_install
            && current.is_some_and(|current| crate::is_newer_version(latest, current))
    }

    fn try_operation(&self) -> UpdaterResult<OwnedMutexGuard<()>> {
        self.operation.clone().try_lock_owned().map_err(|_| {
            UpdaterError::Lock("another updater operation is already in progress".into())
        })
    }

    fn set_phase(&self, phase: DaemonPhase) {
        self.state.lock().expect("daemon state lock poisoned").phase = phase;
        self.persist();
    }

    fn persist(&self) {
        let status = self
            .state
            .lock()
            .expect("daemon state lock poisoned")
            .clone();
        if let Err(error) = persist_status(&self.state_path, &status) {
            debug!(%error, path = %self.state_path.display(), "could not persist updater status");
        }
    }

    fn clone_for_task(&self) -> Self {
        Self {
            updater: self.updater.clone(),
            state: self.state.clone(),
            operation: self.operation.clone(),
            next_operation: self.next_operation.clone(),
            next_event: self.next_event.clone(),
            state_path: self.state_path.clone(),
            arch: self.arch,
        }
    }
}

/// Progress sink that assigns daemon-wide event IDs.
struct DaemonProgressSink {
    state: Arc<Mutex<UpdaterStatus>>,
    state_path: PathBuf,
    next_event: Arc<AtomicU64>,
    operation_id: u64,
}

impl ProgressSink for DaemonProgressSink {
    fn emit(&mut self, event: ProgressEvent) {
        let id = self.next_event.fetch_add(1, Ordering::Relaxed);
        let record = ProgressRecord {
            id,
            operation_id: self.operation_id,
            phase: event.phase,
            message: event.message,
            detail: event.detail,
            progress: event.progress,
        };
        let status = {
            let mut status = self.state.lock().expect("daemon state lock poisoned");
            status.progress.push(record);
            if status.progress.len() > MAX_PROGRESS_EVENTS {
                let excess = status.progress.len() - MAX_PROGRESS_EVENTS;
                status.progress.drain(..excess);
            }
            status.clone()
        };
        if let Err(error) = persist_status(&self.state_path, &status) {
            debug!(%error, "could not persist updater progress");
        }
    }
}

async fn write_response(mut stream: UnixStream, response: UpdaterResponse) -> UpdaterResult<()> {
    let mut frame =
        serde_json::to_vec(&response).map_err(|error| UpdaterError::Protocol(error.to_string()))?;
    frame.push(b'\n');
    stream.write_all(&frame).await?;
    stream.shutdown().await?;
    Ok(())
}

fn prepare_socket(path: &Path) -> UpdaterResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_socket() {
            return Err(UpdaterError::Config(format!(
                "refusing to replace non-socket updater path: {}",
                path.display()
            )));
        }
        fs::remove_file(path)?;
    }
    Ok(())
}

fn default_status() -> UpdaterStatus {
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

fn state_path(config: &pgpanel_core::config::Config) -> PathBuf {
    let preferred = PathBuf::from("/var/lib/pgpanel/update-status.json");
    if preferred
        .parent()
        .map(|parent| fs::create_dir_all(parent).is_ok())
        .unwrap_or(false)
    {
        preferred
    } else {
        config.paths.state_dir.join("update-status.json")
    }
}

fn load_status(path: &Path) -> Option<UpdaterStatus> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn persist_status(path: &Path, status: &UpdaterStatus) -> UpdaterResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| UpdaterError::Config("status path has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile_path(path);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    let data = serde_json::to_vec_pretty(status)
        .map_err(|error| UpdaterError::Protocol(error.to_string()))?;
    file.write_all(&data)?;
    file.sync_all()?;
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o640))?;
    fs::rename(&temp, path)?;
    temp.clear();
    Ok(())
}

fn tempfile_path(path: &Path) -> PathBuf {
    path.with_extension(format!("tmp.{}", std::process::id()))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn valid_version_request(version: &str) -> bool {
    let value = version.strip_prefix('v').unwrap_or(version);
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgpanel_core::config::Config;

    #[tokio::test]
    async fn concurrent_operations_are_rejected() {
        let mut config = Config::dev_default();
        let dir = tempfile::tempdir().unwrap();
        config.paths.state_dir = dir.path().into();
        config.updates.install_root = dir.path().join("install");
        let daemon =
            UpdaterDaemon::new(Updater::new(config).unwrap(), TargetArch::detect()).unwrap();
        let _guard = daemon.try_operation().unwrap();
        assert!(daemon.try_operation().is_err());
        assert_eq!(daemon.status().phase, DaemonPhase::Idle);
    }

    #[test]
    fn persisted_status_is_bounded_and_non_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        let mut status = default_status();
        status.last_error = Some("safe error".into());
        persist_status(&path, &status).unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("safe error"));
        assert!(!text.contains("secret_key"));
    }

    #[test]
    fn auto_install_false_never_starts_from_a_newer_release() {
        let mut config = Config::dev_default();
        config.updates.auto_install = false;
        let daemon =
            UpdaterDaemon::new(Updater::new(config).unwrap(), TargetArch::detect()).unwrap();
        assert!(!daemon.should_auto_install("99.0.0", Some("1.0.0")));
    }

    #[test]
    fn version_requests_cannot_escape_release_directory() {
        assert!(valid_version_request("v1.2.3-rc.1"));
        assert!(!valid_version_request("../../etc"));
        assert!(!valid_version_request(""));
    }
}
