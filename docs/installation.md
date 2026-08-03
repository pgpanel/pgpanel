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
| Caddy, Databasus, cloudflared | Hivatalos, verziózott image pull |
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
- Cloudflare Tunnel token (`/etc/pgpanel/cloudflare-tunnel.env`)

## Cloudflare Tunnel

Friss telepítéskor a telepítő bekéri a Tunnel tokent és a Databasus
hostname-et. A token a Cloudflare Dashboardban létrehozott remote Tunnel
`Add a replica` parancsából származó token legyen.

Tunnel módban:

- a `cloudflared` konténer a `pgpanel_frontend` hálózaton fut;
- Caddy csak belső `http://caddy:80` originként működik;
- a hoston nincs 80/443 Caddy port bind, csak a Tunnel kimenő kapcsolata kell;
- a Dashboardban két Public Hostname route szükséges:
  - `<panel-domain>` → `http://caddy:80`
  - `<databasus-domain>` → `http://caddy:80`
- a token `/etc/pgpanel/cloudflare-tunnel.env` fájlban marad, `0600`
  jogosultsággal.

A Tunnel csatlakozásához a VPS-ről kimenő TCP/UDP 7844 és TCP 443 legyen
engedélyezve. A telepítő a Cloudflare DNS/ingress konfigurációját nem módosítja.

## Databasus

A Databasus hivatalos `databasus/databasus:v3.51.0` image-ként indul,
adatkönyvtára `/var/lib/pgpanel/databasus`, belső portja `4005`. Nincs host
portja és nem kap Docker socketet. A panel által létrehozott PostgreSQL
konténereket a `pgpanel_database_management` hálózaton éri el.

Az első belépés után a Databasus UI-ban manuálisan:

1. hozz létre Databasus admin felhasználót;
2. add hozzá a klasztert `pgpanel_pg_<slug>` hosttal és `5432` porttal;
3. állítsd be a storage-ot, retentiont, WAL/PITR-t és restore verificationt;
4. futtass explicit backup + restore próbát.

A PgPanel natív backup motorja ettől függetlenül megmarad; a telepítő nem
állít be nem dokumentált Databasus HTTP API hívásokat.

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
