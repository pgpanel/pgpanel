#!/usr/bin/env bash
# Collect PgPanel diagnostics (safe, read-only). Does not print secrets.
set -euo pipefail

readonly CONFIG="/etc/pgpanel/pgpanel.toml"
readonly INSTALL_ROOT="/opt/pgpanel"
readonly STATE_DIR="/var/lib/pgpanel"
readonly CADDY_LISTEN="127.0.0.1:8080"
readonly BLUE_LISTEN="127.0.0.1:8081"
readonly GREEN_LISTEN="127.0.0.1:8082"
readonly OUTPUT_DIR="${1:-}"

timestamp() {
    date -u +"%Y-%m-%dT%H:%M:%SZ"
}

section() {
    printf '\n========== %s ==========\n' "$1"
}

redact_config() {
    sed -E \
        -e 's/^(secret_key\s*=\s*).*/\1"[REDACTED]"/' \
        -e 's/^(api_key\s*=\s*).*/\1"[REDACTED]"/' \
        "$1" 2>/dev/null || true
}

collect_to_stdout() {
    section "PgPanel diagnostics $(timestamp)"

    section "System"
    printf 'hostname: %s\n' "$(hostname -f 2>/dev/null || hostname)"
    if [[ -f /etc/os-release ]]; then
        # shellcheck source=/dev/null
        source /etc/os-release
        printf 'os: %s %s\n' "${NAME:-unknown}" "${VERSION_ID:-}"
    fi
    printf 'kernel: %s\n' "$(uname -r)"
    printf 'arch: %s\n' "$(uname -m)"
    printf 'uptime: %s\n' "$(uptime -p 2>/dev/null || uptime)"

    section "PgPanel binaries"
    if [[ -L "${INSTALL_ROOT}/current" ]]; then
        printf 'current release: %s\n' "$(readlink -f "${INSTALL_ROOT}/current" 2>/dev/null || readlink "${INSTALL_ROOT}/current")"
    else
        printf 'current release: not installed\n'
    fi
    if [[ -L "${INSTALL_ROOT}/previous" ]]; then
        printf 'previous release: %s\n' "$(readlink -f "${INSTALL_ROOT}/previous" 2>/dev/null || readlink "${INSTALL_ROOT}/previous")"
    fi
    for bin in pgpanel-web pgpanel-helper pgpanel-updater; do
        local path="${INSTALL_ROOT}/current/bin/${bin}"
        if [[ -x "${path}" ]]; then
            printf '%s: %s\n' "${bin}" "$("${path}" --version 2>/dev/null || echo 'version unavailable')"
        else
            printf '%s: not found\n' "${bin}"
        fi
    done

    section "Updater status"
    if [[ -x "${INSTALL_ROOT}/current/bin/pgpanel-updater" ]]; then
        "${INSTALL_ROOT}/current/bin/pgpanel-updater" --config "${CONFIG}" status 2>/dev/null || printf 'status: unavailable\n'
    fi

    section "systemd"
    local unit
    for unit in \
        pgpanel-blue.service \
        pgpanel-green.service \
        pgpanel-helper.service \
        pgpanel-update-check.timer \
        caddy.service; do
        printf '--- %s ---\n' "${unit}"
        systemctl is-active "${unit}" 2>/dev/null || printf 'inactive\n'
        systemctl show "${unit}" -p ActiveState -p SubState -p MainPID -p ExecMainStatus --no-pager 2>/dev/null || true
    done

    section "Health endpoints"
    local base
    for base in \
        "http://${CADDY_LISTEN}" \
        "http://${BLUE_LISTEN}" \
        "http://${GREEN_LISTEN}"; do
        printf '--- %s ---\n' "${base}"
        curl -fsS -o /dev/null -w 'live: %{http_code}\n' "${base}/health/live" 2>/dev/null || printf 'live: unreachable\n'
        curl -fsS -o /dev/null -w 'ready: %{http_code}\n' "${base}/health/ready" 2>/dev/null || printf 'ready: unreachable\n'
    done

    section "Disk usage"
    df -h "${STATE_DIR}" "${INSTALL_ROOT}" /var/lib/postgresql 2>/dev/null || df -h

    section "PostgreSQL clusters (pg_lsclusters)"
    if command -v pg_lsclusters >/dev/null 2>&1; then
        pg_lsclusters 2>/dev/null || printf 'pg_lsclusters failed\n'
    else
        printf 'pg_lsclusters not installed\n'
    fi

    section "Configuration (redacted)"
    if [[ -f "${CONFIG}" ]]; then
        redact_config "${CONFIG}"
    else
        printf 'config not found: %s\n' "${CONFIG}"
    fi

    section "Caddy upstream"
    if [[ -f /etc/caddy/pgpanel-upstream.caddy ]]; then
        cat /etc/caddy/pgpanel-upstream.caddy
    else
        printf 'upstream config not found\n'
    fi

    section "Recent journal (pgpanel-blue, last 20 lines)"
    journalctl -u pgpanel-blue.service -u pgpanel-green.service -u pgpanel-helper.service \
        --no-pager -n 20 2>/dev/null || printf 'journal unavailable\n'
}

main() {
    if [[ -n "${OUTPUT_DIR}" ]]; then
        mkdir -p "${OUTPUT_DIR}"
        local outfile="${OUTPUT_DIR}/pgpanel-diagnostics-$(date -u +%Y%m%dT%H%M%SZ).txt"
        collect_to_stdout | tee "${outfile}"
        printf '\nDiagnostics written to: %s\n' "${outfile}"
    else
        collect_to_stdout
    fi
}

main "$@"
