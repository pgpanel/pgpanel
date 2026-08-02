# Security policy — PgPanel

## Status

**This MVP is not production-ready** until:

1. Databasus registration, backup, and restore are integration-tested against a real Databasus version.
2. Docker socket access is either hardened (rootless, socket proxy) or split into a separate provisioner service.
3. Penetration testing / threat-model review is completed for your environment.

## Threat model summary

See [docs/threat-model.md](docs/threat-model.md).

## Privileged risk: Docker socket

The panel container mounts `/var/run/docker.sock` to create PostgreSQL containers, volumes, and networks.

**Impact if the panel is compromised:** full control of the Docker host (escape to host depending on daemon config).

**Mitigations in this design:**

- Only allowlisted images: `postgres:16`, `postgres:17`, `postgres:18` (no `:latest`).
- No user-supplied image names, mounts, env vars, or shell commands.
- Internal networks for clusters; public DB port off by default.
- Frontend and Databasus **never** receive the Docker socket.
- Architecture allows a future **provisioner** service so the HTTP API need not hold the socket.

**Operator actions:**

- Prefer rootless Docker or a dedicated docker group with minimal host privileges.
- Network-restrict who can reach the panel.
- Keep panel software updated.
- Monitor audit logs for cluster create/delete and port publish events.

## Authentication & sessions

- First admin is created via one-time bootstrap (optional `PGPANEL_BOOTSTRAP_TOKEN`).
- Admin passwords hashed with **Argon2id**.
- Browser sessions are **server-side** (random token hashed at rest), **not JWT**.
- Cookies: `HttpOnly`, `SameSite=Lax`, `Secure` in production.
- CSRF: double-submit style header `x-csrf-token` required on mutating requests.
- Login lockout after repeated failures.

## Secrets

- PostgreSQL role passwords: generated from ≥24 random bytes; encrypted with **AES-256-GCM** in SQLite.
- Master key: `PGPANEL_MASTER_KEY` from environment (never commit).
- Passwords are not written to logs or put in URLs.
- Plaintext credentials returned only at creation or explicit reveal paths.

## SQL console

- Default **read-only**: `pg_query` parser allowlist + `BEGIN`/`SET TRANSACTION READ ONLY` + timeouts.
- Admin write mode behind feature flag (`PGPANEL_SQL_ADMIN_MODE`) — disabled by default.
- Dangerous functions (e.g. `pg_read_file`) blocked.

## Identifiers

Cluster / database / role names must match `^[a-z][a-z0-9_]{2,62}$` and are quoted safely. Identifiers are never bound as SQL parameters.

## Audit

Dangerous actions are written to `audit_logs` (cluster create/delete, volume delete, DB/role delete, password change, port publish, backup config).

## Reporting

If you discover a vulnerability, contact the repository maintainers privately. Do not open a public issue with exploit details.

## Known residual risks (MVP)

| Risk | Severity | Notes |
|------|----------|--------|
| Docker socket in panel container | Critical | Documented; provisioner split planned |
| Databasus HTTP API paths may not match your version | High | Manual adapter / PendingManualSetup fallback |
| No mTLS between panel and Databasus | Medium | Shared internal Docker network |
| Rate limiting is basic (login lockout only) | Medium | Add reverse-proxy / tower rate limits as needed |
| SQL password interpolation for CREATE ROLE | Medium | Escaped quotes; prefer future SCRAM APIs |
| SSE auth relies on cookie; long-lived connections | Low | Session expiry still enforced on other requests |
