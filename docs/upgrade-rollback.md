# Upgrade and rollback

## Update flow (`pgpanel update`)

1. Load `/etc/pgpanel/installer.conf`  
2. Backup config to `/opt/pgpanel/backups/config/<timestamp>/`  
   - `.env`  
   - `installer.conf`  
   - panel SQLite `panel.db` if present  
3. Fetch/checkout git ref (or rsync local tree)  
4. **Do not regenerate secrets**  
5. Re-render `compose.yml` + `Caddyfile`  
6. `docker compose pull/build/up -d`  
7. Health checks (`/health`, `/ready`, containers)  
8. On failure: offer rollback of `.env` and re-up  

### What update must never delete

- PostgreSQL Docker volumes (`pgpanel_vol_*`)  
- Databasus data directory  
- `/opt/pgpanel/.env` secrets (except restore from backup you choose)  
- Panel SQLite (except the timestamped backup copy)  
- Backup storage objects in S3/R2/etc.  

## Rollback procedure

### Soft rollback (config)

```bash
BACKUP=/opt/pgpanel/backups/config/YYYYMMDDHHMMSS
sudo cp -a "$BACKUP/.env" /opt/pgpanel/.env
sudo chmod 0600 /opt/pgpanel/.env
cd /opt/pgpanel/deploy && sudo docker compose up -d
sudo pgpanel status
```

### Application code rollback

```bash
cd /opt/pgpanel
sudo git fetch --tags
sudo git checkout <previous-tag-or-commit>
cd deploy && sudo docker compose build && sudo docker compose up -d
```

### Database (panel SQLite)

```bash
sudo docker compose -f /opt/pgpanel/deploy/compose.yml stop panel
sudo cp -a /opt/pgpanel/backups/config/<ts>/panel.db /var/lib/pgpanel/panel/panel.db
sudo docker compose -f /opt/pgpanel/deploy/compose.yml start panel
```

## Repair vs update

| Mode | Use when |
|------|----------|
| `repair` | networks missing, containers stopped, perms wrong, compose file missing — **no secret regen** |
| `update` | new PgPanel version / image rebuild |
| `configure` | change domain, notify, backup settings without full reinstall |

## Channel: stable vs edge

Stored in `UPDATE_CHANNEL` / installer conf. Edge may track `main`; stable should pin tags once you publish releases.

## Production readiness gate

`PRODUCTION_READY` stays `0` until:

1. Databasus backup storage verified  
2. At least one successful backup of a real or test cluster  
3. Restore verification succeeds  

Document the gate in ops runbooks; the installer will not flip the flag automatically on green health alone.
