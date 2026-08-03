//! Progress events for SSE / UI consumption.

use serde::{Deserialize, Serialize};

/// High-level update phase for progress reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    /// Acquiring global update lock.
    Locking,
    /// Querying GitHub for releases.
    CheckingRelease,
    /// Downloading release artifacts.
    Downloading,
    /// Verifying checksums and signatures.
    Verifying,
    /// Validating release manifest.
    ValidatingManifest,
    /// Extracting archive securely.
    Extracting,
    /// Setting file permissions.
    SettingPermissions,
    /// Backing up SQLite database.
    BackingUpDatabase,
    /// Starting the inactive deployment slot.
    StartingSlot,
    /// Running health checks on inactive slot.
    HealthCheck,
    /// Running smoke test requests.
    SmokeTest,
    /// Switching Caddy upstream.
    SwitchingTraffic,
    /// Draining connections from old slot.
    Draining,
    /// Stopping the old deployment slot.
    StoppingOldSlot,
    /// Finalizing symlinks and state.
    Finalizing,
    /// Update completed successfully.
    Complete,
    /// Rolling back after failure.
    Rollback,
    /// Update failed.
    Failed,
}

/// Structured progress event emitted during update / rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    /// Monotonic sequence number within a single operation.
    pub seq: u64,
    /// Current phase.
    pub phase: UpdatePhase,
    /// Human-readable message.
    pub message: String,
    /// Optional structured detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
    /// Progress fraction 0.0–1.0 when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
}

impl ProgressEvent {
    /// Create a new progress event.
    pub fn new(seq: u64, phase: UpdatePhase, message: impl Into<String>) -> Self {
        Self {
            seq,
            phase,
            message: message.into(),
            detail: None,
            progress: None,
        }
    }

    /// Attach optional JSON detail.
    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }

    /// Attach optional progress fraction.
    pub fn with_progress(mut self, progress: f64) -> Self {
        self.progress = Some(progress.clamp(0.0, 1.0));
        self
    }

    /// Serialize to SSE `data:` line payload.
    pub fn to_sse_data(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            format!(
                r#"{{"seq":{},"phase":"failed","message":"failed to serialize progress"}}"#,
                self.seq
            )
        })
    }
}

/// Callback sink for progress events.
pub trait ProgressSink: Send + Sync {
    /// Emit a progress event.
    fn emit(&mut self, event: ProgressEvent);
}

/// No-op progress sink.
#[derive(Debug, Default)]
pub struct NullProgress;

impl ProgressSink for NullProgress {
    fn emit(&mut self, _event: ProgressEvent) {}
}

/// Channel-based progress sink for async consumers.
#[derive(Debug)]
pub struct ChannelProgress {
    tx: tokio::sync::mpsc::UnboundedSender<ProgressEvent>,
}

impl ChannelProgress {
    /// Create a channel progress sink and receiver.
    pub fn pair() -> (Self, tokio::sync::mpsc::UnboundedReceiver<ProgressEvent>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Self { tx }, rx)
    }
}

impl ProgressSink for ChannelProgress {
    fn emit(&mut self, event: ProgressEvent) {
        let _ = self.tx.send(event);
    }
}

/// Helper that tracks sequence numbers.
#[derive(Debug)]
pub struct ProgressReporter<S: ProgressSink> {
    seq: u64,
    sink: S,
}

impl<S: ProgressSink> ProgressReporter<S> {
    /// Wrap a sink with automatic sequence numbering.
    pub fn new(sink: S) -> Self {
        Self { seq: 0, sink }
    }

    /// Emit an event, assigning the next sequence number.
    pub fn emit(&mut self, phase: UpdatePhase, message: impl Into<String>) {
        self.seq += 1;
        self.sink.emit(ProgressEvent::new(self.seq, phase, message));
    }

    /// Emit with detail.
    pub fn emit_detail(
        &mut self,
        phase: UpdatePhase,
        message: impl Into<String>,
        detail: serde_json::Value,
    ) {
        self.seq += 1;
        self.sink
            .emit(ProgressEvent::new(self.seq, phase, message).with_detail(detail));
    }

    /// Emit with progress fraction.
    pub fn emit_progress(&mut self, phase: UpdatePhase, message: impl Into<String>, progress: f64) {
        self.seq += 1;
        self.sink
            .emit(ProgressEvent::new(self.seq, phase, message).with_progress(progress));
    }

    /// Borrow the inner sink.
    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_event_serializes() {
        let event = ProgressEvent::new(1, UpdatePhase::Downloading, "downloading release")
            .with_progress(0.5);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"phase\":\"downloading\""));
        assert!(json.contains("\"progress\":0.5"));
    }

    #[test]
    fn sse_data_is_valid_json() {
        let event = ProgressEvent::new(2, UpdatePhase::Complete, "done");
        let data = event.to_sse_data();
        let _: serde_json::Value = serde_json::from_str(&data).unwrap();
    }
}
