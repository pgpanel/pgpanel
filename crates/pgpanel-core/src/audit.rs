//! Audit log helpers and chained hash computation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Audit event ready for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Timestamp.
    pub at: DateTime<Utc>,
    /// Actor user id if known.
    pub user_id: Option<i64>,
    /// Actor username.
    pub username: Option<String>,
    /// Source IP.
    pub source_ip: Option<String>,
    /// Action name, e.g. `cluster.delete`.
    pub action: String,
    /// Target descriptor.
    pub target: Option<String>,
    /// Result: success | failure.
    pub result: String,
    /// Request ID.
    pub request_id: String,
    /// Safe before metadata JSON.
    pub before_meta: Option<serde_json::Value>,
    /// Safe after metadata JSON.
    pub after_meta: Option<serde_json::Value>,
    /// Failure reason (safe).
    pub failure_reason: Option<String>,
}

impl AuditEvent {
    /// Create a success event.
    pub fn success(
        action: impl Into<String>,
        user_id: Option<i64>,
        username: Option<String>,
        source_ip: Option<String>,
        request_id: impl Into<String>,
        target: Option<String>,
        after_meta: Option<serde_json::Value>,
    ) -> Self {
        Self {
            at: Utc::now(),
            user_id,
            username,
            source_ip,
            action: action.into(),
            target,
            result: "success".into(),
            request_id: request_id.into(),
            before_meta: None,
            after_meta,
            failure_reason: None,
        }
    }

    /// Create a failure event.
    pub fn failure(
        action: impl Into<String>,
        user_id: Option<i64>,
        username: Option<String>,
        source_ip: Option<String>,
        request_id: impl Into<String>,
        target: Option<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            at: Utc::now(),
            user_id,
            username,
            source_ip,
            action: action.into(),
            target,
            result: "failure".into(),
            request_id: request_id.into(),
            before_meta: None,
            after_meta: None,
            failure_reason: Some(reason.into()),
        }
    }
}

/// Compute chained hash: sha256(prev_hash || canonical_payload).
pub fn chain_hash(prev_hash: &str, event: &AuditEvent) -> String {
    let payload = serde_json::json!({
        "at": event.at.to_rfc3339(),
        "user_id": event.user_id,
        "username": event.username,
        "source_ip": event.source_ip,
        "action": event.action,
        "target": event.target,
        "result": event.result,
        "request_id": event.request_id,
        "before_meta": event.before_meta,
        "after_meta": event.after_meta,
        "failure_reason": event.failure_reason,
    });
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(payload.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

/// Genesis previous hash (64 zero hex chars).
pub const AUDIT_GENESIS: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_changes_with_content() {
        let e1 = AuditEvent::success("test", None, None, None, "r1", None, None);
        let h1 = chain_hash(AUDIT_GENESIS, &e1);
        let e2 = AuditEvent::success("test2", None, None, None, "r2", None, None);
        let h2 = chain_hash(&h1, &e2);
        assert_ne!(h1, h2);
        assert_eq!(h1.len(), 64);
    }
}
