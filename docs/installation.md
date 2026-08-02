# PgPanel telepítés

**Fejlesztő:** Dezső Benedek Péter  

## Egy parancs (production)

```bash
sudo apt-get update && sudo apt-get install -y curl && \
curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
```

A telepítő **nem kér** Git repository URL-t. A hivatalos forrás rögzített:

`https://github.com/pgpanel/pgpanel.git`

## Kell-e a VPS-nek fordítania?

**Nem (alapértelmezés).**

| Komponens | A VPS-en |
|-----------|---------|
| Rust API + frontend | **Előre buildelt** Docker image: `ghcr.io/pgpanel/pgpanel:<verzió>` |
| Caddy, Databasus | Hivatalos image pull |
| PostgreSQL clusterek | Dinamikus container a panel által |

A CI (`.github/workflows/docker-publish.yml`) multi-arch (`amd64`/`arm64`) image-t tol a GHCR-re. A VPS csak `docker compose pull` + `up`.

Helyi fordítás csak ha az image nem elérhető:

```bash
sudo bash /opt/pgpanel/deploy/install.sh --build-local
# vagy: PGPANEL_BUILD_LOCAL=1
```

## Frissítés (adatvesztés nélkül)

```bash
sudo pgpanel update
# vagy menü → 2. Frissítés
# vagy: sudo bash /opt/pgpanel/deploy/install.sh --mode update
```

A frissítő:

1. **Verzióellenőrzés** (helyi `VERSION` vs remote)
2. Config + panel SQLite backup (`/opt/pgpanel/backups/config/<timestamp>/`)
3. Hivatalos repo `git pull` / checkout
4. **Új image pull** (nem töröl volume-ot)
5. `docker compose up -d` (**soha** `down -v`)
6. Health check; hibánál rollback ajánlat

**Megmarad:**

- `/opt/pgpanel/.env` titkok  
- panel SQLite  
- PostgreSQL Docker volume-ok  
- Databasus adatok  

## Verzió

```bash
pgpanel version
sudo bash /opt/pgpanel/deploy/install.sh --mode version
```

Verzió forrása: gyökér `VERSION` fájl (pl. `0.1.0`).

## Alapértelmezett admin

| Mező | Érték |
|------|--------|
| User | `admin` (a webes setup során választható) |
| Jelszó | A webes setup során választandó — **nincs** fix gyári jelszó |
| Setup | `https://DOMAIN/setup` |

## Hasznos parancsok

```bash
pgpanel status
pgpanel logs
pgpanel update
pgpanel version
pgpanel repair
pgpanel security-check
pgpanel uninstall
```

## Elérési utak

| Útvonal | Tartalom |
|---------|----------|
| `/opt/pgpanel` | Alkalmazás / compose |
| `/opt/pgpanel/.env` | Titkok (`0600`) |
| `/var/lib/pgpanel` | Panel + Databasus adatok |
| `/etc/pgpanel/installer.conf` | Nem-titkos telepítő config |
| `/var/log/pgpanel` | Installer log |
