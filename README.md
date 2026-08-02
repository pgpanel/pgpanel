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
- Prefer an **SSH session with a real TTY** (not a broken non-interactive pipe).  
- If install is interrupted: `sudo bash /opt/pgpanel/deploy/install.sh --mode resume`  
- First login: `https://YOUR_DOMAIN/setup` — default user `admin` (or what you chose); password is generated/shown once.

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

- VPS is typically **linux/amd64** — that is what production pulls.  
- Apple Silicon (arm64) needs either `--platform linux/amd64` or a multi-arch publish.  
- If GHCR is private/missing, the installer **falls back to building on the VPS** (slow) or reuses a local `pgpanel-panel` image.

### Publish image from your Mac

```bash
# once
echo "ghp_XXX" | docker login ghcr.io -u YOUR_GITHUB_USER --password-stdin

cd /path/to/pgpanel
./deploy/push-image.sh --latest          # linux/amd64 → GHCR
# optional multi-arch:
./deploy/push-image.sh --multi --latest

# image + git in one go:
./deploy/release-local.sh --yes --latest
```

Make the package **public** under GitHub → Packages → `pgpanel` if VPS pulls without auth.

---

## Features

- Admin bootstrap + session login (Argon2id, server-side sessions, CSRF)  
- PostgreSQL **16 / 17 / 18** only (no `:latest`)  
- Cluster lifecycle: create, start, stop, restart, delete modes  
- Databases & roles, password rotation  
- Read-only browser + SQL console (parser + `READ ONLY`)  
- Durable job queue + SSE  
- Caddy HTTPS reverse proxy  
- Databasus adapter (HTTP / manual / mock)  
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
