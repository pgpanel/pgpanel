# Contributing to PgPanel

Thank you for contributing. PgPanel prioritizes security, correctness, and maintainability.

## Development setup

### Requirements

- Rust 1.85+ (see `rust-toolchain.toml`)
- Node.js 20+ (Tailwind CSS build only)
- Ubuntu 24.04 or compatible Linux for integration tests involving `postgresql-common`

### Clone and build

```bash
git clone https://github.com/pgpanel/pgpanel.git
cd pgpanel
make install-dev
```

### Common commands

```bash
make fmt          # Format Rust code
make clippy       # Lint with warnings denied
make test         # Run workspace tests
make frontend     # Build Tailwind CSS
make release      # Production tarball (local arch)
make audit        # cargo audit
make deny         # cargo deny
```

### Running locally

Terminal 1 — helper (dev mode):

```bash
PGPANEL_CONFIG=./config/pgpanel.toml cargo run -p pgpanel-helper -- --dev
```

Terminal 2 — web (dev mode):

```bash
PGPANEL_CONFIG=./config/pgpanel.toml cargo run -p pgpanel-web -- --dev
```

Dev defaults bind to `127.0.0.1:8081` with SQLite at `./data/pgpanel.db`.

## Code style

- Run `cargo fmt` before committing
- `cargo clippy -- -D warnings` must pass
- No `unwrap()` in request-handling or privileged-operation paths
- Match existing module structure and naming
- Minimal scope — avoid drive-by refactors

## Security-sensitive changes

Changes to the following require extra review:

- `pgpanel-helper` command invocation
- Authentication and session handling
- SQL identifier quoting
- Update signature verification
- systemd hardening directives

Add tests for validation logic and malicious inputs.

## Pull requests

1. Fork and create a feature branch from `main`
2. Include tests for new behavior
3. Update documentation if user-facing behavior changes
4. Ensure CI passes (fmt, clippy, test, audit, deny, frontend build)
5. Write a clear PR description with security impact noted if applicable

## Commit messages

Use imperative mood, focused on *why*:

```
fix helper timeout on large log reads

Add streaming read with configurable line cap to prevent OOM on
multi-gigabyte PostgreSQL logs.
```

## Frontend

- Server-rendered Askama templates with HTMX
- Tailwind utility classes via `static/css/input.css`
- Alpine.js only for small UI state (sidebar, theme, modals)
- Rebuild CSS after template/class changes: `make frontend`

## Testing

```bash
cargo test --workspace
cargo test -p pgpanel-core -- config
cargo test -p pgpanel-updater -- deploy
```

Integration tests may require PostgreSQL installed locally.

## Release process

Maintainers tag `vX.Y.Z` to trigger `.github/workflows/release.yml`. See [keys/README.md](keys/README.md) for signing setup.

## Code of conduct

Be respectful and constructive. Security issues: see [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions are licensed under the Apache-2.0 license.
