# Installation Guide

PgPanel installs on **Ubuntu Server 24.04 LTS** (amd64 or arm64) without Docker. PostgreSQL may already be installed; the installer does not modify existing clusters.

## Prerequisites

- Root access
- Outbound HTTPS (GitHub releases, Caddy repository)
- A trusted Ed25519 release public key (see [Signing key](#signing-key))
- `postgresql-common` installed (installer pulls it if missing)
- Loopback access or Cloudflare Tunnel for the web UI

## Automated install

The installer is safe to run via a downloaded script (no git checkout required). It downloads the GitHub release bundle, verifies checksums and the Ed25519 signature with a **pinned** public key, validates the archive, then installs packaging assets from the extracted `pgpanel-<version>/` tree.

```bash
curl -fsSL -o install.sh \
  "https://raw.githubusercontent.com/pgpanel/pgpanel/vX.Y.Z/scripts/install.sh"
sudo bash install.sh --version vX.Y.Z --signing-key /path/to/signing.pub
```

Or from a release checkout after configuring a key:

```bash
sudo ./scripts/install.sh --version v0.1.0 --signing-key ./keys/signing.pub
```

### Options

| Flag | Description |
|------|-------------|
| `--version vX.Y.Z` | Install a specific GitHub release tag |
| `--from-local PATH` | Install from a local `.tar.gz` (requires `SHA256SUMS`, `SHA256SUMS.sig`, and `manifest.json` in the same directory) |
| `--skip-release` | Configure systemd/Caddy only (binaries must exist under `/opt/pgpanel/current`) |
| `--signing-key PATH` | Pinned Ed25519 public key file (raw 32-byte hex, 64 characters) |
| `--allow-unsupported-os` | Allow non-Ubuntu-24.04 (unsupported; fails closed otherwise) |
| `--dry-run` | Print actions without executing |

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PGPANEL_GITHUB_OWNER` | `pgpanel` | Release repository owner |
| `PGPANEL_GITHUB_REPO` | `pgpanel` | Release repository name |
| `PGPANEL_CHANNEL` | `stable` | `stable` or `prerelease` |
| `PGPANEL_SIGNING_PUBLIC_KEY_HEX` | _(unset)_ | Raw 32-byte Ed25519 public key as hex |

### Signing key

Bootstrap verification is **fail-closed**. The installer never trusts `keys/signing.pub.example`, a key shipped only inside the tarball, or an empty embedded constant.

Provide trust via one of:

1. `PGPANEL_RELEASE_PUBLIC_KEY_HEX` embedded in `scripts/install.sh` (set to a real 64-char hex value before publishing installers)
2. `--signing-key PATH` pointing at a hex public key file
3. `PGPANEL_SIGNING_PUBLIC_KEY_HEX=<64-hex-chars>`

The same pinned key is written to `/etc/pgpanel/signing.pub` on first install (existing files are preserved).

## What the installer does

1. Enforces Ubuntu 24.04 (unless `--allow-unsupported-os`) and detects architecture
2. Installs dependencies: `caddy`, `postgresql-common`, `jq`, `xxd`, `sqlite3`, `openssl`
3. Creates `pgpanel` system user and group
4. Creates `/opt/pgpanel`, `/etc/pgpanel`, `/var/lib/pgpanel`, `/run/pgpanel`
5. Downloads `pgpanel-linux-{amd64,arm64}.tar.gz`, `SHA256SUMS`, `SHA256SUMS.sig`, and `manifest.json` over HTTPS with size limits
6. Verifies the Ed25519 signature over `SHA256SUMS`, then the artifact SHA-256, then `manifest.json`
7. Validates archive members (no absolute paths, `..` traversal, symlinks, or hardlinks)
8. Extracts the single `pgpanel-<version>/` root and installs it under `/opt/pgpanel/releases/<version>/`
9. Creates `/opt/pgpanel/slots/{blue,green}` and points `current` at the blue slot when appropriate
10. Generates `/etc/pgpanel/pgpanel.toml` only if missing (preserves existing config/state)
11. Installs Caddy and systemd units from the verified release packaging tree
12. Starts `pgpanel-helper` and `pgpanel-blue`, then Caddy, and checks `http://127.0.0.1:8080`

**Databasus is not installed.** Backup integration is optional and external; see [docs/databasus.md](docs/databasus.md).

PgPanel is **not** bound to a public interface and **no firewall ports are opened**. Re-runs preserve existing configuration, state, and PostgreSQL clusters.

## First login

1. Open `http://127.0.0.1:8080` (or your tunnel hostname).
2. Complete the first-run wizard: create the owner account, set a strong password, enable TOTP.
3. Review `/etc/pgpanel/pgpanel.toml` and adjust PostgreSQL version allowlists if needed.

## Manual configuration

If you need to recreate config from a verified release tree:

```bash
sudo cp /opt/pgpanel/current/config/pgpanel.toml.example /etc/pgpanel/pgpanel.toml
sudo chmod 640 /etc/pgpanel/pgpanel.toml
sudo chown root:pgpanel /etc/pgpanel/pgpanel.toml
```

Generate a secret key:

```bash
openssl rand -base64 48 | tr -d '/+=' | head -c 48
```

## systemd services

| Unit | Description |
|------|-------------|
| `pgpanel-helper.service` | Privileged helper (always running) |
| `pgpanel-blue.service` | Web slot blue (`:8081`) |
| `pgpanel-green.service` | Web slot green (`:8082`, started during updates) |
| `pgpanel-update-check.timer` | Daily update availability check |

```bash
sudo systemctl status pgpanel-helper pgpanel-blue caddy
```

## Caddy

- Main config: `/etc/caddy/Caddyfile`
- Upstream snippet (managed by updater): `/etc/caddy/pgpanel-upstream.caddy`

Reload after manual edits:

```bash
sudo systemctl reload caddy
```

## Cloudflare Tunnel

See [docs/cloudflare-tunnel.md](docs/cloudflare-tunnel.md). Tunnel target:

```
http://127.0.0.1:8080
```

## Uninstall

```bash
sudo ./scripts/uninstall.sh
```

PostgreSQL is **never** removed. To delete PgPanel state:

```bash
sudo ./scripts/uninstall.sh --purge-state
```

## Troubleshooting

```bash
sudo ./scripts/diagnostics.sh
journalctl -u pgpanel-blue -u pgpanel-helper -f
curl -s http://127.0.0.1:8080/health/ready
```

Common install failures:

- Missing signing key → configure `--signing-key` / `PGPANEL_SIGNING_PUBLIC_KEY_HEX` / embedded `PGPANEL_RELEASE_PUBLIC_KEY_HEX`
- Signature or checksum failure → refuse the release; do not extract
- Wrong Ubuntu version → use 24.04 or pass `--allow-unsupported-os` knowingly

See [docs/recovery.md](docs/recovery.md) for failure scenarios.
