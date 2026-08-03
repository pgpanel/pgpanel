#!/usr/bin/env bash
# Remove PgPanel from the system. Does NOT remove PostgreSQL clusters or data.
set -euo pipefail

readonly INSTALL_ROOT="/opt/pgpanel"
readonly CONFIG_DIR="/etc/pgpanel"
readonly STATE_DIR="/var/lib/pgpanel"
readonly RUN_DIR="/run/pgpanel"
readonly PGPANEL_USER="pgpanel"
readonly PGPANEL_GROUP="pgpanel"

PURGE_STATE="0"
REMOVE_CADDY="0"
DRY_RUN="0"

log() {
    printf '[pgpanel-uninstall] %s\n' "$*"
}

die() {
    printf '[pgpanel-uninstall] ERROR: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: uninstall.sh [OPTIONS]

Remove PgPanel services, binaries, and configuration.

PostgreSQL clusters and databases are NEVER removed by this script.

Options:
  --purge-state   Also delete /var/lib/pgpanel (SQLite, audit log, local state)
  --remove-caddy  Remove PgPanel Caddy configuration (does not uninstall Caddy)
  --dry-run       Print actions without making changes
  -h, --help      Show this help

WARNING: --purge-state permanently deletes PgPanel's internal database and audit log.
EOF
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --purge-state)
                PURGE_STATE="1"
                shift
                ;;
            --remove-caddy)
                REMOVE_CADDY="1"
                shift
                ;;
            --dry-run)
                DRY_RUN="1"
                shift
                ;;
            -h | --help)
                usage
                exit 0
                ;;
            *)
                die "unknown argument: $1"
                ;;
        esac
    done
}

run() {
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] $*"
    else
        "$@"
    fi
}

require_root() {
    if [[ "${EUID}" -ne 0 ]]; then
        die "this uninstaller must be run as root"
    fi
}

stop_services() {
    log "stopping PgPanel services"
    local unit
    for unit in \
        pgpanel-blue.service \
        pgpanel-green.service \
        pgpanel-helper.service \
        pgpanel-update-check.timer \
        pgpanel-update-check.service; do
        run systemctl stop "${unit}" 2>/dev/null || true
        run systemctl disable "${unit}" 2>/dev/null || true
    done
    run systemctl daemon-reload
}

remove_systemd_units() {
    log "removing systemd units"
    local unit
    for unit in \
        pgpanel-blue.service \
        pgpanel-green.service \
        pgpanel-helper.service \
        pgpanel-update-check.service \
        pgpanel-update-check.timer; do
        if [[ -f "/etc/systemd/system/${unit}" ]]; then
            run rm -f "/etc/systemd/system/${unit}"
        fi
    done
    run systemctl daemon-reload
}

remove_caddy_config() {
    if [[ "${REMOVE_CADDY}" != "1" ]]; then
        log "leaving Caddy configuration in place (use --remove-caddy to delete)"
        return 0
    fi
    log "removing PgPanel Caddy snippets"
    run rm -f /etc/caddy/pgpanel-upstream.caddy
    if [[ -f /etc/caddy/Caddyfile ]] && grep -q 'pgpanel-upstream' /etc/caddy/Caddyfile 2>/dev/null; then
        log "restoring default Caddyfile (PgPanel block removed)"
        run rm -f /etc/caddy/Caddyfile
    fi
    run systemctl reload caddy 2>/dev/null || true
}

remove_install_root() {
    if [[ -d "${INSTALL_ROOT}" ]]; then
        log "removing ${INSTALL_ROOT}"
        run rm -rf "${INSTALL_ROOT}"
    fi
}

remove_config() {
    if [[ -d "${CONFIG_DIR}" ]]; then
        log "removing ${CONFIG_DIR}"
        run rm -rf "${CONFIG_DIR}"
    fi
}

remove_state() {
    if [[ "${PURGE_STATE}" != "1" ]]; then
        log "preserving ${STATE_DIR} (use --purge-state to delete)"
        return 0
    fi
    log "purging state directory ${STATE_DIR}"
    run rm -rf "${STATE_DIR}"
}

remove_runtime() {
    if [[ -d "${RUN_DIR}" ]]; then
        run rm -rf "${RUN_DIR}"
    fi
}

remove_user() {
    if getent passwd "${PGPANEL_USER}" >/dev/null 2>&1; then
        log "removing user ${PGPANEL_USER}"
        run userdel "${PGPANEL_USER}" 2>/dev/null || true
    fi
    if getent group "${PGPANEL_GROUP}" >/dev/null 2>&1; then
        log "removing group ${PGPANEL_GROUP}"
        run groupdel "${PGPANEL_GROUP}" 2>/dev/null || true
    fi
}

main() {
    parse_args "$@"
    require_root

    if [[ "${PURGE_STATE}" == "1" ]]; then
        log "WARNING: --purge-state will permanently delete PgPanel state at ${STATE_DIR}"
        sleep 2
    fi

    stop_services
    remove_systemd_units
    remove_caddy_config
    remove_install_root
    remove_config
    remove_state
    remove_runtime
    remove_user

    cat <<EOF

PgPanel has been removed.

PostgreSQL clusters were not modified.

EOF
    if [[ "${PURGE_STATE}" != "1" && -d "${STATE_DIR}" ]]; then
        printf 'State preserved at: %s\n' "${STATE_DIR}"
        printf 'Re-run with --purge-state to delete it.\n'
    fi
}

main "$@"
