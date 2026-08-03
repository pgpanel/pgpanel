# Recovery Procedures

This guide covers common failure scenarios and recovery steps for PgPanel on Ubuntu 24.04.

## Quick diagnostics

```bash
sudo ./scripts/diagnostics.sh /tmp/pgpanel-recovery
journalctl -u pgpanel-helper -u pgpanel-blue -u caddy --since "1 hour ago"
curl -s http://127.0.0.1:8080/health/ready | jq .
```

## Service will not start

### pgpanel-helper

```bash
sudo systemctl status pgpanel-helper
sudo journalctl -u pgpanel-helper -n 50
```

Common causes:

- Invalid `/etc/pgpanel/pgpanel.toml` — validate syntax and `secret_key` length (≥ 32)
- Socket path not writable — check `/run/pgpanel` permissions
- Missing binary — verify `/opt/pgpanel/current/bin/pgpanel-helper` exists

```bash
sudo /opt/pgpanel/current/bin/pgpanel-helper --config /etc/pgpanel/pgpanel.toml
```

### pgpanel-blue / pgpanel-green

```bash
sudo systemctl status pgpanel-blue
```

Common causes:

- Helper not running (web requires helper for readiness)
- Port conflict on 8081/8082
- SQLite permissions on `/var/lib/pgpanel/pgpanel.db`

```bash
sudo chown -R pgpanel:pgpanel /var/lib/pgpanel
sudo chmod 750 /var/lib/pgpanel
```

### Caddy

```bash
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl status caddy
```

Upstream health failures if no slot is running:

```bash
curl http://127.0.0.1:8081/health/live
```

## Failed update

If an update fails mid-process:

```bash
sudo /opt/pgpanel/current/bin/pgpanel-updater --config /etc/pgpanel/pgpanel.toml status
```

Rollback:

```bash
sudo ./scripts/rollback.sh
```

If updater is broken, manual Caddy switch:

```bash
# Edit /etc/caddy/pgpanel-upstream.caddy to point at known-good upstream
sudo systemctl reload caddy
sudo systemctl restart pgpanel-blue  # or green, matching upstream
```

## Health check fails after update

The updater should auto-rollback Caddy. Verify:

```bash
cat /etc/caddy/pgpanel-upstream.caddy
curl http://127.0.0.1:8080/health/ready
```

Restore SQLite from pre-update backup if migrations corrupted state:

```bash
sudo systemctl stop pgpanel-blue pgpanel-green
sudo cp /opt/pgpanel/state/backups/pgpanel-*.db.bak /var/lib/pgpanel/pgpanel.db
sudo chown pgpanel:pgpanel /var/lib/pgpanel/pgpanel.db
sudo systemctl start pgpanel-helper pgpanel-blue
```

## Corrupted SQLite database

```bash
sudo systemctl stop pgpanel-blue pgpanel-green
sudo sqlite3 /var/lib/pgpanel/pgpanel.db "PRAGMA integrity_check;"
```

If integrity check fails, restore from [BACKUP.md](../BACKUP.md) or re-run first-run setup (loses panel users/audit history).

## Locked out (admin password)

No backdoor. Recovery options:

1. Restore SQLite from backup containing known credentials
2. If no backup: stop web, reset database (destructive to panel state only):

```bash
sudo systemctl stop pgpanel-blue pgpanel-green
sudo mv /var/lib/pgpanel/pgpanel.db /var/lib/pgpanel/pgpanel.db.bak
sudo systemctl start pgpanel-helper pgpanel-blue
# Complete first-run setup again
```

PostgreSQL data is unaffected.

## Helper socket permission errors

```bash
ls -la /run/pgpanel/
sudo chown root:pgpanel /run/pgpanel
sudo chmod 750 /run/pgpanel
sudo systemctl restart pgpanel-helper
```

## Disk full

```bash
df -h /var/lib/pgpanel /var/lib/postgresql /opt/pgpanel
```

Clean old releases (keep `current` and `previous`):

```bash
ls /opt/pgpanel/releases/
# Remove old version directories manually after confirming symlinks
```

Prune monitoring samples via retention settings in config.

## Complete reinstall (preserve PostgreSQL)

```bash
sudo ./scripts/uninstall.sh          # keeps /var/lib/pgpanel by default
sudo ./scripts/install.sh --version vX.Y.Z
```

Purge everything including panel state:

```bash
sudo ./scripts/uninstall.sh --purge-state
```

## PostgreSQL cluster issues

PgPanel issues are separate from PostgreSQL failures:

```bash
sudo pg_lsclusters
sudo pg_ctlcluster 17 main status
sudo tail -f /var/log/postgresql/postgresql-17-main.log
```

Use standard PostgreSQL recovery for data corruption or crash recovery.

## Escalation data to collect

```bash
sudo ./scripts/diagnostics.sh /tmp/incident
sudo journalctl -u 'pgpanel-*' -u caddy --since today > /tmp/incident/journal.txt
```

Redact secrets before sharing externally. Report security incidents per [SECURITY.md](../SECURITY.md).
