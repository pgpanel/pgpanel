-- Multi-admin RBAC, backup destinations, cluster replicas, monitoring alerts, API tokens

-- ── Users / RBAC ────────────────────────────────────────────────────────────

ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'admin';
-- owner | admin | operator | viewer
ALTER TABLE users ADD COLUMN display_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;
ALTER TABLE users ADD COLUMN must_change_password INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN created_by TEXT;
ALTER TABLE users ADD COLUMN updated_at TEXT;

UPDATE users SET role = 'owner' WHERE role = 'admin'
  AND id = (SELECT id FROM users ORDER BY created_at ASC LIMIT 1);

-- ── Named backup destinations (any cluster/node → any destination) ──────────

CREATE TABLE IF NOT EXISTS backup_destinations (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL UNIQUE,
    storage_type TEXT NOT NULL DEFAULT 'local', -- local|s3|r2|b2|minio|hetzner
    endpoint TEXT NOT NULL DEFAULT '',
    region TEXT NOT NULL DEFAULT 'auto',
    bucket TEXT NOT NULL DEFAULT '',
    prefix TEXT NOT NULL DEFAULT 'pgpanel/',
    path_style INTEGER NOT NULL DEFAULT 1,
    tls_verify INTEGER NOT NULL DEFAULT 1,
    access_key_encrypted TEXT,
    secret_key_encrypted TEXT,
    encrypt_backups INTEGER NOT NULL DEFAULT 1,
    compression_level INTEGER NOT NULL DEFAULT 6,
    enabled INTEGER NOT NULL DEFAULT 1,
    is_default INTEGER NOT NULL DEFAULT 0,
    notes TEXT,
    last_test_at TEXT,
    last_test_ok INTEGER,
    last_test_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Which nodes may use which destinations (empty = all nodes allowed)
CREATE TABLE IF NOT EXISTS node_backup_destinations (
    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    destination_id TEXT NOT NULL REFERENCES backup_destinations(id) ON DELETE CASCADE,
    prefer INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (node_id, destination_id)
);

-- Per-cluster multi-destination backup bindings
CREATE TABLE IF NOT EXISTS cluster_backup_targets (
    id TEXT PRIMARY KEY NOT NULL,
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    destination_id TEXT NOT NULL REFERENCES backup_destinations(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 100,
    include_databases TEXT NOT NULL DEFAULT '*', -- comma list or *
    exclude_databases TEXT NOT NULL DEFAULT '',
    cron TEXT NOT NULL DEFAULT '0 3 * * *',
    retention_days INTEGER NOT NULL DEFAULT 14,
    keep_count INTEGER NOT NULL DEFAULT 30,
    schema_only INTEGER NOT NULL DEFAULT 0,
    verify_after INTEGER NOT NULL DEFAULT 0,
    compression_level INTEGER,
    dump_format TEXT NOT NULL DEFAULT 'custom',
    exclude_schemas TEXT NOT NULL DEFAULT '',
    exclude_tables TEXT NOT NULL DEFAULT '',
    parallel_jobs INTEGER NOT NULL DEFAULT 1,
    notify_on_success INTEGER NOT NULL DEFAULT 0,
    notify_on_failure INTEGER NOT NULL DEFAULT 1,
    window_start_hour INTEGER,
    window_end_hour INTEGER,
    last_run_at TEXT,
    last_status TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(cluster_id, destination_id, include_databases)
);

ALTER TABLE backups ADD COLUMN destination_id TEXT REFERENCES backup_destinations(id);
ALTER TABLE backups ADD COLUMN source_node_id TEXT;

-- ── Cluster replicas / redundancy ───────────────────────────────────────────

CREATE TABLE IF NOT EXISTS cluster_replicas (
    id TEXT PRIMARY KEY NOT NULL,
    primary_cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    replica_cluster_id TEXT REFERENCES clusters(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'scheduled_sync', -- duplicate|scheduled_sync|streaming
    target_node_id TEXT NOT NULL REFERENCES nodes(id),
    sync_cron TEXT NOT NULL DEFAULT '*/30 * * * *',
    status TEXT NOT NULL DEFAULT 'pending', -- pending|syncing|healthy|lagging|failed|paused|promoting
    lag_seconds INTEGER,
    last_sync_at TEXT,
    last_error TEXT,
    auto_failover INTEGER NOT NULL DEFAULT 0,
    promote_protection INTEGER NOT NULL DEFAULT 1,
    config_json TEXT NOT NULL DEFAULT '{}',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_replicas_primary ON cluster_replicas(primary_cluster_id);
CREATE INDEX IF NOT EXISTS idx_replicas_status ON cluster_replicas(status);

-- ── Tags ────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS cluster_tags (
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (cluster_id, tag)
);

ALTER TABLE clusters ADD COLUMN environment TEXT NOT NULL DEFAULT 'production';
ALTER TABLE clusters ADD COLUMN description TEXT;
ALTER TABLE clusters ADD COLUMN maintenance_mode INTEGER NOT NULL DEFAULT 0;

-- ── Alerting ────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS alert_rules (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    enabled INTEGER NOT NULL DEFAULT 1,
    severity TEXT NOT NULL DEFAULT 'warning', -- info|warning|critical
    metric TEXT NOT NULL, -- cpu_percent|memory_percent|backup_age_hours|replica_lag|disk_percent|failed_backups
    operator TEXT NOT NULL DEFAULT 'gt', -- gt|gte|lt|lte|eq
    threshold REAL NOT NULL,
    duration_seconds INTEGER NOT NULL DEFAULT 300,
    scope TEXT NOT NULL DEFAULT 'all', -- all|cluster|node
    scope_id TEXT,
    notify_channels TEXT NOT NULL DEFAULT 'webhook',
    cooldown_seconds INTEGER NOT NULL DEFAULT 1800,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS alerts (
    id TEXT PRIMARY KEY NOT NULL,
    rule_id TEXT REFERENCES alert_rules(id) ON DELETE SET NULL,
    severity TEXT NOT NULL,
    title TEXT NOT NULL,
    message TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    status TEXT NOT NULL DEFAULT 'open', -- open|acked|resolved
    fired_at TEXT NOT NULL,
    acked_at TEXT,
    acked_by TEXT,
    resolved_at TEXT,
    details TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_alerts_status ON alerts(status, fired_at);

CREATE TABLE IF NOT EXISTS notification_channels (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL, -- webhook|email_smtp|slack
    config_json TEXT NOT NULL DEFAULT '{}', -- secrets encrypted inside JSON values where needed
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- ── API tokens (automation) ─────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS api_tokens (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    token_prefix TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'operator',
    scopes TEXT NOT NULL DEFAULT 'read,write',
    expires_at TEXT,
    last_used_at TEXT,
    revoked_at TEXT,
    created_at TEXT NOT NULL
);

-- ── Maintenance windows ─────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS maintenance_windows (
    id TEXT PRIMARY KEY NOT NULL,
    cluster_id TEXT REFERENCES clusters(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    starts_at TEXT NOT NULL,
    ends_at TEXT NOT NULL,
    notify INTEGER NOT NULL DEFAULT 1,
    created_by TEXT,
    created_at TEXT NOT NULL
);

-- ── Daily metric rollups (analytics) ────────────────────────────────────────

CREATE TABLE IF NOT EXISTS metric_daily (
    day TEXT NOT NULL,
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    avg_cpu REAL NOT NULL DEFAULT 0,
    max_cpu REAL NOT NULL DEFAULT 0,
    avg_memory_mb REAL NOT NULL DEFAULT 0,
    max_memory_mb REAL NOT NULL DEFAULT 0,
    samples INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, cluster_id)
);

-- Seed default alert rules
INSERT OR IGNORE INTO alert_rules (
    id, name, enabled, severity, metric, operator, threshold, duration_seconds, scope, notify_channels, cooldown_seconds, created_at, updated_at
) VALUES
('00000000-0000-4000-8000-0000000000a1', 'High CPU', 1, 'warning', 'cpu_percent', 'gt', 85, 300, 'all', 'webhook', 1800, datetime('now'), datetime('now')),
('00000000-0000-4000-8000-0000000000a2', 'High memory', 1, 'warning', 'memory_percent', 'gt', 90, 300, 'all', 'webhook', 1800, datetime('now'), datetime('now')),
('00000000-0000-4000-8000-0000000000a3', 'Backup stale', 1, 'critical', 'backup_age_hours', 'gt', 36, 60, 'all', 'webhook', 3600, datetime('now'), datetime('now')),
('00000000-0000-4000-8000-0000000000a4', 'Replica lag', 1, 'critical', 'replica_lag', 'gt', 600, 120, 'all', 'webhook', 1800, datetime('now'), datetime('now'));

INSERT OR IGNORE INTO settings (key, value, updated_at) VALUES
    ('monitoring.retention_days', '30', datetime('now')),
    ('monitoring.sample_interval_sec', '60', datetime('now')),
    ('replicas.default_sync_cron', '*/30 * * * *', datetime('now')),
    ('panel.allow_viewer_sql', 'false', datetime('now'));
