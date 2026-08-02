# PgPanel installation guide

## Overview

The production installer (`deploy/install.sh`) is an interactive Bash script that sets up:

- Docker Engine + Compose plugin
- PgPanel (API + static frontend in one container)
- Caddy reverse proxy (HTTPS)
- Databasus (backup service, separate container)
- Networks, volumes, host directories
- Optional UFW firewall, swap, sysctl
- Management CLI: `pgpanel`

**Target:** Linux VPS (multi-distro). Primary test matrix: Ubuntu 22.04 / 24.04, Debian 12. Also supports RHEL-family, Fedora, Amazon Linux 2023, openSUSE, Arch (best-effort).

## Quick start

```bash
# From a git clone on the VPS:
sudo bash deploy/install.sh
```

Menu:

1. Új telepítés  
2. Frissítés  
3. Repair  
4. Configure  
5. Security check  
6. Backup-integration test  
7. Uninstall  
8. Exit  

Non-interactive (uses conf/defaults where possible):

```bash
sudo bash deploy/install.sh --mode install --non-interactive
```

## Minimum requirements

| Resource | Minimum | Recommended |
|----------|---------|-------------|
| CPU | 2 | 4 |
| RAM | 4 GB | 8 GB |
| Disk | 30 GB free | 80 GB NVMe |

Below-minimum hosts require explicit confirmation.

## Default paths

| Path | Purpose |
|------|---------|
| `/opt/pgpanel` | Application install |
| `/var/lib/pgpanel` | Panel SQLite, Databasus data |
| `/var/lib/pgpanel/clusters` | Host path hint for cluster data |
| `/var/log/pgpanel` | Installer logs |
| `/etc/pgpanel/installer.conf` | Non-secret installer config |
| `/opt/pgpanel/.env` | **Secrets** (mode `0600`) |
| `/usr/local/bin/pgpanel` | Management CLI |

Cluster data must **never** live inside the git working tree.

## Domain & TLS

- Preferred: real domain + Let's Encrypt via Caddy  
- Without domain: HTTP-only allowed with strong warning (not production-safe)  
- Cloudflare proxy: set the installer flag; ensure DNS/SSL mode compatible with Caddy  

## Databasus & backups

PgPanel does **not** reimplement WAL/PITR. Databasus runs as a sibling container.

The installer:

- generates internal tokens in `.env`
- optionally probes S3-compatible storage (list/write/delete)
- does **not** invent undocumented Databasus HTTP APIs

If automatic registration is unavailable, status is **Pending manual setup** — complete storage/backup policy in the Databasus UI using credentials from PgPanel.

**Do not mark the deployment production-ready** until backup **and** restore verification succeed.

## Management CLI

```bash
pgpanel status
pgpanel start|stop|restart
pgpanel logs [service]
pgpanel update
pgpanel repair
pgpanel configure
pgpanel backup-test
pgpanel security-check
pgpanel uninstall
pgpanel version
pgpanel env-check
```

## Logging

- Installer: `/var/log/pgpanel/installer.log` (secrets masked)  
- Summary (redacted): `/var/log/pgpanel/install-summary.txt`  
- Container logs: `pgpanel logs`  

## Security notes

See [SECURITY.md](../SECURITY.md), [threat-model.md](threat-model.md), and [security-install.md](security-install.md).

Critical residual risk: **Docker socket mounted into the panel container**.

## Uninstall

```bash
sudo pgpanel uninstall
```

Modes range from stop containers only → full wipe. Full data wipe requires typing `DELETE ALL DATA`. External S3 backups are never deleted by the installer.
