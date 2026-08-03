# Backup Guide

PgPanel manages PostgreSQL clusters on the host filesystem. Backups require a strategy for both **PgPanel state** and **PostgreSQL data**.

## PgPanel internal state

### SQLite database

Location: `/var/lib/pgpanel/pgpanel.db`

Contains users, sessions (hashed), audit log, monitoring samples, and settings.

**Manual backup:**

```bash
sudo sqlite3 /var/lib/pgpanel/pgpanel.db ".backup '/var/backups/pgpanel-$(date +%F).db'"
```

**During updates:** the updater automatically copies SQLite to `/opt/pgpanel/state/backups/` before migration.

### Configuration

```bash
sudo tar -czf pgpanel-config-$(date +%F).tar.gz /etc/pgpanel/
```

Includes `pgpanel.toml` and `signing.pub`. Store secrets encrypted at rest.

## PostgreSQL clusters

PgPanel does **not** replace a dedicated backup solution. Use `pg_dump`, `pg_basebackup`, WAL archiving, or [Databasus](docs/databasus.md).

### Logical backup (single database)

```bash
sudo -u postgres pg_dump -Fc mydb > mydb-$(date +%F).dump
```

### Cluster-wide (all databases)

```bash
sudo pg_dumpall -h /var/run/postgresql > dumpall-$(date +%F).sql
```

### Physical backup

Use `pg_basebackup` against clusters managed by `postgresql-common`:

```bash
sudo -u postgres pg_basebackup -D /var/backups/pg17-main-$(date +%F) -Fp -Xs -P
```

Consult PostgreSQL documentation for PITR and WAL archiving.

## Databasus integration

The PgPanel installer does **not** install Databasus. If Databasus is already deployed, you can enable an optional health probe in `pgpanel.toml`:

```toml
[databasus]
enabled = true
base_url = "https://databasus.internal"
tls_verify = true
```

PgPanel only probes `GET /api/v1/system/health`. It does **not** expose backup listing, trigger, or restore operations today. An API key is optional and unused for the health probe. See [docs/databasus.md](docs/databasus.md).

## Backup schedule recommendations

| Asset | Frequency | Retention |
|-------|-----------|-----------|
| PgPanel SQLite | Daily | 30 days |
| `/etc/pgpanel` | On change | Version controlled |
| PostgreSQL logical | Daily | Per policy |
| PostgreSQL physical / WAL | Continuous | Per RPO/RTO |

## Restore PgPanel state

1. Stop web slots: `sudo systemctl stop pgpanel-blue pgpanel-green`
2. Restore database: `sudo cp /var/backups/pgpanel-YYYY-MM-DD.db /var/lib/pgpanel/pgpanel.db`
3. Fix ownership: `sudo chown pgpanel:pgpanel /var/lib/pgpanel/pgpanel.db`
4. Start services: `sudo systemctl start pgpanel-helper pgpanel-blue`

## Restore PostgreSQL

Use standard PostgreSQL restore procedures. PgPanel does not automate cluster restore — recreate clusters with `pg_createcluster` or restore data directories following PostgreSQL docs.

## Off-site backups

- Encrypt backups before transfer
- Test restore quarterly
- Exclude secrets from unencrypted off-site copies where possible
- Audit log integrity: export CSV periodically for long-term retention

## What uninstall preserves

`scripts/uninstall.sh` preserves `/var/lib/pgpanel` unless `--purge-state` is passed. PostgreSQL data is never touched.
