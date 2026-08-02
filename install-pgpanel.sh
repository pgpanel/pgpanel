#!/usr/bin/env bash
# =============================================================================
# PgPanel one-line remote installer (Databasus-style)
#
# Usage (on a Linux VPS):
#
#   sudo apt-get update && sudo apt-get install -y curl
#   curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
#
# Or pin a tag/branch:
#
#   curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/v0.1.0/install-pgpanel.sh | sudo bash
#   PGPANEL_REPO=https://github.com/pgpanel/pgpanel.git \
#   PGPANEL_REF=main \
#     curl -sSL ... | sudo bash
#
# Environment overrides:
#   PGPANEL_REPO          Git clone URL (required when not in a local checkout)
#   PGPANEL_REF           Branch or tag (default: main)
#   PGPANEL_INSTALL_DIR   Install path (default: /opt/pgpanel)
#   PGPANEL_MODE          install|update|repair|... (default: interactive menu)
# =============================================================================
set -Eeuo pipefail

readonly DEFAULT_INSTALL_DIR="/opt/pgpanel"
# CHANGE THIS after you push to GitHub (or always set PGPANEL_REPO):
readonly DEFAULT_REPO="${PGPANEL_REPO:-https://github.com/pgpanel/pgpanel.git}"
readonly DEFAULT_REF="${PGPANEL_REF:-main}"

INSTALL_DIR="${PGPANEL_INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
REPO_URL="${DEFAULT_REPO}"
GIT_REF="${DEFAULT_REF}"
MODE="${PGPANEL_MODE:-}"

C_RED=$'\033[31m'
C_GREEN=$'\033[32m'
C_YELLOW=$'\033[33m'
C_CYAN=$'\033[36m'
C_BOLD=$'\033[1m'
C_RESET=$'\033[0m'

log()  { printf '%s[pgpanel]%s %s\n' "$C_CYAN" "$C_RESET" "$*"; }
ok()   { printf '%s[pgpanel]%s %s\n' "$C_GREEN" "$C_RESET" "$*"; }
warn() { printf '%s[pgpanel]%s %s\n' "$C_YELLOW" "$C_RESET" "$*"; }
die()  { printf '%s[pgpanel]%s %s\n' "$C_RED" "$C_RESET" "$*" >&2; exit 1; }

need_root() {
  if [[ "${EUID}" -ne 0 ]]; then
    die "Run as root: curl -sSL <url> | sudo bash"
  fi
}

have() { command -v "$1" >/dev/null 2>&1; }

detect_pkg() {
  if have apt-get; then echo apt
  elif have dnf; then echo dnf
  elif have yum; then echo yum
  elif have zypper; then echo zypper
  elif have pacman; then echo pacman
  elif have apk; then echo apk
  else echo unknown
  fi
}

install_prereqs() {
  local pkg
  pkg="$(detect_pkg)"
  local need=()
  have curl || need+=(curl)
  have git || need+=(git)
  have openssl || need+=(openssl)
  have ca-certificates || need+=(ca-certificates)

  # filter: ca-certificates may already exist as files
  [[ -f /etc/ssl/certs/ca-certificates.crt || -f /etc/pki/tls/certs/ca-bundle.crt ]] && true

  if [[ ${#need[@]} -eq 0 ]]; then
    ok "Prerequisites present (curl/git/openssl)"
    return 0
  fi

  log "Installing prerequisites: ${need[*]}"
  case "$pkg" in
    apt)
      export DEBIAN_FRONTEND=noninteractive
      apt-get update -qq
      apt-get install -y -qq ca-certificates curl git openssl
      ;;
    dnf) dnf install -y ca-certificates curl git openssl ;;
    yum) yum install -y ca-certificates curl git openssl ;;
    zypper) zypper --non-interactive install -y curl git openssl ca-certificates ;;
    pacman) pacman -Sy --noconfirm --needed curl git openssl ca-certificates ;;
    apk) apk add --no-cache curl git openssl ca-certificates ;;
    *) die "Install curl and git manually, then re-run" ;;
  esac
}

is_placeholder_repo() {
  [[ "$REPO_URL" == *"pgpanel"* ]]
}

resolve_repo_url() {
  if ! is_placeholder_repo; then
    return 0
  fi

  # If script lives inside a real checkout, use that
  if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
    local here
    here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    if [[ -f "${here}/deploy/install.sh" || -f "${here}/Cargo.toml" ]]; then
      REPO_URL=""
      return 0
    fi
  fi

  # Interactive / env required for curl|bash with placeholder
  if [[ -n "${PGPANEL_REPO:-}" ]]; then
    REPO_URL="$PGPANEL_REPO"
    return 0
  fi

  if [[ -t 0 ]]; then
    warn "Default GitHub URL still contains pgpanel — set your real repo."
    read -r -p "Git repository URL: " REPO_URL || true
    [[ -n "$REPO_URL" ]] || die "Repository URL required"
    return 0
  fi

  cat >&2 <<'EOF'
ERROR: Set your GitHub repository URL before one-line install.

Example:
  PGPANEL_REPO=https://github.com/myuser/pgpanel.git \
  curl -sSL https://raw.githubusercontent.com/myuser/pgpanel/main/install-pgpanel.sh | sudo bash

Or edit DEFAULT_REPO in install-pgpanel.sh after first push.
EOF
  exit 1
}

clone_or_update() {
  install_prereqs
  resolve_repo_url

  # Local tree (script next to deploy/)
  if [[ -z "$REPO_URL" ]]; then
    local here
    here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    if [[ -f "${here}/deploy/install.sh" ]]; then
      INSTALL_DIR="$here"
      ok "Using local checkout: $INSTALL_DIR"
      return 0
    fi
  fi

  mkdir -p "$(dirname "$INSTALL_DIR")"

  if [[ -d "${INSTALL_DIR}/.git" ]]; then
    log "Updating existing install at ${INSTALL_DIR}"
    git -C "$INSTALL_DIR" remote set-url origin "$REPO_URL" 2>/dev/null || true
    git -C "$INSTALL_DIR" fetch --tags --force origin
    if git -C "$INSTALL_DIR" rev-parse --verify "origin/${GIT_REF}" >/dev/null 2>&1; then
      git -C "$INSTALL_DIR" checkout -B "$GIT_REF" "origin/${GIT_REF}"
      git -C "$INSTALL_DIR" merge --ff-only "origin/${GIT_REF}" || true
    elif git -C "$INSTALL_DIR" rev-parse --verify "refs/tags/${GIT_REF}" >/dev/null 2>&1; then
      git -C "$INSTALL_DIR" checkout "tags/${GIT_REF}"
    else
      git -C "$INSTALL_DIR" checkout "$GIT_REF" || die "Unknown ref: $GIT_REF"
    fi
  elif [[ -f "${INSTALL_DIR}/deploy/install.sh" ]]; then
    ok "Found existing tree without .git at ${INSTALL_DIR}"
  else
    log "Cloning ${REPO_URL} (${GIT_REF}) → ${INSTALL_DIR}"
    # Hide tokens in logs
    if ! git clone --branch "$GIT_REF" --single-branch "$REPO_URL" "$INSTALL_DIR" 2>/dev/null; then
      git clone "$REPO_URL" "$INSTALL_DIR" || die "git clone failed"
      git -C "$INSTALL_DIR" checkout "$GIT_REF" || true
    fi
  fi

  [[ -f "${INSTALL_DIR}/deploy/install.sh" ]] || die "deploy/install.sh missing after clone"
  ok "Repository ready: ${INSTALL_DIR} @ $(git -C "$INSTALL_DIR" rev-parse --short HEAD 2>/dev/null || echo n/a)"
}

run_installer() {
  local args=()
  if [[ -n "$MODE" ]]; then
    args+=(--mode "$MODE")
  fi
  # Pass through remaining args from environment style
  export PGPANEL_INSTALL_DIR="$INSTALL_DIR"
  log "Launching interactive installer…"
  echo ""
  exec bash "${INSTALL_DIR}/deploy/install.sh" "${args[@]+"${args[@]}"}" "$@"
}

print_banner() {
  cat <<EOF
${C_BOLD}${C_CYAN}
  PgPanel remote installer
${C_RESET}
  Repo:  ${REPO_URL:-local}
  Ref:   ${GIT_REF}
  Dir:   ${INSTALL_DIR}

  Default admin username (after setup): ${C_BOLD}admin${C_RESET}
  Password: generated once at install (or you choose interactively)
  First login: open the panel URL → /setup  (bootstrap)

EOF
}

main() {
  need_root
  print_banner
  clone_or_update
  # Remaining CLI args after bootstrap (if any when not piped)
  run_installer "$@"
}

main "$@"
