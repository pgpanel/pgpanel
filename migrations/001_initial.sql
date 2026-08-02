-- PgPanel panel database (SQLite, WAL mode set at runtime)

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY NOT NULL,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    failed_login_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until TEXT,
    created_at TEXT NOT NULL,
    last_login_at TEXT
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    csrf_token TEXT NOT NULL,
    ip_address TEXT,
    user_agent TEXT,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_token ON sessions(token_hash);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);

CREATE TABLE IF NOT EXISTS clusters (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    postgres_version TEXT NOT NULL,
    docker_container_id TEXT,
    docker_container_name TEXT NOT NULL UNIQUE,
    docker_volume_name TEXT NOT NULL UNIQUE,
    docker_network_name TEXT NOT NULL UNIQUE,
    internal_hostname TEXT NOT NULL,
    public_port INTEGER,
    cpu_limit REAL NOT NULL,
    memory_mb INTEGER NOT NULL,
    storage_limit_gb INTEGER NOT NULL,
    status TEXT NOT NULL,
    health TEXT NOT NULL DEFAULT 'unknown',
    databasus_status TEXT NOT NULL DEFAULT 'not_configured',
    delete_protection INTEGER NOT NULL DEFAULT 0,
    enable_backup INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_clusters_status ON clusters(status);

CREATE TABLE IF NOT EXISTS cluster_credentials (
    id TEXT PRIMARY KEY NOT NULL,
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    role_name TEXT NOT NULL,
    username TEXT NOT NULL,
    password_encrypted TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'admin',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(cluster_id, role_name)
);

CREATE TABLE IF NOT EXISTS databases (
    id TEXT PRIMARY KEY NOT NULL,
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    owner_role TEXT NOT NULL,
    connection_limit INTEGER,
    created_at TEXT NOT NULL,
    UNIQUE(cluster_id, name)
);

CREATE TABLE IF NOT EXISTS database_roles (
    id TEXT PRIMARY KEY NOT NULL,
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    is_superuser INTEGER NOT NULL DEFAULT 0,
    can_login INTEGER NOT NULL DEFAULT 1,
    connection_limit INTEGER,
    created_at TEXT NOT NULL,
    UNIQUE(cluster_id, name)
);

CREATE TABLE IF NOT EXISTS backup_integrations (
    id TEXT PRIMARY KEY NOT NULL,
    cluster_id TEXT NOT NULL UNIQUE REFERENCES clusters(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    external_id TEXT,
    last_successful_backup TEXT,
    last_backup_status TEXT,
    backup_lag_seconds INTEGER,
    wal_status TEXT,
    failed_backups INTEGER NOT NULL DEFAULT 0,
    message TEXT,
    manual_setup_info TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS operations (
    id TEXT PRIMARY KEY NOT NULL,
    job_type TEXT NOT NULL,
    status TEXT NOT NULL,
    cluster_id TEXT,
    payload TEXT NOT NULL DEFAULT '{}',
    result TEXT,
    error TEXT,
    progress INTEGER NOT NULL DEFAULT 0,
    idempotency_key TEXT UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_operations_status ON operations(status);
CREATE INDEX IF NOT EXISTS idx_operations_cluster ON operations(cluster_id);

CREATE TABLE IF NOT EXISTS operation_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL REFERENCES operations(id) ON DELETE CASCADE,
    level TEXT NOT NULL DEFAULT 'info',
    message TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_operation_logs_op ON operation_logs(operation_id);

CREATE TABLE IF NOT EXISTS audit_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_id TEXT,
    actor_username TEXT,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    details TEXT NOT NULL DEFAULT '{}',
    ip_address TEXT,
    correlation_id TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_logs(action);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO settings (key, value, updated_at)
VALUES ('bootstrap_completed', 'false', datetime('now'));
