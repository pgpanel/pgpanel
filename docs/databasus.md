# Databasus Integration

PgPanel can optionally probe [Databasus](https://github.com/databasus/databasus) health. PgPanel does **not** implement a backup engine and does **not** install Databasus.

The PgPanel installer does not deploy or configure Databasus. You must install and operate Databasus separately if you want it.

## What PgPanel supports today

| Capability | Status |
|------------|--------|
| Health probe (`GET /api/v1/system/health`) | Supported when enabled and `base_url` is set |
| Backup job listing | **Not supported** — requires `database_id`; PgPanel has no mapping |
| Trigger backup | **Not supported** — not called; no invented API |
| Restore verification | **Not supported** |

No backup operations are currently exposed in the PgPanel UI or HTTP routes. A future explicit adapter would be required before listing or triggering backups.

PgPanel remains fully functional when Databasus is disabled, misconfigured, or unreachable.

## Documented public health API

Databasus exposes:

- **Endpoint:** `GET /api/v1/system/health`
- **Auth:** unauthenticated
- **Responses:** `200` healthy, `503` degraded

PgPanel joins the configured `base_url` with `/api/v1/system/health`, enforces `http`/`https`, applies `timeout_secs` and TLS verification (`tls_verify`), and caps response size. The optional API key is **not** sent to this endpoint.

## Configuration

Edit `/etc/pgpanel/pgpanel.toml`:

```toml
[databasus]
enabled = true
base_url = "https://databasus.internal.example"
timeout_secs = 10
tls_verify = true
```

An API key is optional and unused for the health probe. If you set one for a future adapter, prefer the environment variable (never logged by PgPanel):

```bash
PGPANEL_DATABASUS_API_KEY=your-api-key-here
```

Or in config (less secure):

```toml
api_key = "your-api-key-here"
```

Reload PgPanel after changes.

## UI

When you open **Backups**, the page shows:

- Whether integration is enabled
- Whether it is configured (`base_url` present)
- Health: healthy / degraded / unavailable
- Job listing, trigger, and restore verification labeled **unsupported** unless a future adapter is added

## TLS

Keep `tls_verify = true` in production. Disable only for local development with self-signed certificates.

## Permissions

Viewing the Backups page requires panel read permissions for clusters. Managing Databasus itself is out of scope — use the Databasus UI or API directly.

## Network

Ensure the PgPanel host can reach `base_url` over HTTP(S). No inbound connection from Databasus to PgPanel is required.

## Troubleshooting

| Issue | Action |
|-------|--------|
| Unavailable | Verify `base_url`, firewall, and that Databasus is running |
| Unexpected scheme errors | Use `http://` or `https://` only |
| TLS errors | Verify certificate or set `tls_verify = false` for local dev only |
| Missing job list / trigger | Expected — not supported without a future adapter |

## Related

- [BACKUP.md](../BACKUP.md) — general backup strategy (PostgreSQL and PgPanel state)
- [config/pgpanel.toml.example](../config/pgpanel.toml.example)
