-- Pending TOTP enrollments are session-bound and expire quickly.
CREATE TABLE IF NOT EXISTS pending_totp_enrollments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    secret_encrypted TEXT NOT NULL,
    otpauth_uri_encrypted TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    UNIQUE(user_id, session_id)
);
CREATE INDEX IF NOT EXISTS idx_pending_totp_expiry
    ON pending_totp_enrollments(expires_at);
