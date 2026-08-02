# PgPanel

Self-hosted **PostgreSQL control plane** for Linux VPS.

Provision and manage isolated PostgreSQL clusters with Docker, a secure web admin UI, and backup integration via **Databasus** (backup / WAL / PITR are handled by Databasus, not reimplemented here).

| | |
|---|---|
| **Developer** | Dezső Benedek Péter |
| **Repository** | https://github.com/pgpanel/pgpanel |
| **Version** | [`VERSION`](VERSION) (currently `0.1.0`) |
| **License** | MIT |

---

## Install (one line)

On a fresh Ubuntu / Debian (or similar) VPS:

```bash
sudo apt-get update && sudo apt-get install -y curl && \
curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
```

This is the **supported production install path**. It:

1. Installs prerequisites (`curl`, `git`, `openssl`) if needed  
2. Clones/updates **https://github.com/pgpanel/pgpanel.git** into `/opt/pgpanel`  
3. Launches the interactive installer (domain, HTTPS, admin, firewall, …)  
4. Starts **Caddy + panel + Databasus** via Docker Compose  

**Notes:**

- No Git URL to type — the official repo is fixed.  
- **Only pulls images and configures** — never builds Rust/frontend on the VPS.  
- Panel image: `ghcr.io/pgpanel/pgpanel:<version>` (publish from your Mac with `./deploy/push-image.sh`).  
- Prefer an **SSH session with a real TTY**.  
- If install is interrupted: `sudo bash /opt/pgpanel/deploy/install.sh --mode resume`  
- First login: `https://YOUR_DOMAIN/setup` — default user `admin` (or what you chose); password is shown once.

### Minimum host

| | Minimum | Recommended |
|---|--------|-------------|
| CPU | 2 | 4 |
| RAM | 4 GB | 8 GB |
| Disk | 30 GB free | 80 GB+ |

---

## After install

```bash
pgpanel status
pgpanel logs
pgpanel version
```

| Path | Purpose |
|------|---------|
| `/opt/pgpanel` | App + Compose |
| `/opt/pgpanel/.env` | Secrets (`0600`) |
| `/var/lib/pgpanel` | Panel SQLite + Databasus data |
| `/etc/pgpanel/installer.conf` | Non-secret installer config |
| `/etc/pgpanel/install-answers.env` | Saved answers (resume) |
| `/var/log/pgpanel` | Installer logs |

---

## Update (no data loss)

```bash
sudo pgpanel update
# or:
sudo bash /opt/pgpanel/deploy/install.sh --mode update
```

**Preserved:** `.env` secrets, panel SQLite, PostgreSQL volumes, Databasus data.  
**Never** runs `docker compose down -v` during update.

```bash
sudo pgpanel version    # local vs remote
sudo pgpanel resume     # continue a failed first install
sudo pgpanel repair
sudo pgpanel security-check
sudo pgpanel uninstall
```

---

## Images (GHCR)

Default panel image:

```text
ghcr.io/pgpanel/pgpanel:<VERSION>
```

- Production install **only pulls** this image (plus Caddy + Databasus). **No VPS build.**  
- VPS is typically **linux/amd64** — publish that platform (or multi-arch from your Mac).  
- If pull fails (private/missing package), install **stops** — publish a public GHCR image first.

### Publish image from your Mac

```bash
# once
echo "ghp_XXX" | docker login ghcr.io -u YOUR_GITHUB_USER --password-stdin
# PAT: write:packages, read:packages, repo  (org pgpanel write access)

cd /path/to/pgpanel
# each run: patch +1 (0.1.0 → 0.1.1), build linux/amd64, push, write VERSION
./deploy/push-image.sh --latest
# optional multi-arch:
./deploy/push-image.sh --multi --latest

# recommended: image + VERSION commit + git push
./deploy/release-local.sh --yes
```

- **Auto version:** every `./deploy/push-image.sh` (without `--no-bump` / `--tag`) increments the patch and publishes that tag so `VERSION` always matches GHCR.  
- Make the package **Public**: GitHub → Packages → `pgpanel` → Package settings → Change visibility.  
- If the repo shows **“No packages published”**, the image never reached GHCR (failed login/push). Fix login and re-run push before installing on the VPS.

---

## Features

- Admin bootstrap + **multi-user RBAC** (owner / admin / operator / viewer)  
- PostgreSQL **16 / 17 / 18** only (no `:latest`)  
- Cluster lifecycle: create, start, stop, restart, delete modes  
- **Multi-node**: multiple Docker hosts from one panel  
- **Replicas / redundancy**: duplicate or scheduled sync within a node or across nodes; promote  
- Databases & roles, password rotation  
- Read-only browser + SQL console (parser + `READ ONLY`)  
- **Named backup destinations** — any cluster/node → any storage target (local/S3/R2/B2/MinIO/Hetzner)  
- Per-cluster multi-destination schedules, restore, verify, retention prune  
- **WAF** policy editor with change history + Caddy snippet export  
- **Monitoring & analytics** + alert rules (CPU/mem/backup age/replica lag)  
- API tokens for automation  
- Durable job queue + SSE  
- Caddy HTTPS reverse proxy  
- Idempotent installer with **resume** after failure  

---

## Architecture

```text
Internet → Caddy :80/:443 → panel :8080 (internal)
                         → Databasus (internal; optional public subdomain)
panel + Docker socket → dynamic PostgreSQL containers (private networks)
```

```text
pgpanel/
├── crates/                 # Rust (api, core, docker, postgres, jobs, databasus)
├── frontend/               # SvelteKit + shadcn-svelte
├── migrations/             # Panel SQLite
├── deploy/
│   ├── install.sh          # Full interactive installer
│   ├── compose.yml
│   ├── Caddyfile
│   ├── Dockerfile
│   ├── pgpanel             # CLI → /usr/local/bin/pgpanel
│   ├── push-image.sh       # Local image → GHCR
│   └── release-local.sh    # Image + git release helper
├── install-pgpanel.sh      # One-line bootstrap (curl | bash)
└── VERSION
```

---

## Development

```bash
export PGPANEL_MASTER_KEY="$(openssl rand -base64 48)"
export PGPANEL_COOKIE_SECURE=false
export PGPANEL_DATA_DIR="$(pwd)/data"
export PGPANEL_BIND=127.0.0.1:8080
mkdir -p data && cargo run -p pgpanel-api

cd frontend && npm install && npm run dev
```

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

---

## Documentation

| Doc | Topic |
|-----|--------|
| [docs/installation.md](docs/installation.md) | Install details |
| [docs/getting-started.md](docs/getting-started.md) | First login / admin |
| [docs/upgrade-rollback.md](docs/upgrade-rollback.md) | Updates & rollback |
| [SECURITY.md](SECURITY.md) | Security model & risks |
| [docs/threat-model.md](docs/threat-model.md) | Threat model |

---

## License

MIT · © Dezső Benedek Péter
