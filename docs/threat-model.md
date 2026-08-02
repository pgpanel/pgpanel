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
 Caddy (TLS terminate) ──────── edge network
   │
   ▼
 Panel :8080 ────────────────── internal network
   │         │
   │         ├── Databasus (backup only)
   │         └── Docker Engine API (socket) ──► PG containers + volumes
```

- Caddy is the only public listener.
- Panel talks to PostgreSQL on private Docker networks (or optional published ports).
- Databasus must not receive the Docker socket.

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
| Backup logic bugs | Outsourced to Databasus; panel only orchestrates |
| Panel DB on managed PG | SQLite local to panel so cluster failure ≠ panel failure |

## Out of scope for MVP

- Multi-tenant RBAC / multiple admin roles
- Full WAF ruleset
- Hardware security modules for master key
- Automated CVE scanning pipeline (recommended for ops)

## Residual risks

See SECURITY.md table. Highest priority follow-ups:

1. Split Docker provisioner from HTTP API.
2. Verify Databasus API against a pinned version; integration tests for backup/restore.
3. Optional: rootless Docker + user namespace remapping.
