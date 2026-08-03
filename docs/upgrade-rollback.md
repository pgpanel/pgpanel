# Frissítés és visszavonás

**Fejlesztő:** Dezső Benedek Péter  

## Frissítés productionben

```bash
sudo pgpanel update
```

### Lépések

1. Helyi vs remote verzió megjelenítése  
2. Megerősítés  
3. Backup: `.env`, `installer.conf`, `panel.db` → `/opt/pgpanel/backups/config/<ts>/`  
4. Hivatalos git fa frissítése (`https://github.com/pgpanel/pgpanel.git`)  
5. `PGPANEL_IMAGE` pin frissítése a `VERSION` alapján  
6. `docker compose pull` + `up -d --remove-orphans` (a `--with-host` mód a Caddy/Databasus/Tunnel konfigurációt is frissíti)
7. Health check  

### Amit soha nem töröl

- PostgreSQL volume-ok (`pgpanel_vol_*`)  
- Databasus adatkönyvtár  
- Cloudflare Tunnel token (`/etc/pgpanel/cloudflare-tunnel.env`)
- `.env` titkok (kivéve te állítod vissza backupból)  
- panel SQLite (kivéve explicit restore)  

## Verzióellenőrzés

```bash
pgpanel version
```

## Rollback

```bash
BACKUP=/opt/pgpanel/backups/config/YYYYMMDDHHMMSS
sudo cp -a "$BACKUP/.env" /opt/pgpanel/.env
sudo chmod 0600 /opt/pgpanel/.env
# opcionális panel DB:
sudo docker compose -f /opt/pgpanel/deploy/compose.yml stop panel
sudo cp -a "$BACKUP/panel.db" /var/lib/pgpanel/panel/panel.db
cd /opt/pgpanel/deploy && sudo docker compose up -d
sudo pgpanel status
```

Image pin a `.env` `PGPANEL_IMAGE=` sorában (pl. `ghcr.io/pgpanel/pgpanel:0.1.0`).

## Databasus és Cloudflare Tunnel

Az image-ek verziózva vannak:

- `databasus/databasus:v3.51.0`
- `cloudflare/cloudflared:2026.7.3`

Host oldali változás után:

```bash
sudo pgpanel update --with-host
```

Tunnel módban a Cloudflare Dashboardban a panel és a Databasus hostname-et
egyaránt a `http://caddy:80` originre kell irányítani. A telepítő a tokent nem
írja a Git repositoryba, `.env`-be vagy logba.

## Csatornák

| Channel | Image tag |
|---------|-----------|
| stable | `ghcr.io/pgpanel/pgpanel:<VERSION>` |
| edge | `ghcr.io/pgpanel/pgpanel:latest` |
