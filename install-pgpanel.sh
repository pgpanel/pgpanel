#!/usr/bin/env bash
# =============================================================================
# PgPanel one-line production installer
#
#   curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
#
# Optional:
#   curl -sSL ... | sudo env PGPANEL_REF=main bash
#   curl -sSL ... | sudo bash -s -- --mode update
#
# Developer: Dezső Benedek Péter
# =============================================================================
set -Eeuo pipefail

readonly OFFICIAL_REPO="https://github.com/pgpanel/pgpanel.git"
readonly DEFAULT_INSTALL_DIR="/opt/pgpanel"
readonly DEFAULT_REF="main"

INSTALL_DIR="${PGPANEL_INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
REPO_URL="$OFFICIAL_REPO"
GIT_REF="${PGPANEL_REF:-$DEFAULT_REF}"
MODE="${PGPANEL_MODE:-}"

C_RED=$'\033[31m'
C_GREEN=$'\033[32m'
C_CYAN=$'\033[36m'
C_BOLD=$'\033[1m'
C_RESET=$'\033[0m'

log()  { printf '%s[pgpanel]%s %s\n' "$C_CYAN" "$C_RESET" "$*"; }
ok()   { printf '%s[pgpanel]%s %s\n' "$C_GREEN" "$C_RESET" "$*"; }
die()  { printf '%s[pgpanel]%s %s\n' "$C_RED" "$C_RESET" "$*" >&2; exit 1; }

need_root() {
  [[ "${EUID}" -eq 0 ]] || die "Run as root: curl -sSL <url> | sudo bash"
}

have() { command -v "$1" >/dev/null 2>&1; }

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --ref) GIT_REF="${2:-main}"; shift 2 ;;
      --ref=*) GIT_REF="${1#*=}"; shift ;;
      --dir) INSTALL_DIR="${2:-}"; shift 2 ;;
      --dir=*) INSTALL_DIR="${1#*=}"; shift ;;
      --mode) MODE="${2:-}"; shift 2 ;;
      --mode=*) MODE="${1#*=}"; shift ;;
      -h|--help)
        cat <<'EOF'
PgPanel installer

  curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash

Options:
  --ref NAME    git branch/tag (default: main)
  --dir PATH    install directory (default: /opt/pgpanel)
  --mode NAME   install|update|repair|security|uninstall
EOF
        exit 0
        ;;
      *) log "Ignoring unknown argument: $1"; shift ;;
    esac
  done
}

install_prereqs() {
  local need=()
  have curl || need+=(curl)
  have git || need+=(git)
  have openssl || need+=(openssl)
  if [[ ! -f /etc/ssl/certs/ca-certificates.crt && ! -f /etc/pki/tls/certs/ca-bundle.crt ]]; then
    need+=(ca-certificates)
  fi
  [[ ${#need[@]} -eq 0 ]] && { ok "Prerequisites OK"; return 0; }

  log "Installing: ${need[*]}"
  if have apt-get; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq ca-certificates curl git openssl
  elif have dnf; then
    dnf install -y ca-certificates curl git openssl
  elif have yum; then
    yum install -y ca-certificates curl git openssl
  else
    die "Install curl git openssl, then re-run"
  fi
}

clone_or_update() {
  install_prereqs
  mkdir -p "$(dirname "$INSTALL_DIR")"

  if [[ -d "${INSTALL_DIR}/.git" ]]; then
    log "Updating ${INSTALL_DIR} (${GIT_REF})"
    git -C "$INSTALL_DIR" remote set-url origin "$REPO_URL"
    git -C "$INSTALL_DIR" fetch --tags --force origin
    if git -C "$INSTALL_DIR" rev-parse --verify "origin/${GIT_REF}" >/dev/null 2>&1; then
      git -C "$INSTALL_DIR" checkout -B "$GIT_REF" "origin/${GIT_REF}"
      git -C "$INSTALL_DIR" merge --ff-only "origin/${GIT_REF}" || true
    else
      git -C "$INSTALL_DIR" checkout "$GIT_REF" || die "Unknown ref: $GIT_REF"
    fi
  elif [[ -f "${INSTALL_DIR}/deploy/install.sh" ]]; then
    ok "Using existing tree at ${INSTALL_DIR}"
  else
    log "Cloning ${REPO_URL} → ${INSTALL_DIR}"
    git clone --branch "$GIT_REF" --single-branch "$REPO_URL" "$INSTALL_DIR" \
      || { git clone "$REPO_URL" "$INSTALL_DIR" && git -C "$INSTALL_DIR" checkout "$GIT_REF"; } \
      || die "git clone failed"
  fi

  [[ -f "${INSTALL_DIR}/deploy/install.sh" ]] || die "deploy/install.sh missing"
  local ver="?"
  [[ -f "${INSTALL_DIR}/VERSION" ]] && ver="$(tr -d ' \n' <"${INSTALL_DIR}/VERSION")"
  ok "Ready ${INSTALL_DIR} version=${ver} commit=$(git -C "$INSTALL_DIR" rev-parse --short HEAD 2>/dev/null || echo n/a)"
}

run_installer() {
  local args=()
  [[ -n "$MODE" ]] && args+=(--mode "$MODE")
  export PGPANEL_INSTALL_DIR="$INSTALL_DIR"
  log "Starting PgPanel installer…"
  echo ""
  if [[ ! -t 0 && -r /dev/tty ]]; then
    exec bash "${INSTALL_DIR}/deploy/install.sh" "${args[@]+"${args[@]}"}" </dev/tty
  fi
  exec bash "${INSTALL_DIR}/deploy/install.sh" "${args[@]+"${args[@]}"}"
}

main() {
  parse_args "$@"
  need_root
  cat <<EOF
${C_BOLD}${C_CYAN}
  PgPanel installer
${C_RESET}
  Developer: Dezső Benedek Péter
  Repo:      ${REPO_URL}
  Ref:       ${GIT_REF}
  Dir:       ${INSTALL_DIR}

  Admin user after setup: admin
  First login: https://YOUR_DOMAIN/setup

EOF
  clone_or_update
  run_installer
}

main "$@"
