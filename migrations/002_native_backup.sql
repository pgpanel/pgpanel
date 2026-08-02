-- Native backup engine (replaces Databasus integration)

CREATE TABLE IF NOT EXISTS backups (
    id TEXT PRIMARY KEY NOT NULL,
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    kind TEXT NOT NULL DEFAULT 'logical_full',
    status TEXT NOT NULL DEFAULT 'pending',
    database_name TEXT NOT NULL DEFAULT 'postgres',
    storage_key TEXT,
    size_bytes INTEGER,
    checksum_sha256 TEXT,
    encrypted INTEGER NOT NULL DEFAULT 1,
    error TEXT,
    started_at TEXT,
    finished_at TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_backups_cluster ON backups(cluster_id);
CREATE INDEX IF NOT EXISTS idx_backups_status ON backups(status);
CREATE INDEX IF NOT EXISTS idx_backups_created ON backups(created_at);

CREATE TABLE IF NOT EXISTS backup_schedules (
    id TEXT PRIMARY KEY NOT NULL,
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    cron TEXT NOT NULL DEFAULT '0 3 * * *',
    kind TEXT NOT NULL DEFAULT 'logical_full',
    database_name TEXT NOT NULL DEFAULT 'postgres',
    enabled INTEGER NOT NULL DEFAULT 1,
    retention_days INTEGER NOT NULL DEFAULT 14,
    keep_count INTEGER NOT NULL DEFAULT 30,
    last_run_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(cluster_id, database_name, kind)
);

-- Cluster metrics samples (traffic / resource monitoring)
CREATE TABLE IF NOT EXISTS cluster_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    cpu_percent REAL NOT NULL DEFAULT 0,
    memory_usage_mb REAL NOT NULL DEFAULT 0,
    memory_limit_mb REAL NOT NULL DEFAULT 0,
    network_rx_bytes INTEGER,
    network_tx_bytes INTEGER,
    collected_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_metrics_cluster_time ON cluster_metrics(cluster_id, collected_at);

INSERT OR IGNORE INTO settings (key, value, updated_at)
VALUES ('wizard_completed', 'false', datetime('now'));

INSERT OR IGNORE INTO settings (key, value, updated_at)
VALUES ('backup_engine', 'native', datetime('now'));
