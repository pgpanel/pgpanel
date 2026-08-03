# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

Security fixes are released as patch versions on the `stable` channel.

## Reporting a vulnerability

**Do not open public GitHub issues for security vulnerabilities.**

Email security reports to the maintainers (configure `SECURITY_CONTACT` in your fork) with:

- Description and impact
- Steps to reproduce
- Affected version
- Suggested fix (if any)

We aim to acknowledge reports within 3 business days.

## Security design principles

### Privilege separation

- `pgpanel-web` runs as the unprivileged `pgpanel` user.
- `pgpanel-helper` runs as root but accepts only typed operations over a Unix socket with peer credential validation.
- The web process never invokes shell commands or `sudo`.

### Network exposure

- Application slots bind to `127.0.0.1` only.
- Caddy listens on `127.0.0.1:8080`.
- External access should use Cloudflare Tunnel or an equivalent private ingress — not a public bind.

### Authentication

- Passwords hashed with Argon2id.
- Sessions use HttpOnly cookies with configurable SameSite and Secure flags.
- TOTP two-factor authentication with hashed backup codes.
- Rate limiting and account lockout on failed logins.
- Recent re-authentication required for destructive operations.

### Updates

- Release artifacts verified with SHA-256 checksums and Ed25519 signatures.
- Public signing key pinned at `/etc/pgpanel/signing.pub`.
- Blue-green deployment validates health before traffic switch.

### PostgreSQL operations

- Cluster identifiers validated against `^[a-z][a-z0-9_]{1,31}$`.
- Commands use absolute paths to `postgresql-common` binaries with separate arguments — no shell interpolation.
- SQL identifiers quoted through a tested quoting function; parameters bound where supported.

## Hardening checklist

- [ ] PgPanel reachable only via tunnel/VPN with Access policy
- [ ] `secret_key` rotated from installer default
- [ ] TOTP enabled for all administrator accounts
- [ ] `allow_superuser_creation = false` unless explicitly required
- [ ] Audit log reviewed regularly
- [ ] Automatic updates disabled unless a maintenance window is defined
- [ ] Signing public key matches your trusted release channel
- [ ] PostgreSQL not listening on public interfaces

## Secrets handling

Never commit or log:

- `secret_key` / `PGPANEL_SECRET_KEY`
- Session tokens
- TOTP secrets or backup codes
- `PGPANEL_DATABASUS_API_KEY`
- Release signing private keys

Environment variable overrides are supported for secrets in production.

## systemd hardening

Production units apply `ProtectSystem`, `NoNewPrivileges`, `MemoryDenyWriteExecute` (web), restricted `ReadWritePaths`, and empty capability sets where compatible. See `packaging/systemd/`.

## Dependency security

CI runs `cargo audit` and `cargo deny` on every push and weekly. Review [deny.toml](deny.toml) for license and advisory policy.

## Incident response

1. Stop exposure (disable tunnel route or stop Caddy).
2. Preserve audit logs and journald output: `sudo ./scripts/diagnostics.sh /tmp/pgpanel-incident`
3. Roll back if compromise suspected: `sudo ./scripts/rollback.sh`
4. Rotate `secret_key`, session invalidation, and user passwords.
5. Report and document the incident.

See [docs/recovery.md](docs/recovery.md) for disaster recovery.
