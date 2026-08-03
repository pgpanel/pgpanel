#!/usr/bin/env bash
# Wrapper for pgpanel-updater rollback command.
set -euo pipefail

readonly CONFIG="/etc/pgpanel/pgpanel.toml"
readonly UPDATER="/opt/pgpanel/current/bin/pgpanel-updater"

usage() {
    cat <<'EOF'
Usage: rollback.sh

Roll back PgPanel to the previous release using blue-green deployment.

Switches Caddy upstream back to the previous slot after health checks.
Requires root.
EOF
}

main() {
    if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
        usage
        exit 0
    fi

    if [[ "${EUID}" -ne 0 ]]; then
        echo "rollback.sh must be run as root" >&2
        exit 1
    fi

    if [[ ! -x "${UPDATER}" ]]; then
        echo "pgpanel-updater not found at ${UPDATER}" >&2
        exit 1
    fi

    exec "${UPDATER}" --config "${CONFIG}" rollback
}

main "$@"
