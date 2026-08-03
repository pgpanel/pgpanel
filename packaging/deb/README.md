# Debian package metadata (future)

PgPanel is currently installed via `scripts/install.sh`, which downloads prebuilt
release archives from GitHub and configures systemd and Caddy on Ubuntu 24.04 LTS.

## Planned `.deb` packaging

A future Debian package will:

- Install binaries under `/opt/pgpanel/releases/<version>/`
- Ship systemd units from `packaging/systemd/`
- Ship Caddy snippets from `packaging/caddy/`
- Create the `pgpanel` system user and required directories
- Install `/etc/pgpanel/pgpanel.toml` from `config/pgpanel.toml.example` on first install
- Declare dependencies: `caddy`, `postgresql-common`, `adduser`

## Building a `.deb` (manual, experimental)

```bash
# From repository root after a release build:
make release
./packaging/deb/build-deb.sh   # script to be added when .deb support lands
```

## Versioning

Package version tracks the Rust workspace version in the root `Cargo.toml`.
Release tags use the `v` prefix (e.g. `v0.1.0`).

## Maintainer scripts (planned)

| Script            | Purpose                                      |
|-------------------|----------------------------------------------|
| `postinst`        | `systemctl daemon-reload`, enable helper     |
| `prerm`           | Stop PgPanel units before package removal    |
| `postrm`          | Optional cleanup (never touches PostgreSQL)  |

## Policy notes

- PgPanel does not bundle PostgreSQL server packages.
- State lives in `/var/lib/pgpanel` and is preserved across package upgrades.
- Configuration in `/etc/pgpanel` is conffile-managed.

For production installs today, use [INSTALL.md](../INSTALL.md).
