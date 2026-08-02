CREATE TABLE IF NOT EXISTS wal_streams (
  cluster_id TEXT PRIMARY KEY NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
  enabled INTEGER NOT NULL DEFAULT 0,
  archive_dir TEXT NOT NULL DEFAULT '/var/lib/postgresql/wal_archive',
  compress INTEGER NOT NULL DEFAULT 1,
  retention_days INTEGER NOT NULL DEFAULT 14,
  status TEXT NOT NULL DEFAULT 'disabled',
  last_segment TEXT,
  last_synced_at TEXT,
  last_error TEXT,
  timeline INTEGER,
  segment_count INTEGER NOT NULL DEFAULT 0,
  total_bytes INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS wal_segments (
  id TEXT PRIMARY KEY NOT NULL,
  cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
  filename TEXT NOT NULL,
  timeline INTEGER NOT NULL DEFAULT 1,
  size_bytes INTEGER NOT NULL DEFAULT 0,
  archived_at TEXT,
  synced_at TEXT NOT NULL,
  storage_key TEXT NOT NULL,
  checksum_sha256 TEXT,
  UNIQUE(cluster_id, filename)
);
CREATE INDEX IF NOT EXISTS idx_wal_segments_cluster_time ON wal_segments(cluster_id, archived_at);
CREATE INDEX IF NOT EXISTS idx_wal_segments_cluster_name ON wal_segments(cluster_id, filename);
CREATE INDEX IF NOT EXISTS idx_wal_segments_timeline ON wal_segments(cluster_id, timeline);
-- WAL streaming archive + searchable segment index

CREATE TABLE IF NOT EXISTS wal_streams (
    cluster_id TEXT PRIMARY KEY NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 0,
    archive_dir TEXT NOT NULL DEFAULT '/var/lib/postgresql/wal_archive',
    compress INTEGER NOT NULL DEFAULT 1,
    retention_days INTEGER NOT NULL DEFAULT 14,
    status TEXT NOT NULL DEFAULT 'disabled',
    last_segment TEXT,
    last_synced_at TEXT,
    last_error TEXT,
    timeline INTEGER,
    segment_count INTEGER NOT NULL DEFAULT 0,
    total_bytes INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS wal_segments (
    id TEXT PRIMARY KEY NOT NULL,
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    filename TEXT NOT NULL,
    timeline INTEGER NOT NULL DEFAULT 1,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    archived_at TEXT,
    synced_at TEXT NOT NULL,
    storage_key TEXT NOT NULL,
    checksum_sha256 TEXT,
    UNIQUE(cluster_id, filename)
);

CREATE INDEX IF NOT EXISTS idx_wal_segments_cluster_time ON wal_segments(cluster_id, archived_at);
CREATE INDEX IF NOT EXISTS idx_wal_segments_cluster_name ON wal_segments(cluster_id, filename);
CREATE INDEX IF NOT EXISTS idx_wal_segments_timeline ON wal_segments(cluster_id, timeline);
