# PostgreSQL Permissions

PgPanel manages PostgreSQL through `postgresql-common` on Ubuntu. This document explains how panel permissions map to PostgreSQL operations and recommended OS-level setup.

## postgresql-common tools

PgPanel invokes only these binaries with validated arguments:

| Binary | Purpose |
|--------|---------|
| `/usr/bin/pg_lsclusters` | List clusters |
| `/usr/bin/pg_createcluster` | Create cluster |
| `/usr/bin/pg_ctlcluster` | Start/stop/reload |
| `/usr/bin/pg_renamecluster` | Rename cluster |
| `/usr/bin/pg_dropcluster` | Delete cluster |
| `/usr/bin/pg_conftool` | Configuration validation |

The helper runs as **root** because `postgresql-common` expects root for cluster lifecycle operations. The web process never calls these directly.

## Cluster naming

System cluster identifiers must match:

```
^[a-z][a-z0-9_]{1,31}$
```

Display names may be more permissive in the UI but map to validated system identifiers at creation time.

## Data directories

Only paths under configured roots are permitted:

```toml
[postgres]
data_directory_roots = ["/var/lib/postgresql"]
```

Custom data directories outside allowlisted roots are rejected.

## PostgreSQL versions

```toml
[postgres]
allowed_versions = ["16", "17", "18"]
```

Install server packages before creating clusters:

```bash
sudo apt-get install postgresql-17
```

## Panel roles vs PostgreSQL roles

PgPanel application roles (owner, administrator, operator, viewer) are **independent** of PostgreSQL database roles. Panel permissions gate access to UI actions:

| Permission | Capability |
|------------|------------|
| `clusters.read` | View cluster list and status |
| `clusters.create` | Create new clusters |
| `clusters.control` | Start/stop/restart/reload |
| `clusters.configure` | Edit allowlisted `postgresql.conf` settings |
| `clusters.delete` | Drop clusters (destructive) |
| `databases.*` | Database CRUD |
| `roles.manage` | PostgreSQL role management |
| `queries.execute` | SQL editor |
| `logs.read` | PostgreSQL log tail |
| `updates.manage` | Trigger panel updates |

## Default PostgreSQL role policy

New login roles created through PgPanel default to:

```sql
NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS
```

Superuser creation requires:

```toml
[app]
allow_superuser_creation = true
```

Plus recent re-authentication and TOTP when enabled.

## SQL editor

- Viewers cannot execute queries
- Operators may be restricted to read-only transactions
- Statement timeout and row limits enforced server-side
- Identifiers quoted via safe quoting function; literals parameterized

## File permissions

PostgreSQL data remains owned by the `postgres` OS user. PgPanel does not change cluster ownership. Configuration backups are written with preserved ownership during config edits.

## OS user `pgpanel`

The `pgpanel` system user:

- Cannot read PostgreSQL data files directly
- Connects to helper via Unix socket only
- Has read/write to `/var/lib/pgpanel` and `/run/pgpanel`

## Hardening PostgreSQL itself

PgPanel does not replace PostgreSQL hardening:

- Set `listen_addresses` appropriately per cluster
- Use `scram-sha-256` authentication
- Restrict `pg_hba.conf` to trusted networks
- Do not expose port 5432 publicly

## Auditing

Every privileged helper operation generates an audit event with user, IP, action, target, and result. Export from the Audit Log page.

See [THREAT_MODEL.md](../THREAT_MODEL.md) and [SECURITY.md](../SECURITY.md).
