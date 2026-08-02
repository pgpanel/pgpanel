# Security policy — PgPanel

## Status

PgPanel targets production VPS deployments with the following controls in place.
Operators should still review residual risks before exposing the panel to the public internet.

Hardening included in current releases:

1. Native backup engine with restore, retention prune, and encrypted object storage.
2. In-app WAF with change history (rate limits, IP allow/deny, UA/path blocks, Caddy snippet export).
3. Multi-node Docker host registry (local + remote `tcp://` / `unix://` endpoints).
4. CORS locked down by default (localhost only unless `PGPANEL_CORS_ORIGINS` is set).
5. Backup dump/restore paths avoid shell interpolation of secrets (docker cp + `-e PGPASSWORD`).

Still recommended before high-trust production:

1. Harden Docker socket access (rootless Docker or socket proxy / provisioner split).
2. Penetration testing for your threat model.
3. External edge WAF (Cloudflare / CrowdSec) in front of Caddy for volumetric attacks.

## Threat model summary

See [docs/threat-model.md](docs/threat-model.md).

## Privileged risk: Docker socket

The panel container mounts `/var/run/docker.sock` (or a remote Docker API) to create PostgreSQL containers, volumes, and networks.

**Impact if the panel is compromised:** full control of the connected Docker host(s).

**Mitigations in this design:**

- Only allowlisted images: `postgres:16`, `postgres:17`, `postgres:18` (no `:latest`).
- No user-supplied image names, mounts, env vars, or shell commands.
- Internal networks for clusters; public DB port off by default.
- Frontend never receives the Docker socket.
- Remote nodes store Docker host URLs encrypted at rest; only probed after validation.
- Architecture allows a future **provisioner** service so the HTTP API need not hold the socket.

**Operator actions:**

- Prefer rootless Docker or a dedicated docker group with minimal host privileges.
- Network-restrict who can reach the panel.
- Keep panel software updated.
- Monitor audit logs for cluster create/delete and port publish events.
- Review WAF change history after policy edits.

## Authentication & sessions

- First admin is created via one-time bootstrap (optional `PGPANEL_BOOTSTRAP_TOKEN`).
- Admin passwords hashed with **Argon2id**.
- Browser sessions are **server-side** (random token hashed at rest), **not JWT**.
- Cookies: `HttpOnly`, `SameSite=Lax`, `Secure` in production.
- CSRF: double-submit style header `x-csrf-token` required on mutating requests.
- Login lockout after repeated failures.
- In-app rate limiting (login / API / global) via active WAF policy.

## Secrets

- PostgreSQL role passwords: generated from ≥24 random bytes; encrypted with **AES-256-GCM** in SQLite.
- Master key: `PGPANEL_MASTER_KEY` from environment (never commit).
- Passwords are not written to logs or put in URLs.
- Plaintext credentials returned only at creation or explicit reveal paths.
- Object-storage keys encrypted in `settings`.

## SQL console

- Default **read-only**: `pg_query` parser allowlist + `BEGIN`/`SET TRANSACTION READ ONLY` + timeouts.
- Admin write mode behind feature flag (`PGPANEL_SQL_ADMIN_MODE`) — disabled by default.
- Dangerous functions (e.g. `pg_read_file`) blocked.

## Identifiers

Cluster / database / role names must match `^[a-z][a-z0-9_]{2,62}$` and are quoted safely. Identifiers are never bound as SQL parameters.

## Audit

Dangerous actions are written to `audit_logs` (cluster create/delete, volume delete, DB/role delete, password change, port publish, backup config/restore, node CRUD, WAF updates).

WAF policy edits are also recorded in `waf_change_log` with before/after JSON and operator reason.

## Reporting

If you discover a vulnerability, contact the repository maintainers privately. Do not open a public issue with exploit details.

## Known residual risks

| Risk | Severity | Notes |
|------|----------|--------|
| Docker socket in panel container | Critical | Documented; provisioner split planned |
| Remote Docker API without TLS/mTLS | High | Prefer SSH tunnels or TLS-protected Docker API |
| No mTLS between panel and object storage | Medium | Use private networks / IAM where possible |
| In-app rate limit is best-effort (in-memory) | Medium | Pair with Caddy / external WAF |
| SQL password via process env on dump | Medium | Avoided in shell scripts; still visible to host root |
| SSE auth relies on cookie; long-lived connections | Low | Session expiry still enforced on other requests |
