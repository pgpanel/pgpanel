# PgPanel

Self-hosted PostgreSQL control plane for Linux VPS.

**Developer:** Dezső Benedek Péter  
**Repository:** https://github.com/pgpanel/pgpanel  
**Version:** see [`VERSION`](VERSION)

PgPanel provisions and manages isolated PostgreSQL clusters with Docker, a secure admin UI, and backup orchestration via Databasus (backup/WAL/PITR are not reimplemented in the panel).

## Production install (one line)

```bash
sudo apt-get update && sudo apt-get install -y curl && \
curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
```

- Does **not** ask for a Git URL (official repo is fixed).
- Pulls **pre-built** panel images from GHCR (`ghcr.io/pgpanel/pgpanel:<version>`).
- The VPS does **not** need to compile Rust or the frontend (unless image pull fails / `--build-local`).

After install: open `https://YOUR_DOMAIN/setup` — default username `admin`, password shown once by the installer.

## Update (no data loss)

```bash
sudo pgpanel update
sudo pgpanel version
```

Keeps: `.env` secrets, panel SQLite, PostgreSQL volumes, Databasus data.  
Never runs `docker compose down -v` during update.

## Management CLI

```bash
pgpanel status
pgpanel logs
pgpanel update
pgpanel version
pgpanel repair
pgpanel security-check
pgpanel uninstall
```

## Architecture

```
pgpanel/
├── crates/          # Rust workspace (API, Docker, Postgres, jobs, Databasus adapters)
├── frontend/        # SvelteKit + shadcn-svelte (built into the panel image)
├── migrations/      # Panel SQLite
├── deploy/          # compose, Caddy, Dockerfile, installers
├── VERSION
└── install-pgpanel.sh
```

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

## Docs

- [docs/installation.md](docs/installation.md) — install & images  
- [docs/upgrade-rollback.md](docs/upgrade-rollback.md) — updates  
- [docs/getting-started.md](docs/getting-started.md) — admin / first login  
- [SECURITY.md](SECURITY.md) — threat model & residual risks  

## License

MIT · © Dezső Benedek Péter
