# PgPanel

Self-hosted, PostgreSQL-only control panel for Ubuntu/Debian VPS deployments.

PgPanel creates and manages **multiple isolated PostgreSQL clusters** via Docker, provides a secure admin UI (databases, roles, read-only browser, SQL console), and **delegates backup / WAL / PITR / restore verification to Databasus**.

> **MVP status:** Core lifecycle, auth, browser, SQL console, job queue, Compose stack, and Databasus **adapter interface** (manual + HTTP placeholder + mock) are implemented.  
> **Not production-ready** until real Databasus backup/restore integration is verified for your Databasus version. See [SECURITY.md](SECURITY.md).

## Features (MVP)

1. Admin bootstrap + session login (Argon2id, server-side sessions, CSRF)
2. Create PostgreSQL **16 / 17 / 18** clusters (allowlisted images only)
3. Start / stop / restart
4. Create databases + non-superuser roles; password rotation
5. Read-only schema/table/row browser
6. Read-only SQL console (`pg_query` + `READ ONLY` transaction)
7. Databasus adapter trait: mock, manual, HTTP (paths must be verified)
8. Docker Compose + Caddy reverse proxy
9. Durable SQLite job queue + SSE operation updates
10. `install.sh` for Ubuntu 22.04/24.04 and Debian 12

## Architecture

```
pgpanel/
├── crates/
│   ├── pgpanel-api/        # Axum routes, auth, SSE
│   ├── pgpanel-core/       # models, crypto, validation, errors
│   ├── pgpanel-docker/     # Bollard; allowlisted PG ops only
│   ├── pgpanel-postgres/   # roles, browser, SQL console
│   ├── pgpanel-databasus/  # Databasus adapters
│   └── pgpanel-jobs/       # durable job queue + workers
├── frontend/               # SvelteKit + Tailwind
├── migrations/             # SQLite (panel state)
├── deploy/                 # compose, Caddy, Dockerfile, install.sh
└── tests/
```

- Panel state: **SQLite (WAL)** — never on a managed cluster.
- Each cluster: own volume + private Docker network when possible.
- Public DB port: **off by default**.
- Docker socket: panel only (documented high risk).

## Quick start (development)

### Prerequisites

- Rust stable (1.85+)
- Node 20+
- Docker Engine (for real cluster provisioning)

```bash
# Backend
export PGPANEL_MASTER_KEY="$(openssl rand -base64 48)"
export PGPANEL_COOKIE_SECURE=false
export PGPANEL_DATA_DIR="$(pwd)/data"
export PGPANEL_BIND=127.0.0.1:8080
mkdir -p data
cargo run -p pgpanel-api

# Frontend (another terminal)
cd frontend && npm install && npm run dev
```

Open http://127.0.0.1:5173 → `/setup` for first admin.

### Tests

```bash
cargo test --workspace
# SQL console allowlist unit tests live in pgpanel-postgres
# Crypto/validation unit tests live in pgpanel-core
```

### Clippy

```bash
cargo clippy --workspace -- -D warnings
```

## Default admin

| Field | Default |
|-------|---------|
| Username | `admin` (changeable during install) |
| Password | **No fixed default** — auto-generated (shown once) or set interactively |
| First visit | `https://YOUR_DOMAIN/setup` |

After install, password is printed once and may be in `/etc/pgpanel/.admin-password-ONCE` (delete after saving). Bootstrap token: `PGPANEL_BOOTSTRAP_TOKEN` in `/opt/pgpanel/.env`.

See [docs/getting-started.md](docs/getting-started.md).

## Production install (VPS)

### One-liner (Databasus-style)

After you push this repo to GitHub and replace `pgpanel`:

```bash
sudo apt-get update && sudo apt-get install -y curl && \
curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
```

Or with explicit repo:

```bash
sudo apt-get update && sudo apt-get install -y curl && \
PGPANEL_REPO=https://github.com/pgpanel/pgpanel.git \
curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
```

### From a local clone

Interactive multi-distro installer (Ubuntu/Debian primary; also RHEL-family, Fedora, Amazon Linux, openSUSE, Arch):

```bash
sudo bash install.sh
# equivalent:
sudo bash deploy/install.sh
```

Menu: new install · update · repair · configure · security-check · backup-test · uninstall.

```bash
sudo bash deploy/install.sh --mode update
sudo bash deploy/install.sh --mode repair
sudo bash deploy/install.sh --mode security
```

The installer:

- installs Docker Engine + Compose from official repos when missing
- creates directories, networks, Caddy + panel + Databasus stack
- generates secrets into `/opt/pgpanel/.env` (mode **0600**, never regenerated on update)
- optional UFW (SSH first), S3 storage probe, notifications
- installs management CLI: `/usr/local/bin/pgpanel`
- does **not** mark the system production-ready without backup/restore verification

Docs:

- [docs/installation.md](docs/installation.md)
- [docs/security-install.md](docs/security-install.md)
- [docs/upgrade-rollback.md](docs/upgrade-rollback.md)

Manual Compose (lab):

```bash
cp .env.example .env   # fill secrets, chmod 0600
cd deploy
docker compose -f compose.yml up -d --build
```

## Environment

| Variable | Purpose |
|----------|---------|
| `PGPANEL_MASTER_KEY` | AES key material for credential encryption (**required**) |
| `PGPANEL_BOOTSTRAP_TOKEN` | Optional one-time setup gate |
| `PGPANEL_COOKIE_SECURE` | Secure cookies (true in prod) |
| `PGPANEL_DATA_DIR` | SQLite + panel data |
| `DATABASUS_BASE_URL` | Internal Databasus URL |
| `DATABASUS_INTERNAL_TOKEN` | Shared secret for Databasus adapter |
| `DOCKER_HOST` | Default `unix:///var/run/docker.sock` |
| `PGPANEL_SQL_ADMIN_MODE` | Enable write SQL console (default false) |

## API (selected)

| Method | Path |
|--------|------|
| POST | `/api/auth/bootstrap` |
| POST | `/api/auth/login` |
| POST | `/api/auth/logout` |
| GET | `/api/me` |
| GET/POST | `/api/clusters` |
| POST | `/api/clusters/{id}/start\|stop\|restart` |
| DELETE | `/api/clusters/{id}` |
| GET/POST | `/api/clusters/{id}/databases` |
| PUT | `/api/clusters/{id}/roles/{role}/password` |
| GET | `/api/clusters/{id}/schemas` / `tables` / rows |
| POST | `/api/clusters/{id}/query` |
| GET/POST | `/api/clusters/{id}/backup` … |
| GET | `/api/operations` / `{id}/events` (SSE) |
| GET | `/api/audit-logs` |
| GET | `/health` `/ready` |

## Cluster delete modes

1. `remove_from_panel` — inventory only  
2. `delete_container_keep_volume` — remove container/network, **keep volume**  
3. `permanently_delete` — requires exact name + `confirm_volume_delete`

Volumes are **never** removed on simple container delete.

## Databasus

PgPanel does **not** reimplement backup/WAL/PITR.

Adapters:

- `MockDatabasusAdapter` — tests  
- `ManualDatabasusAdapter` — connection details + `PendingManualSetup`  
- `HttpDatabasusAdapter` — best-effort HTTP; **endpoint paths are placeholders** until verified against your Databasus version  

If registration fails, the cluster stays up as `healthy_with_backup_warning` with a Retry action.

## Recovery notes

- Panel SQLite lives on `panel_data` volume / `PGPANEL_DATA_DIR`. Back it up separately.
- Cluster data lives on Docker volumes `pgpanel_vol_<slug>`.
- If the panel container is lost but volumes remain, restore the panel image + `.env` master key + SQLite file to regain inventory and encrypted credentials.
- Without the master key, stored PostgreSQL passwords cannot be decrypted (rotate via superuser if you still have host/Docker access).

## Security

Read **[SECURITY.md](SECURITY.md)** and **[docs/threat-model.md](docs/threat-model.md)** before exposing to the internet.

Key residual risk: **Docker socket mounted into the panel container**.

## License

MIT
