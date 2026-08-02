# Installer security guide

**Developer:** Dezső Benedek Péter

## Secret handling

| Item | Rule |
|------|------|
| Generation | `openssl rand` only |
| Storage | `/opt/pgpanel/.env`, mode `0600`, owner `root` |
| Re-install | Existing `.env` is **never** regenerated on update/repair |
| Logs | `mask_secrets` strips passwords, tokens, keys, connection strings |
| Compose | Secrets via `env_file`, never as CLI `-e` for long-lived secrets |
| Passwords | The first admin password is entered in the web setup; database and storage secrets are encrypted or kept in the root-only `.env` |

Non-secret installer choices live in `/etc/pgpanel/installer.conf` (no raw passwords).

## Docker socket

The panel mounts `/var/run/docker.sock` to create PostgreSQL containers.

| Component | Socket? |
|-----------|---------|
| panel | **yes** (required for MVP provisioning) |
| caddy | no |
| databasus | no |
| frontend (static) | n/a |

Treat socket access as **host root equivalent**. Future architecture: dedicated provisioner service.

Installer `security-check` verifies Databasus does not mount the socket and flags privileged containers.

## Network exposure

| Port / surface | Default |
|----------------|---------|
| 80/443 | public via Caddy only (panel domain) |
| 8080 panel | internal Docker network |
| 8000 Databasus | **internal only** (`http://databasus:8000`); public subdomain only if installer `DATABASUS_PUBLIC=1` |
| 5432 PostgreSQL | **not** opened; no public PG domain. Clusters dual-home: private `pgpanel_net_*` + shared `pgpanel_database_management` |

Docker networks:

| Network | Purpose | External |
|---------|---------|----------|
| `pgpanel_frontend` | Caddy ↔ panel | yes (edge) |
| `pgpanel_internal` | panel ↔ Databasus | stack-local |
| `pgpanel_database_management` | panel + Databasus ↔ `pgpanel_pg_*` DNS | **internal: true** |

UFW order: **SSH first**, then 80/443, then enable. PostgreSQL is not globally allowed.

## HTTPS

- Default recommendation: HTTPS on  
- HTTP-only requires explicit confirmation with production warning  
- Security headers set in Caddyfile (HSTS, CSP, X-Frame-Options, etc.)

## Security score

`pgpanel security-check` prints a 0–100 score based on:

- `.env` permissions and ownership  
- HTTPS/domain  
- public PG ports default  
- UFW active  
- Databasus socket absence  
- privileged containers  
- backup storage configured  
- free disk  

Even a high score does **not** set `PRODUCTION_READY=1` without backup/restore verification.

## Multi-distro package trust

Docker is installed from **official Docker repositories** for apt/dnf where available. Avoid third-party “get.docker.com” opaque wrappers when possible; the installer uses vendor repos + package managers.
