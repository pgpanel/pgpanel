# PgPanel threat model (MVP)

## Assets

1. PostgreSQL cluster data (highest).
2. Panel SQLite state (inventory, encrypted credentials, sessions, audit).
3. Master encryption key (`PGPANEL_MASTER_KEY`).
4. Docker host (via socket).
5. Admin browser session.

## Trust boundaries

```
Internet
   │
   ▼
Cloudflare edge (TLS) ──► cloudflared ──► Caddy (HTTP origin)
                                      │
                                      ├── Panel :8080
                                      └── Databasus :4005
Panel (Docker socket) ────────────────► PG containers + volumes
Databasus ────────────────────────────► PG containers (management network)
```

- In direct mode Caddy is the public listener; in Tunnel mode no host 80/443
  binding is created and `cloudflared` is the outbound-only edge connector.
- Panel talks to PostgreSQL on the shared management network (`pgpanel_database_management`)
  where clusters are dual-homed as `pgpanel_pg_<slug>` (optional published ports only if opted in).
- Databasus is a manually configured sidecar, reachable on port 4005 through the
  internal network or its optional Caddy/Tunnel hostname, and must not receive the Docker socket.
- The Cloudflare Tunnel token is stored separately in `/etc/pgpanel/cloudflare-tunnel.env`
  with mode 0600; the installer does not manage Cloudflare DNS or remote ingress rules.

## Adversaries

| Adversary | Goal |
|-----------|------|
| Unauthenticated internet attacker | Account takeover, RCE via panel, Docker escape |
| Authenticated malicious admin | Destroy data, exfiltrate credentials |
| Compromised Databasus | Read backup credentials / lateral movement |
| Supply-chain (malicious image) | Code execution as postgres/container |

## Controls

| Threat | Control |
|--------|---------|
| Arbitrary Docker abuse | Allowlisted images only; fixed mounts/env |
| SQL injection (DDL names) | Strict regex + identifier quoting |
| SQL console write/abuse | Parser + READ ONLY txn + timeouts |
| Credential theft at rest | AES-256-GCM; master key outside DB |
| Session theft | HttpOnly Secure cookie; CSRF; server sessions |
| Brute force login | Lockout after N failures |
| Accidental data loss | Delete protection; confirm name; volume not auto-deleted |
| Backup logic bugs | Native PgPanel backup engine is independently verified; optional Databasus runs as a separate sidecar |
| Panel DB on managed PG | SQLite local to panel so cluster failure ≠ panel failure |

## Out of scope (future)

- Multi-tenant RBAC / multiple admin roles with least privilege
- Hardware security modules for master key
- Automated CVE scanning pipeline (recommended for ops)

## In scope (current)

- In-app WAF policy + change history (`waf_policies` / `waf_change_log`)
- Multi-node Docker hosts (`nodes` table; local + remote)
- Native backup schedules, restore, retention prune
- Optional Databasus manual backup and PITR workflows


## Residual risks

See SECURITY.md table. Highest priority follow-ups:

1. Split Docker provisioner from HTTP API.
2. Keep Databasus cluster registration manual until its public API contract is
   verified against a pinned version; perform an explicit backup/restore test.
3. Optional: rootless Docker + user namespace remapping.
