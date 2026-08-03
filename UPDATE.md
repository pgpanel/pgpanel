# Updates

PgPanel updates use **blue-green deployment** with signed GitHub release artifacts. PostgreSQL clusters and `cloudflared` are not restarted during updates.

## Release channels

Configure in `/etc/pgpanel/pgpanel.toml`:

```toml
[updates]
channel = "stable"    # or "prerelease"
auto_install = false  # strongly recommended
```

## Manual update

```bash
sudo ./scripts/update.sh
```

Install a specific version:

```bash
sudo ./scripts/update.sh --version v0.2.0
```

Allow downgrade (use with caution):

```bash
sudo ./scripts/update.sh --version v0.1.0 --allow-downgrade
```

Equivalent direct invocation:

```bash
sudo /opt/pgpanel/current/bin/pgpanel-updater --config /etc/pgpanel/pgpanel.toml update
```

## Update flow

1. Acquire global update lock (`/opt/pgpanel/state/update.lock`)
2. Query GitHub Releases API for the configured channel and architecture
3. Download `pgpanel-linux-{amd64,arm64}.tar.gz`
4. Verify SHA-256 against `SHA256SUMS` and Ed25519 signature
5. Validate `manifest.json`
6. Backup SQLite database to `/opt/pgpanel/state/backups/`
7. Extract release to `/opt/pgpanel/releases/<version>/`
8. Run database migrations on the inactive slot
9. Start inactive slot (`pgpanel-green` or `pgpanel-blue`)
10. Health-check `/health/live` and `/health/ready`
11. Atomically switch `/etc/caddy/pgpanel-upstream.caddy` and reload Caddy
12. Drain traffic, stop old slot, update `current`/`previous` symlinks
13. Restart helper only if its binary changed

If any step fails **before** the Caddy switch, the old slot remains active.

## Rollback

```bash
sudo ./scripts/rollback.sh
```

This switches Caddy back to the previous slot after health validation and updates symlinks.

## Check for updates

```bash
sudo /opt/pgpanel/current/bin/pgpanel-updater --config /etc/pgpanel/pgpanel.toml check
```

Scheduled check (daily by default):

```bash
systemctl list-timers pgpanel-update-check.timer
```

## Automatic updates

Disabled by default. To enable:

```toml
[updates]
auto_install = true
schedule_window = "Sun 03:00-05:00 UTC"
```

Automatic installation should only be used with a tested rollback procedure and monitoring.

## Release artifacts

Each GitHub release includes:

| File | Purpose |
|------|---------|
| `pgpanel-linux-amd64.tar.gz` | x86_64 binaries and assets |
| `pgpanel-linux-arm64.tar.gz` | aarch64 binaries and assets |
| `SHA256SUMS` | Checksums |
| `SHA256SUMS.sig` | Ed25519 signature (when signing key configured) |
| `manifest.json` | Version metadata and per-arch checksums |

## Signing key

Install the trusted public key at `/etc/pgpanel/signing.pub`. See [keys/README.md](keys/README.md).

## UI update page

The Updates page (`updates.manage`) talks only to the privileged updater daemon over `paths.updater_socket` via `UpdaterClient`. It does not download, extract, or switch slots itself.

The page shows:

- Installed web version, latest available version/tag/date, channel, auto-install, check interval, and schedule window
- Daemon phase, last check, last success/error, active/previous slot and version, and recent progress records
- Check, Install, and Rollback controls

Progress uses short HTML fragment polling (`GET /updates/status-fragment` every few seconds), not SSE. Install and rollback return immediately after the daemon accepts the operation (with an operation id); the update continues after the web slot restarts. Install and rollback require recent re-authentication, CSRF, and an explicit confirmation phrase (`INSTALL` / `ROLLBACK`).

## Status

```bash
sudo /opt/pgpanel/current/bin/pgpanel-updater --config /etc/pgpanel/pgpanel.toml status
```

Example output fields: `active_slot`, `current_version`, `previous_version`, `installed_versions`, `update_in_progress`.

## Failure handling

| Scenario | Behavior |
|----------|----------|
| Download/signature failure | No change; error logged |
| Health failure before switch | New slot stopped; old slot active |
| Health failure after switch | Automatic Caddy rollback to previous slot |
| Helper binary changed | Helper restarted after web validation; rollback on failure |

See [docs/recovery.md](docs/recovery.md).
