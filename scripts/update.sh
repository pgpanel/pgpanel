#!/usr/bin/env bash
# Wrapper for pgpanel-updater update command.
set -euo pipefail

readonly CONFIG="/etc/pgpanel/pgpanel.toml"
readonly UPDATER="/opt/pgpanel/current/bin/pgpanel-updater"

VERSION=""
ALLOW_DOWNGRADE="0"

usage() {
    cat <<'EOF'
Usage: update.sh [OPTIONS]

Download, verify, and deploy a PgPanel update using blue-green deployment.

Options:
  --version VERSION    Install a specific version (default: latest for channel)
  --allow-downgrade    Allow installing an older version
  -h, --help           Show this help

Requires root. Does not restart PostgreSQL or cloudflared.
EOF
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --version)
                VERSION="${2:?--version requires a value}"
                shift 2
                ;;
            --allow-downgrade)
                ALLOW_DOWNGRADE="1"
                shift
                ;;
            -h | --help)
                usage
                exit 0
                ;;
            *)
                echo "unknown argument: $1" >&2
                usage >&2
                exit 1
                ;;
        esac
    done
}

main() {
    parse_args "$@"

    if [[ "${EUID}" -ne 0 ]]; then
        echo "update.sh must be run as root" >&2
        exit 1
    fi

    if [[ ! -x "${UPDATER}" ]]; then
        echo "pgpanel-updater not found at ${UPDATER}" >&2
        exit 1
    fi

    local -a args=(--config "${CONFIG}" update)
    if [[ -n "${VERSION}" ]]; then
        args+=(--version "${VERSION}")
    fi
    if [[ "${ALLOW_DOWNGRADE}" == "1" ]]; then
        args+=(--allow-downgrade)
    fi

    exec "${UPDATER}" "${args[@]}"
}

main "$@"
