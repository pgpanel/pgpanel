#!/usr/bin/env bash
# =============================================================================
# PgPanel one-line remote installer (Databasus-style)
#
# Usage (on a Linux VPS):
#
#   sudo apt-get update && sudo apt-get install -y curl
#   curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
#
# Optional overrides (must be visible to bash, not only curl):
#
#   curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh \
#     | sudo env PGPANEL_REF=main bash
#
#   curl -sSL ... | sudo bash -s -- --ref main
#
# Environment:
#   PGPANEL_REPO          Git clone URL (default: official repo below)
#   PGPANEL_REF           Branch or tag (default: main)
#   PGPANEL_INSTALL_DIR   Install path (default: /opt/pgpanel)
#   PGPANEL_MODE          install|update|repair|... (default: interactive menu)
# =============================================================================
set -Eeuo pipefail

readonly DEFAULT_INSTALL_DIR="/opt/pgpanel"
readonly DEFAULT_REPO="https://github.com/pgpanel/pgpanel.git"
readonly DEFAULT_REF="main"

INSTALL_DIR="${PGPANEL_INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
REPO_URL="${PGPANEL_REPO:-$DEFAULT_REPO}"
GIT_REF="${PGPANEL_REF:-$DEFAULT_REF}"
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
    die "Run as root, e.g.: curl -sSL <url> | sudo bash"
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

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --repo)
        REPO_URL="${2:-}"
        shift 2
        ;;
      --repo=*)
        REPO_URL="${1#*=}"
        shift
        ;;
      --ref)
        GIT_REF="${2:-}"
        shift 2
        ;;
      --ref=*)
        GIT_REF="${1#*=}"
        shift
        ;;
      --dir)
        INSTALL_DIR="${2:-}"
        shift 2
        ;;
      --dir=*)
        INSTALL_DIR="${1#*=}"
        shift
        ;;
      --mode)
        MODE="${2:-}"
        shift 2
        ;;
      --mode=*)
        MODE="${1#*=}"
        shift
        ;;
      -h|--help)
        cat <<'EOF'
PgPanel remote installer

Usage:
  curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash

  curl -sSL ... | sudo bash -s -- --ref main --mode install

Options:
  --repo URL     Git repository (default: https://github.com/pgpanel/pgpanel.git)
  --ref  NAME    Branch or tag (default: main)
  --dir  PATH    Install directory (default: /opt/pgpanel)
  --mode NAME    Pass --mode to deploy/install.sh
EOF
        exit 0
        ;;
      *)
        # Ignore unknown for forward compatibility when piped
        warn "Ignoring unknown argument: $1"
        shift
        ;;
    esac
  done
}

install_prereqs() {
  local pkg
  pkg="$(detect_pkg)"
  local need=()
  have curl || need+=(curl)
  have git || need+=(git)
  have openssl || need+=(openssl)

  # Only request ca-certificates package if cert bundle missing
  if [[ ! -f /etc/ssl/certs/ca-certificates.crt && ! -f /etc/pki/tls/certs/ca-bundle.crt ]]; then
    need+=(ca-certificates)
  fi

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

# Only treat explicit placeholders as unset — never the real pgpanel org URL.
is_placeholder_repo() {
  local u="${1:-}"
  [[ -z "$u" ]] && return 0
  [[ "$u" == *"YOUR_USER"* ]] && return 0
  [[ "$u" == *"myuser"* ]] && return 0
  [[ "$u" == *"example.com"* ]] && return 0
  [[ "$u" == "CHANGE_ME" ]] && return 0
  return 1
}

resolve_repo_url() {
  # Prefer env if set and non-placeholder
  if [[ -n "${PGPANEL_REPO:-}" ]] && ! is_placeholder_repo "$PGPANEL_REPO"; then
    REPO_URL="$PGPANEL_REPO"
  fi

  # Running from a local checkout: use that tree
  if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
    local here
    here="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd)" || here=""
    if [[ -n "$here" && ( -f "${here}/deploy/install.sh" || -f "${here}/Cargo.toml" ) ]]; then
      # Local mode only if not forced remote
      if [[ -z "${PGPANEL_REPO:-}" ]]; then
        REPO_URL=""
        INSTALL_DIR="$here"
        ok "Using local checkout: $INSTALL_DIR"
        return 0
      fi
    fi
  fi

  if is_placeholder_repo "$REPO_URL"; then
    if [[ -t 0 ]]; then
      warn "Repository URL looks like a placeholder."
      read -r -p "Git repository URL [${DEFAULT_REPO}]: " REPO_URL || true
      REPO_URL="${REPO_URL:-$DEFAULT_REPO}"
    else
      REPO_URL="$DEFAULT_REPO"
    fi
  fi

  if is_placeholder_repo "$REPO_URL"; then
    die "Invalid repository URL. Use: curl -sSL ... | sudo bash"
  fi

  ok "Repository URL: $REPO_URL"
}

clone_or_update() {
  install_prereqs
  resolve_repo_url

  # Local tree already selected
  if [[ -z "$REPO_URL" ]]; then
    [[ -f "${INSTALL_DIR}/deploy/install.sh" ]] || die "Local deploy/install.sh missing"
    return 0
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
    # Clean partial failed clone
    if [[ -d "$INSTALL_DIR" && ! -d "${INSTALL_DIR}/.git" ]]; then
      if [[ -z "$(ls -A "$INSTALL_DIR" 2>/dev/null || true)" ]]; then
        rmdir "$INSTALL_DIR" 2>/dev/null || true
      fi
    fi
    if ! git clone --branch "$GIT_REF" --single-branch "$REPO_URL" "$INSTALL_DIR"; then
      git clone "$REPO_URL" "$INSTALL_DIR" || die "git clone failed — is the repo public and the URL correct?"
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
  export PGPANEL_INSTALL_DIR="$INSTALL_DIR"
  log "Launching installer…"
  echo ""
  exec bash "${INSTALL_DIR}/deploy/install.sh" "${args[@]+"${args[@]}"}"
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
  parse_args "$@"
  need_root
  print_banner
  clone_or_update
  run_installer
}

main "$@"
