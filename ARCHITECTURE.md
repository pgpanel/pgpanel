# Architecture

PgPanel is a multi-process Rust application designed for native deployment on Ubuntu Server 24.04 without container runtimes.

## Components

### pgpanel-web

- **User:** `pgpanel`
- **Bind:** `127.0.0.1:8081` (blue) or `127.0.0.1:8082` (green)
- **Stack:** Axum, Askama templates, HTMX, embedded static assets
- **Storage:** SQLite at `/var/lib/pgpanel/pgpanel.db`
- **Responsibilities:** HTTP UI, authentication, authorization, audit log, SQL editor proxy, monitoring samples

Communicates with `pgpanel-helper` exclusively via a Unix domain socket at `/run/pgpanel/helper.sock`.

### pgpanel-helper

- **User:** `root` (hardened systemd unit)
- **Bind:** Unix socket only
- **Responsibilities:** Invoke `postgresql-common` tools, read/write cluster configuration within policy, stream logs, emit audit events

The helper validates peer UID (must match `pgpanel`), enforces operation timeouts, and rejects any request outside the typed protocol.

### pgpanel-updater

- **Invocation:** CLI (`check`, `update`, `rollback`, `status`) and optional systemd timer
- **Responsibilities:** Download GitHub releases, verify signatures, extract to `/opt/pgpanel/releases/<version>`, run migrations, start inactive slot, health-check, switch Caddy upstream, drain and stop old slot

### Caddy

- **Bind:** `127.0.0.1:8080`
- **Config:** `/etc/caddy/Caddyfile` imports `/etc/caddy/pgpanel-upstream.caddy`
- **Role:** Reverse proxy to the active blue or green slot; health-aware upstream selection

## Directory layout

```
/opt/pgpanel/
  releases/<version>/bin/{pgpanel-web,pgpanel-helper,pgpanel-updater}
  current -> releases/<active>
  previous -> releases/<prior>
  state/{staging,backups,slots,update.lock}

/etc/pgpanel/
  pgpanel.toml
  signing.pub

/var/lib/pgpanel/
  pgpanel.db

/run/pgpanel/
  helper.sock
```

State is **never** stored inside versioned release directories.

## Blue-green deployment

```mermaid
sequenceDiagram
    participant U as pgpanel-updater
    participant I as Inactive slot
    participant C as Caddy
    participant A as Active slot

    U->>U: Download & verify release
    U->>I: Start on opposite port
    U->>I: /health/live + /health/ready
    U->>C: Atomic upstream switch + reload
    U->>A: Drain period
    U->>A: Stop old slot
    U->>U: Update current/previous symlinks
```

Rollback reverses the Caddy upstream to the previous slot after health validation.

## Request flow

```mermaid
sequenceDiagram
    participant B as Browser
    participant Ca as Caddy
    participant W as pgpanel-web
    participant H as pgpanel-helper
    participant P as PostgreSQL

    B->>Ca: HTTPS (via tunnel)
    Ca->>W: HTTP 127.0.0.1
    W->>W: AuthZ + CSRF check
    W->>H: Typed RPC over Unix socket
    H->>P: pg_ctlcluster / SQL (policy-bound)
    H-->>W: Structured response
    W-->>B: HTML / JSON
```

## Configuration

Single TOML file: `/etc/pgpanel/pgpanel.toml`. See [config/pgpanel.toml.example](config/pgpanel.toml.example).

Environment overrides:

| Variable | Overrides |
|----------|-----------|
| `PGPANEL_SECRET_KEY` | `app.secret_key` |
| `PGPANEL_SLOT` | `app.slot` |
| `PGPANEL_LISTEN` | `server.listen` |
| `PGPANEL_DATABASUS_API_KEY` | `databasus.api_key` |
| `PGPANEL_CONFIG` | Config file path (CLI) |

## Frontend build

Tailwind CSS is compiled at build time via Node.js. The compiled `static/css/app.css` is embedded or packaged in release archives. **Node is not required on production servers.**

## Health endpoints

| Endpoint | Purpose |
|----------|---------|
| `GET /health/live` | Process alive |
| `GET /health/ready` | SQLite + migrations + helper reachable |
| `GET /health/version` | Version, slot, architecture |

## Observability

- Structured JSON logs via journald (`SyslogIdentifier=pgpanel-*`)
- Append-only audit log in SQLite with optional hash chaining
- `scripts/diagnostics.sh` for support bundles (secrets redacted)

## Related documents

- [THREAT_MODEL.md](THREAT_MODEL.md)
- [docs/postgresql-permissions.md](docs/postgresql-permissions.md)
- [docs/cloudflare-tunnel.md](docs/cloudflare-tunnel.md)
