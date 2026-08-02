-- Multi-node hosts, WAF change tracking, enhanced backup policy

-- ── Nodes (Docker hosts managed by one panel) ───────────────────────────────

CREATE TABLE IF NOT EXISTS nodes (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL DEFAULT 'local', -- local | remote
    docker_host TEXT,                   -- unix:///… or tcp://… (encrypted when remote)
    docker_host_encrypted INTEGER NOT NULL DEFAULT 0,
    labels TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'unknown', -- unknown | online | offline | degraded | disabled
    last_seen_at TEXT,
    last_error TEXT,
    max_clusters INTEGER,
    notes TEXT,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_nodes_status ON nodes(status);
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);

-- Seed local node (idempotent)
INSERT OR IGNORE INTO nodes (
    id, name, slug, kind, docker_host, docker_host_encrypted, labels,
    status, is_default, created_at, updated_at
) VALUES (
    '00000000-0000-4000-8000-000000000001',
    'Local host',
    'local',
    'local',
    NULL,
    0,
    '{"role":"primary"}',
    'online',
    1,
    datetime('now'),
    datetime('now')
);

-- Attach clusters to a node (default = local)
ALTER TABLE clusters ADD COLUMN node_id TEXT REFERENCES nodes(id);
UPDATE clusters SET node_id = '00000000-0000-4000-8000-000000000001' WHERE node_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_clusters_node ON clusters(node_id);

-- ── WAF policy + change history ─────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS waf_policies (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    enabled INTEGER NOT NULL DEFAULT 1,
    is_active INTEGER NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 1,
    -- JSON body: rate limits, IP lists, UA blocks, body size, geo, challenge, headers
    config_json TEXT NOT NULL DEFAULT '{}',
    notes TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS waf_change_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    policy_id TEXT NOT NULL REFERENCES waf_policies(id) ON DELETE CASCADE,
    version_from INTEGER,
    version_to INTEGER NOT NULL,
    actor_id TEXT,
    actor_username TEXT,
    change_summary TEXT NOT NULL,
    before_json TEXT,
    after_json TEXT NOT NULL,
    reason TEXT,
    ip_address TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_waf_changes_policy ON waf_change_log(policy_id, created_at);
CREATE INDEX IF NOT EXISTS idx_waf_changes_created ON waf_change_log(created_at);

INSERT OR IGNORE INTO waf_policies (
    id, name, enabled, is_active, version, config_json, notes, created_at, updated_at
) VALUES (
    '00000000-0000-4000-8000-0000000000af',
    'default',
    1,
    1,
    1,
    '{"rate_limit_per_minute":120,"login_rate_limit_per_minute":20,"api_rate_limit_per_minute":300,"max_body_bytes":10485760,"block_empty_user_agent":false,"blocked_user_agents":[],"allowed_ips":[],"denied_ips":[],"blocked_paths":[],"challenge_suspicious":false,"geo_block_countries":[],"enable_security_headers":true,"hsts_max_age":31536000,"csp_mode":"strict","fail_closed_on_deny":true}',
    'Default panel WAF edge policy (applied in-app + tracked for Caddy sync)',
    datetime('now'),
    datetime('now')
);

INSERT INTO waf_change_log (
    policy_id, version_from, version_to, actor_username, change_summary, before_json, after_json, reason, created_at
)
SELECT
    '00000000-0000-4000-8000-0000000000af',
    NULL,
    1,
    'system',
    'Initial default WAF policy',
    NULL,
    config_json,
    'bootstrap',
    datetime('now')
FROM waf_policies
WHERE id = '00000000-0000-4000-8000-0000000000af'
  AND NOT EXISTS (SELECT 1 FROM waf_change_log WHERE policy_id = '00000000-0000-4000-8000-0000000000af');

-- ── Enhanced backup schedules / policy ──────────────────────────────────────

ALTER TABLE backup_schedules ADD COLUMN compression_level INTEGER NOT NULL DEFAULT 6;
ALTER TABLE backup_schedules ADD COLUMN dump_format TEXT NOT NULL DEFAULT 'custom'; -- custom | plain | directory
ALTER TABLE backup_schedules ADD COLUMN schema_only INTEGER NOT NULL DEFAULT 0;
ALTER TABLE backup_schedules ADD COLUMN exclude_schemas TEXT NOT NULL DEFAULT '';
ALTER TABLE backup_schedules ADD COLUMN exclude_tables TEXT NOT NULL DEFAULT '';
ALTER TABLE backup_schedules ADD COLUMN include_schemas TEXT NOT NULL DEFAULT '';
ALTER TABLE backup_schedules ADD COLUMN jobs INTEGER NOT NULL DEFAULT 1;
ALTER TABLE backup_schedules ADD COLUMN notify_on_success INTEGER NOT NULL DEFAULT 0;
ALTER TABLE backup_schedules ADD COLUMN notify_on_failure INTEGER NOT NULL DEFAULT 1;
ALTER TABLE backup_schedules ADD COLUMN verify_after INTEGER NOT NULL DEFAULT 0;
ALTER TABLE backup_schedules ADD COLUMN window_start_hour INTEGER; -- optional UTC hour window
ALTER TABLE backup_schedules ADD COLUMN window_end_hour INTEGER;
ALTER TABLE backup_schedules ADD COLUMN pause_until TEXT;
ALTER TABLE backup_schedules ADD COLUMN next_run_at TEXT;
ALTER TABLE backup_schedules ADD COLUMN description TEXT;

ALTER TABLE backups ADD COLUMN node_id TEXT;
ALTER TABLE backups ADD COLUMN schedule_id TEXT;
ALTER TABLE backups ADD COLUMN verified INTEGER NOT NULL DEFAULT 0;
ALTER TABLE backups ADD COLUMN retained_until TEXT;
ALTER TABLE backups ADD COLUMN notes TEXT;

-- Global backup policy keys (defaults)
INSERT OR IGNORE INTO settings (key, value, updated_at) VALUES
    ('backup.compression_level', '6', datetime('now')),
    ('backup.dump_format', 'custom', datetime('now')),
    ('backup.verify_after', 'false', datetime('now')),
    ('backup.keep_local_copy', 'false', datetime('now')),
    ('backup.notify_webhook', '', datetime('now')),
    ('backup.notify_on_success', 'false', datetime('now')),
    ('backup.notify_on_failure', 'true', datetime('now')),
    ('backup.cron_default', '0 3 * * *', datetime('now')),
    ('backup.exclude_schemas_default', '', datetime('now')),
    ('backup.wal_archiving_default', 'false', datetime('now')),
    ('backup.parallel_jobs', '1', datetime('now')),
    ('waf.active_policy_id', '00000000-0000-4000-8000-0000000000af', datetime('now'));
