-- Unlimited capacity by default; node pairing / fleet; update channel

-- Treat 0 / empty as unlimited: clear accidental zero caps
UPDATE nodes SET max_clusters = NULL WHERE max_clusters IS NOT NULL AND max_clusters <= 0;

-- Fleet pairing: this panel can expose an agent endpoint; peers join with token
CREATE TABLE IF NOT EXISTS fleet_peers (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    -- encrypted shared token for calling peer agent API
    token_encrypted TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending', -- pending|online|offline|revoked
    role TEXT NOT NULL DEFAULT 'peer', -- peer|controller
    advertised_capacity INTEGER, -- peer-reported max (informational; peer enforces locally)
    cluster_count INTEGER NOT NULL DEFAULT 0,
    last_seen_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS fleet_invite_tokens (
    id TEXT PRIMARY KEY NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    token_prefix TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT 'invite',
    expires_at TEXT,
    used_at TEXT,
    revoked_at TEXT,
    created_by TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS panel_remote_access (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    enabled INTEGER NOT NULL DEFAULT 0,
    public_base_url TEXT NOT NULL DEFAULT '',
    agent_token_hash TEXT,
    agent_token_prefix TEXT,
    updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO panel_remote_access (id, enabled, public_base_url, updated_at)
VALUES (1, 0, '', datetime('now'));

INSERT OR IGNORE INTO settings (key, value, updated_at) VALUES
    ('update.channel', 'stable', datetime('now')),
    ('update.last_checked_at', '', datetime('now')),
    ('update.latest_version', '', datetime('now')),
    ('update.latest_url', '', datetime('now')),
    ('fleet.public_name', 'PgPanel node', datetime('now'));
