# Installation Guide

PgPanel installs natively on **Ubuntu Server 24.04 LTS** (amd64 or arm64). PostgreSQL may already be installed; the installer does not modify existing clusters. Docker is used only when the optional Databasus installation is selected.

## Prerequisites

- Root access
- Outbound HTTPS (GitHub releases, Caddy repository)
- A trusted Ed25519 release public key (see [Signing key](#signing-key))
- `postgresql-common` installed (installer pulls it if missing)
- A choice of local-only, Cloudflare Tunnel, or public HTTPS exposure

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
| `--repair` | Reinstall/repair the selected verified release, packaging, symlinks, Caddy, and systemd services |
| `--exposure local\|cloudflare\|public` | Select Caddy/exposure setup; noninteractive default is `local` |
| `--hostname HOST` | DNS hostname required by `--exposure public` |
| `--cloudflare-token TOKEN` | Remotely managed tunnel token; prefer the environment variable because command arguments can be visible |
| `--with-databasus` | Install pinned Databasus using Docker Engine and Compose |
| `--without-databasus` | Explicitly skip Databasus and disable its PgPanel health probe |
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
| `PGPANEL_CLOUDFLARE_TUNNEL_TOKEN` | _(unset)_ | Remotely managed Tunnel token; read only during service installation |

When stdin is a TTY and these choices are omitted, the installer prompts for exposure and whether to install Databasus. Tunnel-token input is silent. A noninteractive run defaults to local exposure and does not install Databasus.

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
8. Extracts the single `pgpanel-<version>/` root and atomically installs or repairs it under `/opt/pgpanel/releases/<version>/`
9. Repairs `/opt/pgpanel/slots/{blue,green}`, `/opt/pgpanel/slots/blue/current`, and `/opt/pgpanel/current`
10. Generates `/etc/pgpanel/pgpanel.toml` only if missing (preserves existing config/state)
11. Installs Caddy and systemd units from the verified release packaging tree
12. Applies the selected exposure mode and optional Databasus setup
13. Restarts `pgpanel-helper`, `pgpanel-updater`, `pgpanel-blue`, and Caddy, then checks `http://127.0.0.1:8080`

Re-running the installer performs the same repair behavior as `--repair`: verified release files and packaging are reinstalled, incomplete release trees and links are repaired, and services are restarted. Existing `/etc/pgpanel` configuration, `/var/lib/pgpanel` state, and PostgreSQL clusters are preserved except for settings explicitly selected on the command line.

The installer does not install a PostgreSQL server major version or alter
existing clusters. Install the server package for every version you want to
create, for example:

```bash
sudo apt-get update
sudo apt-get install -y postgresql-16
```

Cluster creation requires the selected version's `initdb` binary. If it is
missing, PgPanel reports the exact package command instead of only returning
`pg_createcluster failed`.

The default authenticated API limit is 600 requests per IP per minute. Login
attempts remain limited separately to 10 per five minutes. Health checks and
static assets are exempt so Caddy and the browser cannot exhaust the API
bucket.

## Exposure modes

### Local

`--exposure local` keeps Caddy on `127.0.0.1:8080`. It opens no firewall ports and is the safe noninteractive default. Switching to local mode disables secure-only cookies, removes a stale public base URL, and disables direct trust of `CF-Connecting-IP`.

### Cloudflare Tunnel

`--exposure cloudflare` keeps Caddy loopback-only, installs `cloudflared` from Cloudflare's official stable apt repository, and runs a remotely managed tunnel as a system service. Create the tunnel and public hostname route in the Cloudflare dashboard. The origin service must be:

```
http://127.0.0.1:8080
```

Pass the token with `PGPANEL_CLOUDFLARE_TUNNEL_TOKEN` where possible. PgPanel does not log or save it; Cloudflare's official `cloudflared service install` manages the service credentials. Cloudflare mode removes any stale `server.public_base_url` and trusts forwarded addresses only from the loopback proxy, not arbitrary `CF-Connecting-IP` headers. Cloudflare Access with MFA is strongly recommended.

The installer marks successfully installed services with `/etc/pgpanel/cloudflared-managed`. It never alters a pre-existing unmarked `cloudflared.service`; Cloudflare mode fails with cleanup guidance instead. Switching to local/public mode removes cloudflared only when this marker proves PgPanel owns it.

### Public HTTPS

`--exposure public --hostname panel.example.com` configures Caddy automatic HTTPS for that exact DNS hostname, sets `server.public_base_url` and secure session cookies, and disables direct trust of `CF-Connecting-IP`. DNS must already resolve to the server and inbound TCP 80/443 must reach it. If UFW is active, the installer allows only `80/tcp` and `443/tcp`; it does not expose a raw public HTTP mode. PgPanel and blue/green slots remain loopback-only.

## Optional Databasus

`--with-databasus` installs Docker Engine plus the Compose plugin from Docker's official Ubuntu apt repository if needed. It writes `/opt/databasus/docker-compose.yml`, pins `databasus/databasus:v3.51.0`, binds only `127.0.0.1:4005`, creates persistent Docker volume `databasus-data`, starts it, and waits for health. No remote installer is piped to a shell.

The selected configuration enables PgPanel's documented `GET /api/v1/system/health` probe. Backup jobs and restores remain managed in Databasus's own UI. See [docs/databasus.md](docs/databasus.md).

## First login

1. Open `http://127.0.0.1:8080`, the tunnel hostname, or the configured public HTTPS hostname.
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
- Service/health failure → the installer prints concise `systemctl status` and journal excerpts for helper, updater, blue, and Caddy before exiting
- Cloudflare Tunnel failure → verify the remotely managed tunnel token and dashboard origin route
- Databasus failure → inspect `docker compose -f /opt/databasus/docker-compose.yml ps` and `docker logs databasus`

See [docs/recovery.md](docs/recovery.md) for failure scenarios.
