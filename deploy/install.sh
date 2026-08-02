#!/usr/bin/env bash
# shellcheck disable=SC2034,SC2155
# =============================================================================
# PgPanel production installer — multi-distro interactive Bash installer
#
# Compatible (primary): Ubuntu 22.04/24.04, Debian 12
# Also supported: Debian 11+, Ubuntu 20.04+, Rocky/Alma/RHEL 8/9, Fedora 38+,
#                 openSUSE Leap 15+, Amazon Linux 2023, Arch Linux
#
# Usage:
#   sudo bash deploy/install.sh
#   sudo bash deploy/install.sh --non-interactive   # uses installer.conf defaults
#   sudo bash deploy/install.sh --mode update|repair|security|uninstall
#
# shellcheck: shellcheck -x deploy/install.sh
# =============================================================================
set -Eeuo pipefail

# ── Constants ────────────────────────────────────────────────────────────────
readonly INSTALLER_VERSION="1.5.0"
readonly PGPANEL_AUTHOR="Dezső Benedek Péter"
readonly PGPANEL_OFFICIAL_REPO="https://github.com/pgpanel/pgpanel.git"
readonly PGPANEL_GHCR_IMAGE="ghcr.io/pgpanel/pgpanel"
readonly PGPANEL_DEFAULT_INSTALL_DIR="/opt/pgpanel"
readonly PGPANEL_DEFAULT_DATA_DIR="/var/lib/pgpanel"
readonly PGPANEL_DEFAULT_LOG_DIR="/var/log/pgpanel"
readonly PGPANEL_ETC_DIR="/etc/pgpanel"
readonly PGPANEL_CONF="${PGPANEL_ETC_DIR}/installer.conf"
readonly PGPANEL_ANSWERS_FILE="${PGPANEL_ETC_DIR}/install-answers.env"
readonly PGPANEL_PROGRESS_FILE="${PGPANEL_ETC_DIR}/install-progress.env"
readonly PGPANEL_CLI_PATH="/usr/local/bin/pgpanel"
readonly MIN_CPU=2
readonly MIN_RAM_MB=4096
readonly MIN_DISK_GB=30
readonly REC_CPU=4
readonly REC_RAM_MB=8192
readonly REC_DISK_GB=80
readonly NETWORK_TIMEOUT=15

# ── Runtime state ────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
TMPDIR_INSTALL=""
LOG_FILE=""
INSTALL_PHASE="init"
NON_INTERACTIVE=0
CLI_MODE=""
USE_WHIPTAIL=0
INTERRUPTED=0

# Config (loaded from conf / prompts)
INSTALL_DIR="${PGPANEL_DEFAULT_INSTALL_DIR}"
DATA_DIR="${PGPANEL_DEFAULT_DATA_DIR}"
CLUSTER_DATA_DIR="${PGPANEL_DEFAULT_DATA_DIR}/clusters"
LOG_DIR="${PGPANEL_DEFAULT_LOG_DIR}"
BACKUP_CACHE_DIR="${PGPANEL_DEFAULT_DATA_DIR}/backups"
REPO_URL="${PGPANEL_OFFICIAL_REPO}"
GIT_REF="main"
UPDATE_CHANNEL="stable"
PGPANEL_VERSION="0.1.0"
PGPANEL_IMAGE=""
FORCE_UPDATE=0
USE_DOMAIN=1
PANEL_DOMAIN="db.example.com"
# Empty by default — Databasus is internal-only unless DATABASUS_PUBLIC=1
DATABASUS_DOMAIN=""
DATABASUS_PUBLIC=0
LETSENCRYPT_EMAIL="admin@example.com"
ENABLE_HTTPS=1
# Never expose PostgreSQL ports on the host by default
PUBLIC_PG_PORTS=0
ADMIN_IP_ALLOWLIST=""
SSH_PORT=22
CONFIGURE_UFW=1
BEHIND_CLOUDFLARE=0
USE_CLOUDFLARE_TUNNEL=0
ADMIN_USERNAME="admin"
ADMIN_EMAIL="admin@example.com"
ADMIN_PASSWORD=""
ADMIN_PASSWORD_GENERATED=0
PREPARE_2FA=0
SESSION_TTL_HOURS=24
LOGIN_MAX_ATTEMPTS=5
PG_DEFAULT_VERSION="17"
PG_ALLOWED_VERSIONS="16,17,18"
PG_DEFAULT_CPU="2"
PG_DEFAULT_MEMORY_MB="2048"
PG_DEFAULT_STORAGE_GB="20"
PG_TIMEZONE="Europe/Budapest"
PG_LOCALE="en_US.UTF-8"
PG_MAX_CLUSTERS="20"
PG_CLUSTER_PREFIX="pgpanel_pg_"
PG_AUTO_RESTART=1
DOCKER_REGISTRY=""
ENABLE_DATABASUS=1
DATABASUS_ADMIN_EMAIL=""
DATABASUS_ADMIN_PASSWORD=""
BACKUP_STORAGE_TYPE="later"
S3_ENDPOINT=""
S3_REGION=""
S3_BUCKET=""
S3_ACCESS_KEY=""
S3_SECRET_KEY=""
S3_PATH_STYLE=1
S3_PREFIX="pgpanel/"
S3_TLS_VERIFY=1
S3_ENCRYPT=1
BACKUP_RETENTION_DAYS=14
BACKUP_MAX_COUNT=30
BACKUP_FULL_FREQ="daily"
BACKUP_INCR_FREQ="hourly"
BACKUP_WAL_STREAMING=1
BACKUP_RESTORE_VERIFY="daily"
BACKUP_FIRST_NOW=1
NOTIFY_TYPE="none"
SMTP_HOST=""
SMTP_PORT="587"
SMTP_TLS="starttls"
SMTP_USER=""
SMTP_PASSWORD=""
SMTP_FROM=""
SMTP_TO=""
WEBHOOK_URL=""
WEBHOOK_TOKEN=""
TIMEZONE_SET="${PG_TIMEZONE}"
CREATE_SWAP=0
SWAP_SIZE_MB=2048
ENABLE_WATCHTOWER=0
PRODUCTION_READY=0
MINIMAL_INSTALL=1

# ── Colors ───────────────────────────────────────────────────────────────────
if [[ -t 1 ]]; then
  C_RESET=$'\033[0m'
  C_BOLD=$'\033[1m'
  C_DIM=$'\033[2m'
  C_RED=$'\033[31m'
  C_GREEN=$'\033[32m'
  C_YELLOW=$'\033[33m'
  C_BLUE=$'\033[34m'
  C_CYAN=$'\033[36m'
  C_MAGENTA=$'\033[35m'
else
  C_RESET="" C_BOLD="" C_DIM="" C_RED="" C_GREEN="" C_YELLOW="" C_BLUE="" C_CYAN="" C_MAGENTA=""
fi

# ── Logging ──────────────────────────────────────────────────────────────────
log_raw() {
  local msg="$1"
  if [[ -n "${LOG_FILE:-}" ]]; then
    printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$msg" >>"$LOG_FILE" 2>/dev/null || true
  fi
}

mask_secrets() {
  local s="$1"
  s="$(printf '%s' "$s" | sed -E \
    -e 's/(password|passwd|secret|token|key|credential)(=|[[:space:]]*:[[:space:]]*)[^[:space:]]+/\1\2***REDACTED***/Ig' \
    -e 's/(AKIA[0-9A-Z]{16})/***ACCESS_KEY***/g' \
    -e 's|(postgresql://[^:]+:)[^@]+@|\1***@|g')"
  printf '%s' "$s"
}

log_info()  { local m; m="$(mask_secrets "$*")"; printf '%s[INFO]%s  %s\n' "$C_BLUE" "$C_RESET" "$m"; log_raw "INFO  $m"; }
log_ok()    { local m; m="$(mask_secrets "$*")"; printf '%s[ OK ]%s  %s\n' "$C_GREEN" "$C_RESET" "$m"; log_raw "OK    $m"; }
log_warn()  { local m; m="$(mask_secrets "$*")"; printf '%s[WARN]%s  %s\n' "$C_YELLOW" "$C_RESET" "$m"; log_raw "WARN  $m"; }
log_error() { local m; m="$(mask_secrets "$*")"; printf '%s[ERR ]%s  %s\n' "$C_RED" "$C_RESET" "$m" >&2; log_raw "ERROR $m"; }
log_step()  { local n="$1" total="$2" msg="$3"; printf '\n%s[%s/%s]%s %s%s%s\n' "$C_CYAN" "$n" "$total" "$C_RESET" "$C_BOLD" "$msg" "$C_RESET"; log_raw "STEP  [$n/$total] $msg"; }
die()       { log_error "$*"; exit 1; }

# ── Cleanup / traps ──────────────────────────────────────────────────────────
cleanup() {
  local ec=$?
  if [[ -n "${TMPDIR_INSTALL:-}" && -d "${TMPDIR_INSTALL}" ]]; then
    rm -rf "${TMPDIR_INSTALL}" 2>/dev/null || true
  fi
  # Persist failure point for resume (answers already saved after prompts)
  if [[ $ec -ne 0 && "$INSTALL_PHASE" != "init" && "$INSTALL_PHASE" != "done" ]]; then
    write_progress_meta "failed" || true
  fi
  if [[ $INTERRUPTED -eq 1 ]]; then
    log_warn "Interrupted (Ctrl+C). Answers and progress were saved."
    log_info "Continue: sudo bash /opt/pgpanel/deploy/install.sh --mode resume"
    log_info "Or menu → Telepítés folytatása"
  elif [[ $ec -ne 0 && "$INSTALL_PHASE" != "init" && "$INSTALL_PHASE" != "done" ]]; then
    log_error "Installer failed during phase: ${INSTALL_PHASE}"
    log_info "Logs: ${LOG_FILE:-n/a}"
    log_info "Continue (keeps your answers): sudo bash /opt/pgpanel/deploy/install.sh --mode resume"
    log_info "Repair only:              sudo pgpanel repair"
    log_info "Full log:                 tail -200 ${LOG_FILE:-/var/log/pgpanel/installer.log}"
  fi
}
trap cleanup EXIT
on_interrupt() {
  INTERRUPTED=1
  write_progress_meta "interrupted" || true
  exit 130
}
trap on_interrupt INT TERM

# ── Helpers ──────────────────────────────────────────────────────────────────
require_cmd() { command -v "$1" >/dev/null 2>&1 || die "Required command not found: $1"; }

have_cmd() { command -v "$1" >/dev/null 2>&1; }

# Log command output without SIGPIPE / pipefail crashes (e.g. docker | head)
log_cmd_preview() {
  local max="${1:-8}"
  shift
  local out=""
  out="$("$@" 2>&1)" || true
  local i=0 line
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" ]] && continue
    log_info "  $line"
    i=$((i + 1))
    (( i >= max )) && break
  done <<<"$out"
}

# ── Install progress / answers (resume support) ──────────────────────────────
write_progress_meta() {
  local status="${1:-in_progress}"
  mkdir -p "$PGPANEL_ETC_DIR"
  local tmp
  tmp="$(mktemp)"
  {
    echo "STATUS=${status}"
    echo "LAST_PHASE=${INSTALL_PHASE}"
    echo "UPDATED_AT=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "INSTALLER_VERSION=${INSTALLER_VERSION}"
    if [[ -f "$PGPANEL_PROGRESS_FILE" ]]; then
      grep -E '^STEP_' "$PGPANEL_PROGRESS_FILE" 2>/dev/null || true
    fi
  } >"$tmp"
  chmod 0640 "$tmp"
  mv -f "$tmp" "$PGPANEL_PROGRESS_FILE"
}

mark_step_done() {
  local step="$1"
  mkdir -p "$PGPANEL_ETC_DIR"
  touch "$PGPANEL_PROGRESS_FILE"
  chmod 0640 "$PGPANEL_PROGRESS_FILE" 2>/dev/null || true
  if grep -q "^STEP_${step}=" "$PGPANEL_PROGRESS_FILE" 2>/dev/null; then
    local tmp
    tmp="$(mktemp)"
    awk -v k="STEP_${step}" 'BEGIN{FS=OFS="="} $1==k {$0=k"=done"} {print}' \
      "$PGPANEL_PROGRESS_FILE" >"$tmp"
    mv -f "$tmp" "$PGPANEL_PROGRESS_FILE"
  else
    echo "STEP_${step}=done" >>"$PGPANEL_PROGRESS_FILE"
  fi
  INSTALL_PHASE="$step"
  write_progress_meta "in_progress"
  log_ok "Step complete: ${step}"
}

is_step_done() {
  local step="$1"
  [[ -f "$PGPANEL_PROGRESS_FILE" ]] || return 1
  grep -q "^STEP_${step}=done$" "$PGPANEL_PROGRESS_FILE" 2>/dev/null
}

clear_install_progress() {
  rm -f "$PGPANEL_PROGRESS_FILE"
}

# Save all non-transient installer answers (incl. passwords) with 0600
save_install_answers() {
  mkdir -p "$PGPANEL_ETC_DIR"
  umask 077
  local tmp
  tmp="$(mktemp)"
  cat >"$tmp" <<EOF
# PgPanel install answers — mode 0600. Do not share.
# Saved: $(date -u +%Y-%m-%dT%H:%M:%SZ)
INSTALL_DIR=${INSTALL_DIR}
DATA_DIR=${DATA_DIR}
CLUSTER_DATA_DIR=${CLUSTER_DATA_DIR}
LOG_DIR=${LOG_DIR}
BACKUP_CACHE_DIR=${BACKUP_CACHE_DIR}
REPO_URL=${PGPANEL_OFFICIAL_REPO}
GIT_REF=${GIT_REF}
UPDATE_CHANNEL=${UPDATE_CHANNEL}
USE_DOMAIN=${USE_DOMAIN}
PANEL_DOMAIN=${PANEL_DOMAIN}
DATABASUS_DOMAIN=${DATABASUS_DOMAIN}
DATABASUS_PUBLIC=${DATABASUS_PUBLIC}
LETSENCRYPT_EMAIL=${LETSENCRYPT_EMAIL}
ENABLE_HTTPS=${ENABLE_HTTPS}
PUBLIC_PG_PORTS=${PUBLIC_PG_PORTS}
ADMIN_IP_ALLOWLIST=${ADMIN_IP_ALLOWLIST}
SSH_PORT=${SSH_PORT}
CONFIGURE_UFW=${CONFIGURE_UFW}
BEHIND_CLOUDFLARE=${BEHIND_CLOUDFLARE}
USE_CLOUDFLARE_TUNNEL=${USE_CLOUDFLARE_TUNNEL}
ADMIN_USERNAME=${ADMIN_USERNAME}
ADMIN_EMAIL=${ADMIN_EMAIL}
ADMIN_PASSWORD=${ADMIN_PASSWORD}
ADMIN_PASSWORD_GENERATED=${ADMIN_PASSWORD_GENERATED}
PREPARE_2FA=${PREPARE_2FA}
SESSION_TTL_HOURS=${SESSION_TTL_HOURS}
LOGIN_MAX_ATTEMPTS=${LOGIN_MAX_ATTEMPTS}
PG_DEFAULT_VERSION=${PG_DEFAULT_VERSION}
PG_ALLOWED_VERSIONS=${PG_ALLOWED_VERSIONS}
PG_DEFAULT_CPU=${PG_DEFAULT_CPU}
PG_DEFAULT_MEMORY_MB=${PG_DEFAULT_MEMORY_MB}
PG_DEFAULT_STORAGE_GB=${PG_DEFAULT_STORAGE_GB}
PG_TIMEZONE=${PG_TIMEZONE}
PG_LOCALE=${PG_LOCALE}
PG_MAX_CLUSTERS=${PG_MAX_CLUSTERS}
PG_CLUSTER_PREFIX=${PG_CLUSTER_PREFIX}
PG_AUTO_RESTART=${PG_AUTO_RESTART}
ENABLE_DATABASUS=${ENABLE_DATABASUS}
DATABASUS_ADMIN_EMAIL=${DATABASUS_ADMIN_EMAIL}
DATABASUS_ADMIN_PASSWORD=${DATABASUS_ADMIN_PASSWORD}
BACKUP_STORAGE_TYPE=${BACKUP_STORAGE_TYPE}
S3_ENDPOINT=${S3_ENDPOINT}
S3_REGION=${S3_REGION}
S3_BUCKET=${S3_BUCKET}
S3_ACCESS_KEY=${S3_ACCESS_KEY}
S3_SECRET_KEY=${S3_SECRET_KEY}
S3_PATH_STYLE=${S3_PATH_STYLE}
S3_PREFIX=${S3_PREFIX}
S3_TLS_VERIFY=${S3_TLS_VERIFY}
S3_ENCRYPT=${S3_ENCRYPT}
BACKUP_RETENTION_DAYS=${BACKUP_RETENTION_DAYS}
BACKUP_MAX_COUNT=${BACKUP_MAX_COUNT}
BACKUP_FULL_FREQ=${BACKUP_FULL_FREQ}
BACKUP_INCR_FREQ=${BACKUP_INCR_FREQ}
BACKUP_WAL_STREAMING=${BACKUP_WAL_STREAMING}
BACKUP_RESTORE_VERIFY=${BACKUP_RESTORE_VERIFY}
BACKUP_FIRST_NOW=${BACKUP_FIRST_NOW}
NOTIFY_TYPE=${NOTIFY_TYPE}
SMTP_HOST=${SMTP_HOST}
SMTP_PORT=${SMTP_PORT}
SMTP_TLS=${SMTP_TLS}
SMTP_USER=${SMTP_USER}
SMTP_PASSWORD=${SMTP_PASSWORD}
SMTP_FROM=${SMTP_FROM}
SMTP_TO=${SMTP_TO}
WEBHOOK_URL=${WEBHOOK_URL}
WEBHOOK_TOKEN=${WEBHOOK_TOKEN}
TIMEZONE_SET=${TIMEZONE_SET}
CREATE_SWAP=${CREATE_SWAP}
SWAP_SIZE_MB=${SWAP_SIZE_MB}
ENABLE_WATCHTOWER=${ENABLE_WATCHTOWER}
MINIMAL_INSTALL=${MINIMAL_INSTALL}
EOF
  chmod 0600 "$tmp"
  chown root:root "$tmp" 2>/dev/null || true
  mv -f "$tmp" "$PGPANEL_ANSWERS_FILE"
  chmod 0600 "$PGPANEL_ANSWERS_FILE"
  log_ok "Answers saved → ${PGPANEL_ANSWERS_FILE}"
}

load_install_answers() {
  if [[ ! -f "$PGPANEL_ANSWERS_FILE" ]]; then
    return 1
  fi
  # shellcheck source=/dev/null
  set -a
  # shellcheck disable=SC1090
  source "$PGPANEL_ANSWERS_FILE"
  set +a
  REPO_URL="$PGPANEL_OFFICIAL_REPO"
  log_info "Loaded saved answers from ${PGPANEL_ANSWERS_FILE}"
  return 0
}

has_resumable_install() {
  [[ -f "$PGPANEL_PROGRESS_FILE" ]] || return 1
  local st
  st="$(grep '^STATUS=' "$PGPANEL_PROGRESS_FILE" 2>/dev/null | head -1 | cut -d= -f2- || true)"
  [[ "$st" == "failed" || "$st" == "interrupted" || "$st" == "in_progress" ]]
}

# Run a step unless already done (for resume)
run_step() {
  local step_id="$1"
  local step_no="$2"
  local step_total="$3"
  local step_title="$4"
  shift 4
  INSTALL_PHASE="$step_id"
  if is_step_done "$step_id"; then
    log_ok "Skip (already done): [${step_no}/${step_total}] ${step_title}"
    return 0
  fi
  write_progress_meta "in_progress"
  log_step "$step_no" "$step_total" "$step_title"
  # shellcheck disable=SC2068
  "$@"
  mark_step_done "$step_id"
}

run_as_root() {
  if [[ "${EUID}" -eq 0 ]]; then
    "$@"
  elif have_cmd sudo; then
    sudo "$@"
  else
    die "Root privileges required"
  fi
}

ensure_root() {
  if [[ "${EUID}" -ne 0 ]]; then
    if have_cmd sudo; then
      log_info "Re-executing with sudo…"
      exec sudo -E bash "$0" "$@"
    fi
    die "This installer must run as root or via sudo"
  fi
}

# When launched as `curl | bash`, stdin is the pipe (not the keyboard).
# Rebind stdin to the controlling terminal so menus/prompts work.
ensure_interactive_stdin() {
  if [[ "$NON_INTERACTIVE" -eq 1 ]]; then
    return 0
  fi
  if [[ -t 0 ]]; then
    return 0
  fi
  if [[ -r /dev/tty ]]; then
    # shellcheck disable=SC2094
    exec </dev/tty || die "Cannot open /dev/tty for interactive input"
    log_info "Interactive input attached via /dev/tty (safe for curl|bash)"
    return 0
  fi
  # No TTY available
  if [[ -n "$CLI_MODE" && "$CLI_MODE" != "configure" && "$CLI_MODE" != "uninstall" ]]; then
    NON_INTERACTIVE=1
    log_warn "No TTY; mode=${CLI_MODE} will use defaults (non-interactive)"
    return 0
  fi
  die "No interactive terminal. Prefer: curl -sSL ... -o /tmp/ip.sh && sudo bash /tmp/ip.sh  OR  sudo bash /opt/pgpanel/deploy/install.sh --mode resume"
}

# Read a line from the user; fails cleanly on EOF (no infinite empty loops).
read_user() {
  # usage: read_user [-s] "prompt" -> sets REPLY
  local silent=0
  if [[ "${1:-}" == "-s" ]]; then
    silent=1
    shift
  fi
  local prompt="${1:-}"
  REPLY=""
  if [[ $silent -eq 1 ]]; then
    if ! read -r -s -p "$prompt" REPLY; then
      printf '\n'
      return 1
    fi
    printf '\n'
  else
    if ! read -r -p "$prompt" REPLY; then
      return 1
    fi
  fi
  # Strip CR (Windows paste) and outer whitespace
  REPLY="$(printf '%s' "$REPLY" | tr -d '\r' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
  return 0
}

confirm() {
  local prompt="${1:-Continue?}" default="${2:-N}"
  local def_up ans ans_low
  def_up="$(printf '%s' "$default" | tr '[:lower:]' '[:upper:]')"
  if [[ "$NON_INTERACTIVE" -eq 1 ]]; then
    [[ "$def_up" == "Y" ]]
    return
  fi
  if [[ "$def_up" == "Y" ]]; then
    read_user "${prompt} [Y/n] " || return 1
    ans="${REPLY}"
    ans_low="$(printf '%s' "${ans:-y}" | tr '[:upper:]' '[:lower:]')"
    [[ -z "$ans" || "$ans_low" == "y" || "$ans_low" == "yes" ]]
  else
    read_user "${prompt} [y/N] " || return 1
    ans="${REPLY}"
    ans_low="$(printf '%s' "${ans:-n}" | tr '[:upper:]' '[:lower:]')"
    [[ "$ans_low" == "y" || "$ans_low" == "yes" ]]
  fi
}

prompt_val() {
  # prompt_val VAR "Label" "default"
  local __var="$1" __label="$2" __default="${3:-}"
  local __input=""
  if [[ "$NON_INTERACTIVE" -eq 1 ]]; then
    if [[ -z "${!__var:-}" && -n "$__default" ]]; then
      printf -v "$__var" '%s' "$__default"
    fi
    return 0
  fi
  if [[ -n "$__default" ]]; then
    read_user "${__label} [${__default}]: " || die "Input closed while reading ${__label}"
    __input="${REPLY}"
    printf -v "$__var" '%s' "${__input:-$__default}"
  else
    read_user "${__label}: " || die "Input closed while reading ${__label}"
    __input="${REPLY}"
    printf -v "$__var" '%s' "$__input"
  fi
}

prompt_secret() {
  # prompt_secret VAR "Label" [allow_empty=0]
  local __var="$1" __label="$2" __allow_empty="${3:-0}"
  local __input=""
  if [[ "$NON_INTERACTIVE" -eq 1 ]]; then
    return 0
  fi
  while true; do
    read_user -s "${__label}: " || die "Input closed while reading secret"
    __input="${REPLY}"
    if [[ -n "$__input" || "$__allow_empty" -eq 1 ]]; then
      printf -v "$__var" '%s' "$__input"
      return 0
    fi
    log_warn "Value cannot be empty"
  done
}

prompt_yesno() {
  # prompt_yesno VAR "Label" default_y_or_n
  local __var="$1" __label="$2" __def="${3:-n}"
  if [[ "$NON_INTERACTIVE" -eq 1 ]]; then
    if [[ -z "${!__var:-}" ]]; then
      local __def_low
      __def_low="$(printf '%s' "$__def" | tr '[:upper:]' '[:lower:]')"
      if [[ "$__def_low" == "y" ]]; then printf -v "$__var" '1'; else printf -v "$__var" '0'; fi
    fi
    return 0
  fi
  local ans hint
  local def_low ans_low
  def_low="$(printf '%s' "$__def" | tr '[:upper:]' '[:lower:]')"
  if [[ "$def_low" == "y" ]]; then hint="Y/n"; else hint="y/N"; fi
  read_user "${__label} [${hint}]: " || die "Input closed while reading ${__label}"
  ans="${REPLY:-$__def}"
  ans_low="$(printf '%s' "$ans" | tr '[:upper:]' '[:lower:]')"
  if [[ "$ans_low" == "y" || "$ans_low" == "yes" ]]; then
    printf -v "$__var" '1'
  else
    printf -v "$__var" '0'
  fi
}

gen_secret() {
  # 48 bytes base64, no newlines
  openssl rand -base64 48 | tr -d '\n'
}

gen_password() {
  # 24+ chars cryptographically secure, alnum + safe symbols
  openssl rand -base64 32 | tr -d '\n=/' | head -c 32
}

write_secure_file() {
  local path="$1" content="$2" mode="${3:-0600}"
  local dir
  dir="$(dirname "$path")"
  mkdir -p "$dir"
  umask 077
  local tmp
  tmp="$(mktemp "${dir}/.tmp.XXXXXX")"
  printf '%s\n' "$content" >"$tmp"
  chmod "$mode" "$tmp"
  chown root:root "$tmp" 2>/dev/null || true
  mv -f "$tmp" "$path"
  chmod "$mode" "$path"
  chown root:root "$path" 2>/dev/null || true
}

append_log_section() {
  local title="$1"
  {
    echo ""
    echo "===== ${title} ====="
    date -u +%Y-%m-%dT%H:%M:%SZ
  } >>"$LOG_FILE"
}

timeout_cmd() {
  local secs="$1"; shift
  if have_cmd timeout; then
    timeout "$secs" "$@"
  elif have_cmd gtimeout; then
    gtimeout "$secs" "$@"
  else
    "$@"
  fi
}

# ── OS detection (multi-distro) ──────────────────────────────────────────────
OS_ID=""
OS_VERSION_ID=""
OS_LIKE=""
OS_PRETTY=""
PKG_MGR=""
ARCH=""

detect_os() {
  ARCH="$(uname -m)"
  case "$ARCH" in
    x86_64|amd64|aarch64|arm64) ;;
    *) log_warn "Unusual architecture: $ARCH (amd64/arm64 recommended)" ;;
  esac

  if [[ -f /etc/os-release ]]; then
    # shellcheck source=/dev/null
    . /etc/os-release
    OS_ID="${ID:-unknown}"
    OS_VERSION_ID="${VERSION_ID:-}"
    OS_LIKE="${ID_LIKE:-}"
    OS_PRETTY="${PRETTY_NAME:-$OS_ID}"
  else
    OS_ID="unknown"
    OS_PRETTY="$(uname -s)"
  fi

  case "$OS_ID" in
    ubuntu|debian|raspbian|linuxmint|pop|elementary|zorin)
      PKG_MGR="apt"
      ;;
    rhel|centos|rocky|almalinux|ol|oracle|amzn|fedora)
      if have_cmd dnf; then PKG_MGR="dnf"; else PKG_MGR="yum"; fi
      ;;
    opensuse*|sles|suse)
      PKG_MGR="zypper"
      ;;
    arch|manjaro|endeavouros|garuda)
      PKG_MGR="pacman"
      ;;
    alpine)
      PKG_MGR="apk"
      ;;
    *)
      # Fallback via ID_LIKE
      if [[ "$OS_LIKE" == *debian* || "$OS_LIKE" == *ubuntu* ]]; then
        PKG_MGR="apt"
      elif [[ "$OS_LIKE" == *rhel* || "$OS_LIKE" == *fedora* || "$OS_LIKE" == *centos* ]]; then
        if have_cmd dnf; then PKG_MGR="dnf"; else PKG_MGR="yum"; fi
      elif [[ "$OS_LIKE" == *suse* ]]; then
        PKG_MGR="zypper"
      elif [[ "$OS_LIKE" == *arch* ]]; then
        PKG_MGR="pacman"
      else
        PKG_MGR="unknown"
      fi
      ;;
  esac

  log_info "OS: ${OS_PRETTY} (${OS_ID} ${OS_VERSION_ID}) arch=${ARCH} pkg=${PKG_MGR}"
}

is_primary_supported() {
  case "${OS_ID}:${OS_VERSION_ID}" in
    ubuntu:22.04|ubuntu:24.04|ubuntu:20.04|debian:12*|debian:11*|debian:13*) return 0 ;;
    rocky:8*|rocky:9*|almalinux:8*|almalinux:9*|rhel:8*|rhel:9*|fedora:*|amzn:2023) return 0 ;;
    opensuse-leap:15*|arch|manjaro) return 0 ;;
    *) return 1 ;;
  esac
}

# ── System checks ────────────────────────────────────────────────────────────
cpu_cores() { nproc 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1; }

ram_mb() {
  if [[ -r /proc/meminfo ]]; then
    awk '/MemTotal/ {printf "%d", $2/1024}' /proc/meminfo
  else
    echo 0
  fi
}

disk_free_gb() {
  local path="${1:-/}"
  df -Pk "$path" 2>/dev/null | awk 'NR==2 {printf "%d", $4/1024/1024}'
}

public_ip() {
  timeout_cmd "$NETWORK_TIMEOUT" curl -4 -fsS https://ifconfig.me 2>/dev/null \
    || timeout_cmd "$NETWORK_TIMEOUT" curl -4 -fsS https://api.ipify.org 2>/dev/null \
    || hostname -I 2>/dev/null | awk '{print $1}' \
    || echo "unknown"
}

port_in_use() {
  local port="$1"
  if have_cmd ss; then
    ss -lnt "( sport = :$port )" 2>/dev/null | grep -q ":$port"
  elif have_cmd netstat; then
    netstat -lnt 2>/dev/null | grep -q ":$port "
  else
    return 1
  fi
}

check_internet() {
  timeout_cmd "$NETWORK_TIMEOUT" curl -fsS -o /dev/null https://1.1.1.1 || \
  timeout_cmd "$NETWORK_TIMEOUT" curl -fsS -o /dev/null https://github.com || \
  return 1
}

check_dns() {
  local host="$1"
  [[ -z "$host" ]] && return 1
  if have_cmd getent; then
    getent ahostsv4 "$host" >/dev/null 2>&1 && return 0
  fi
  if have_cmd dig; then
    dig +short "$host" A 2>/dev/null | grep -qE '^[0-9.]+$' && return 0
  fi
  if have_cmd host; then
    host "$host" 2>/dev/null | grep -q 'has address' && return 0
  fi
  return 1
}

dns_points_to_this_host() {
  local domain="$1"
  local vip dip
  vip="$(public_ip)"
  [[ "$vip" == "unknown" || -z "$domain" ]] && return 1
  if have_cmd dig; then
    dip="$(dig +short "$domain" A 2>/dev/null | head -1)"
  elif have_cmd getent; then
    dip="$(getent ahostsv4 "$domain" 2>/dev/null | awk '{print $1; exit}')"
  else
    return 1
  fi
  [[ -n "$dip" && "$dip" == "$vip" ]]
}

existing_install() {
  [[ -f "${PGPANEL_CONF}" ]] || [[ -f "${INSTALL_DIR}/.env" ]] || [[ -d "${INSTALL_DIR}/deploy" ]]
}

preflight_checks() {
  log_step 1 15 "Rendszer ellenőrzése"
  detect_os

  if ! is_primary_supported; then
    log_warn "OS ${OS_PRETTY} is not in the primary test matrix; continuing best-effort"
    if [[ "$MINIMAL_INSTALL" -eq 1 ]]; then
      log_warn "OS ${OS_PRETTY} is outside the primary test matrix; continuing with defaults"
    elif ! confirm "Continue on untested OS?" "N"; then
      die "Aborted by user"
    fi
  fi

  local cores ram disk
  cores="$(cpu_cores)"
  ram="$(ram_mb)"
  disk="$(disk_free_gb /)"

  cat <<EOF

${C_BOLD}Minimum:${C_RESET}
  ${MIN_CPU} CPU · ${MIN_RAM_MB} MB RAM · ${MIN_DISK_GB} GB free disk

${C_BOLD}Recommended:${C_RESET}
  ${REC_CPU} CPU · ${REC_RAM_MB} MB RAM · ${REC_DISK_GB} GB NVMe

${C_BOLD}This host:${C_RESET}
  CPU cores: ${cores}
  RAM:       ${ram} MB
  Free disk: ${disk} GB
  Arch:      ${ARCH}

EOF

  local below=0
  (( cores < MIN_CPU )) && below=1
  (( ram < MIN_RAM_MB )) && below=1
  (( disk < MIN_DISK_GB )) && below=1
  if [[ $below -eq 1 ]]; then
    log_warn "Host is below minimum requirements"
    if [[ "$MINIMAL_INSTALL" -eq 1 ]]; then
      log_warn "Host is below the recommended minimum; continuing with defaults"
    elif ! confirm "Continue anyway?" "N"; then
      die "Aborted due to insufficient resources"
    fi
  fi

  if check_internet; then
    log_ok "Internet connectivity"
  else
    die "No internet connectivity (required for packages/images)"
  fi

  if have_cmd docker; then
    log_ok "Docker CLI present: $(docker --version 2>/dev/null || true)"
    if docker info >/dev/null 2>&1; then
      log_ok "Docker daemon running"
    else
      log_warn "Docker installed but daemon not reachable"
    fi
  else
    log_info "Docker not installed (will install)"
  fi

  if docker compose version >/dev/null 2>&1; then
    log_ok "Docker Compose plugin present"
  else
    log_info "Docker Compose plugin missing (will install)"
  fi

  for c in curl openssl; do
    if have_cmd "$c"; then log_ok "$c present"; else log_warn "$c missing (will install)"; fi
  done
  if have_cmd git; then log_ok "git present"; else log_warn "git missing (will install)"; fi
  if have_cmd ufw; then log_ok "ufw present"; else log_info "ufw not present (optional)"; fi

  if port_in_use 80; then log_warn "Port 80 is in use"; fi
  if port_in_use 443; then log_warn "Port 443 is in use"; fi

  if existing_install; then
    log_warn "Existing PgPanel installation detected"
  fi
}

# ── Package management ───────────────────────────────────────────────────────
pkg_update() {
  case "$PKG_MGR" in
    apt)    export DEBIAN_FRONTEND=noninteractive; apt-get update -qq ;;
    dnf)    dnf -y makecache ;;
    yum)    yum -y makecache ;;
    zypper) zypper --non-interactive refresh ;;
    pacman) pacman -Sy --noconfirm ;;
    apk)    apk update ;;
    *)      log_warn "Unknown package manager; skipping update" ;;
  esac
}

pkg_install() {
  local packages=("$@")
  [[ ${#packages[@]} -eq 0 ]] && return 0
  case "$PKG_MGR" in
    apt)
      export DEBIAN_FRONTEND=noninteractive
      apt-get install -y -qq "${packages[@]}"
      ;;
    dnf)    dnf install -y "${packages[@]}" ;;
    yum)    yum install -y "${packages[@]}" ;;
    zypper) zypper --non-interactive install -y "${packages[@]}" ;;
    pacman) pacman -S --noconfirm --needed "${packages[@]}" ;;
    apk)    apk add --no-cache "${packages[@]}" ;;
    *)      die "Cannot install packages: unknown package manager ($PKG_MGR)" ;;
  esac
}

install_base_packages() {
  pkg_update

  local pkgs=()
  case "$PKG_MGR" in
    apt)
      pkgs=(ca-certificates curl gnupg lsb-release git openssl jq ufw coreutils findutils procps iproute2 dnsutils)
      ;;
    dnf|yum)
      pkgs=(ca-certificates curl gnupg2 git openssl jq firewalld coreutils findutils procps-ng iproute bind-utils)
      ;;
    zypper)
      pkgs=(ca-certificates curl gpg2 git openssl jq coreutils findutils procps iproute2 bind-utils)
      ;;
    pacman)
      pkgs=(ca-certificates curl gnupg git openssl jq coreutils findutils procps-ng iproute2 bind)
      ;;
    apk)
      pkgs=(ca-certificates curl gnupg git openssl jq coreutils findutils procps iproute2 bind-tools)
      ;;
  esac

  # Filter already present optional packages gently
  pkg_install "${pkgs[@]}" || log_warn "Some packages failed; continuing if critical tools exist"
  require_cmd curl
  require_cmd openssl
  log_ok "Base packages installed"
}

# ── Docker install ───────────────────────────────────────────────────────────
install_docker() {
  # Note: log_step may be called by run_step already — safe to call again for standalone use
  if have_cmd docker && docker info >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    log_ok "Docker already operational"
    # Never pipe docker into head under pipefail (SIGPIPE → exit 141)
    local dv cv
    dv="$(docker version --format 'Client {{.Client.Version}} / Server {{.Server.Version}}' 2>/dev/null || true)"
    cv="$(docker compose version --short 2>/dev/null || true)"
    [[ -n "$dv" ]] && log_info "  Docker: ${dv}"
    [[ -n "$cv" ]] && log_info "  Compose: ${cv}"
    return 0
  fi

  case "$PKG_MGR" in
    apt)
      install -m 0755 -d /etc/apt/keyrings
      local docker_os="$OS_ID"
      [[ "$OS_ID" == "linuxmint" || "$OS_ID" == "pop" || "$OS_ID" == "elementary" ]] && docker_os="ubuntu"
      if [[ ! -f /etc/apt/keyrings/docker.asc ]]; then
        timeout_cmd 60 curl -fsSL "https://download.docker.com/linux/${docker_os}/gpg" -o /etc/apt/keyrings/docker.asc
        chmod a+r /etc/apt/keyrings/docker.asc
      fi
      local codename
      codename="$(. /etc/os-release && echo "${VERSION_CODENAME:-stable}")"
      # Mint uses ubuntu codename via UBUNTU_CODENAME
      if [[ -n "${UBUNTU_CODENAME:-}" ]]; then codename="$UBUNTU_CODENAME"; fi
      echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/${docker_os} ${codename} stable" \
        > /etc/apt/sources.list.d/docker.list
      apt-get update -qq
      apt-get install -y -qq docker-ce docker-ce-cli containerd.io docker-compose-plugin docker-buildx-plugin
      ;;
    dnf|yum)
      if [[ "$OS_ID" == "fedora" ]]; then
        dnf -y install dnf-plugins-core
        dnf config-manager --add-repo https://download.docker.com/linux/fedora/docker-ce.repo
        dnf install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
      elif [[ "$OS_ID" == "amzn" ]]; then
        dnf install -y docker
        # Compose plugin may need separate install
        if ! docker compose version >/dev/null 2>&1; then
          mkdir -p /usr/local/lib/docker/cli-plugins
          local arch_dl="x86_64"
          [[ "$ARCH" == "aarch64" || "$ARCH" == "arm64" ]] && arch_dl="aarch64"
          timeout_cmd 120 curl -fsSL \
            "https://github.com/docker/compose/releases/download/v2.32.4/docker-compose-linux-${arch_dl}" \
            -o /usr/local/lib/docker/cli-plugins/docker-compose
          chmod +x /usr/local/lib/docker/cli-plugins/docker-compose
        fi
      else
        # RHEL-like
        dnf -y install dnf-plugins-core || yum -y install yum-utils
        dnf config-manager --add-repo https://download.docker.com/linux/centos/docker-ce.repo 2>/dev/null \
          || yum-config-manager --add-repo https://download.docker.com/linux/centos/docker-ce.repo
        dnf install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin \
          || yum install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
      fi
      ;;
    zypper)
      zypper --non-interactive install -y docker docker-compose
      ;;
    pacman)
      pacman -S --noconfirm --needed docker docker-compose
      ;;
    apk)
      apk add --no-cache docker docker-cli-compose
      ;;
    *)
      die "Automated Docker install not supported for pkg manager: $PKG_MGR. Install Docker manually, then re-run."
      ;;
  esac

  if have_cmd systemctl; then
    systemctl enable --now docker
  elif have_cmd service; then
    service docker start || true
  fi

  # Wait for daemon
  local i
  for i in $(seq 1 30); do
    if docker info >/dev/null 2>&1; then break; fi
    sleep 1
  done

  if ! docker info >/dev/null 2>&1; then
    die "Docker installed but daemon not reachable (try: systemctl start docker)"
  fi
  if ! docker compose version >/dev/null 2>&1; then
    die "Docker Compose plugin missing after install"
  fi
  log_ok "Docker Engine and Compose ready"
  local dv cv
  dv="$(docker version --format 'Client {{.Client.Version}} / Server {{.Server.Version}}' 2>/dev/null || true)"
  cv="$(docker compose version --short 2>/dev/null || true)"
  [[ -n "$dv" ]] && log_info "  Docker: ${dv}"
  [[ -n "$cv" ]] && log_info "  Compose: ${cv}"
}

# ── Directories ──────────────────────────────────────────────────────────────
create_directories() {
  local dirs=(
    "$INSTALL_DIR"
    "$DATA_DIR"
    "${DATA_DIR}/panel"
    "${DATA_DIR}/clusters"
    "${DATA_DIR}/databasus"
    "$BACKUP_CACHE_DIR"
    "$LOG_DIR"
    "$PGPANEL_ETC_DIR"
    "${INSTALL_DIR}/deploy"
    "${INSTALL_DIR}/backups/config"
  )
  local d
  for d in "${dirs[@]}"; do
    mkdir -p "$d"
  done
  chmod 700 "$DATA_DIR" "${DATA_DIR}/panel" "${DATA_DIR}/clusters" "${DATA_DIR}/databasus" "$BACKUP_CACHE_DIR"
  chmod 755 "$INSTALL_DIR" "$LOG_DIR" "$PGPANEL_ETC_DIR"
  chown -R root:root "$PGPANEL_ETC_DIR" "$INSTALL_DIR" 2>/dev/null || true
  log_ok "Directories ready"
}

# ── Version helpers ──────────────────────────────────────────────────────────
read_version_file() {
  local f="$1"
  if [[ -f "$f" ]]; then
    tr -d ' \n\r' <"$f"
  else
    echo "unknown"
  fi
}

get_local_version() {
  if [[ -f "${INSTALL_DIR}/VERSION" ]]; then
    read_version_file "${INSTALL_DIR}/VERSION"
  elif [[ -f "${REPO_ROOT}/VERSION" ]]; then
    read_version_file "${REPO_ROOT}/VERSION"
  else
    echo "${PGPANEL_VERSION:-unknown}"
  fi
}

get_remote_version() {
  # Prefer VERSION on selected ref (stable → main or latest release tag)
  local ref="${1:-$GIT_REF}"
  local url="https://raw.githubusercontent.com/pgpanel/pgpanel/${ref}/VERSION"
  local v
  v="$(timeout_cmd "$NETWORK_TIMEOUT" curl -fsSL "$url" 2>/dev/null | tr -d ' \n\r' || true)"
  if [[ -n "$v" ]]; then
    printf '%s' "$v"
    return 0
  fi
  # Fallback: latest GitHub release tag
  local rel_json
  rel_json="$(timeout_cmd "$NETWORK_TIMEOUT" curl -fsSL https://api.github.com/repos/pgpanel/pgpanel/releases/latest 2>/dev/null || true)"
  v="$(printf '%s' "$rel_json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1 | sed 's/^v//')"
  if [[ -n "$v" ]]; then
    printf '%s' "$v"
    return 0
  fi
  printf 'unknown'
}

resolve_panel_image() {
  PGPANEL_VERSION="$(get_local_version)"
  if [[ -z "$PGPANEL_IMAGE" ]]; then
    if [[ "$UPDATE_CHANNEL" == "edge" ]]; then
      PGPANEL_IMAGE="${PGPANEL_GHCR_IMAGE}:latest"
    else
      PGPANEL_IMAGE="${PGPANEL_GHCR_IMAGE}:${PGPANEL_VERSION}"
    fi
  fi
}

show_version_status() {
  local local_v remote_v
  local_v="$(get_local_version)"
  remote_v="$(get_remote_version "$GIT_REF")"
  PGPANEL_VERSION="$local_v"
  resolve_panel_image
  echo ""
  echo "${C_BOLD}Version status${C_RESET}"
  echo "  Installed (tree):  ${local_v}"
  echo "  Remote (${GIT_REF}): ${remote_v}"
  echo "  Panel image:       ${PGPANEL_IMAGE}"
  echo "  Channel:           ${UPDATE_CHANNEL}"
  echo "  Installer:         ${INSTALLER_VERSION}"
  echo "  Developer:         ${PGPANEL_AUTHOR}"
  if [[ "$local_v" != "unknown" && "$remote_v" != "unknown" && "$local_v" != "$remote_v" ]]; then
    log_warn "Update available: ${local_v} → ${remote_v}"
    return 0
  elif [[ "$local_v" == "$remote_v" && "$local_v" != "unknown" ]]; then
    log_ok "Already on latest known version (${local_v})"
    return 1
  fi
  return 0
}

# ── Repository (official only — never asked) ─────────────────────────────────
clone_or_update_repo() {
  REPO_URL="$PGPANEL_OFFICIAL_REPO"

  # Prefer syncing from current tree if we are already inside this checkout
  if [[ -f "${REPO_ROOT}/Cargo.toml" && -d "${REPO_ROOT}/deploy" && -f "${REPO_ROOT}/VERSION" ]]; then
    if [[ "$REPO_ROOT" != "$INSTALL_DIR" ]]; then
      log_info "Syncing ${REPO_ROOT} → ${INSTALL_DIR}"
      mkdir -p "$INSTALL_DIR"
      if have_cmd rsync; then
        rsync -a --delete \
          --exclude '.git' \
          --exclude 'target' \
          --exclude 'frontend/node_modules' \
          --exclude 'frontend/.svelte-kit' \
          --exclude 'frontend/build' \
          --exclude 'data' \
          --exclude '.env' \
          "${REPO_ROOT}/" "${INSTALL_DIR}/"
      else
        tar -C "$REPO_ROOT" \
          --exclude='.git' --exclude='target' --exclude='frontend/node_modules' \
          --exclude='frontend/.svelte-kit' --exclude='frontend/build' \
          --exclude='data' --exclude='.env' \
          -cf - . | tar -C "$INSTALL_DIR" -xf -
      fi
    else
      log_info "Already running from install directory"
    fi
  fi

  mkdir -p "$(dirname "$INSTALL_DIR")"

  if [[ -d "${INSTALL_DIR}/.git" ]]; then
    git -C "$INSTALL_DIR" remote set-url origin "$REPO_URL" 2>/dev/null || true
    git -C "$INSTALL_DIR" fetch --tags --force origin
    if git -C "$INSTALL_DIR" rev-parse --verify "origin/${GIT_REF}" >/dev/null 2>&1; then
      git -C "$INSTALL_DIR" checkout -B "$GIT_REF" "origin/${GIT_REF}"
      git -C "$INSTALL_DIR" merge --ff-only "origin/${GIT_REF}" || true
    elif git -C "$INSTALL_DIR" rev-parse --verify "refs/tags/${GIT_REF}" >/dev/null 2>&1; then
      git -C "$INSTALL_DIR" checkout "tags/${GIT_REF}"
    elif git -C "$INSTALL_DIR" rev-parse --verify "refs/tags/v${GIT_REF}" >/dev/null 2>&1; then
      git -C "$INSTALL_DIR" checkout "tags/v${GIT_REF}"
    else
      log_warn "Ref ${GIT_REF} not found after fetch — keeping current checkout"
    fi
  elif [[ ! -f "${INSTALL_DIR}/deploy/install.sh" ]]; then
    log_info "Cloning ${REPO_URL} (${GIT_REF}) → ${INSTALL_DIR}"
    git clone --branch "$GIT_REF" --single-branch "$REPO_URL" "$INSTALL_DIR" \
      || git clone "$REPO_URL" "$INSTALL_DIR" \
      || die "git clone failed"
    git -C "$INSTALL_DIR" checkout "$GIT_REF" 2>/dev/null || true
  else
    log_info "Using existing tree at ${INSTALL_DIR}"
  fi

  PGPANEL_VERSION="$(get_local_version)"
  resolve_panel_image
  log_info "Version ${PGPANEL_VERSION} · commit $(git -C "$INSTALL_DIR" rev-parse --short HEAD 2>/dev/null || echo n/a)"
  log_info "Panel image ${PGPANEL_IMAGE}"
  log_ok "Repository ready"
}

# ── Secrets / env ────────────────────────────────────────────────────────────
upsert_env_key() {
  local file="$1" key="$2" val="$3"
  [[ -f "$file" ]] || return 0
  if grep -q "^${key}=" "$file" 2>/dev/null; then
    local tmp
    tmp="$(mktemp)"
    awk -v k="$key" -v v="$val" 'BEGIN{FS=OFS="="} $1==k {$0=k"="v} {print}' "$file" >"$tmp"
    mv -f "$tmp" "$file"
  else
    printf '%s=%s\n' "$key" "$val" >>"$file"
  fi
  chmod 0600 "$file"
  chown root:root "$file" 2>/dev/null || true
}

generate_or_load_secrets() {
  local env_file="${INSTALL_DIR}/.env"

  resolve_panel_image

  if [[ -f "$env_file" ]]; then
    log_ok "Existing .env found — secrets will NOT be regenerated"
    chmod 0600 "$env_file"
    chown root:root "$env_file" 2>/dev/null || true
    # shellcheck source=/dev/null
    set -a
    # shellcheck disable=SC1090
    source "$env_file"
    set +a
    # Refresh non-secret image/version pins for pull-based updates
    upsert_env_key "$env_file" "PGPANEL_VERSION" "$PGPANEL_VERSION"
    upsert_env_key "$env_file" "PGPANEL_IMAGE" "$PGPANEL_IMAGE"
    upsert_env_key "$env_file" "PGPANEL_HOST_DATA" "$DATA_DIR"
    # Networking / backup defaults for upgrades
    upsert_env_key "$env_file" "PGPANEL_MANAGEMENT_NETWORK" "pgpanel_database_management"
    upsert_env_key "$env_file" "BACKUP_STORAGE_TYPE" "${BACKUP_STORAGE_TYPE:-local}"
    upsert_env_key "$env_file" "PGPANEL_BACKUP_DIR" "/var/lib/pgpanel/backups"
    # Admin identity and bootstrap credentials belong to SQLite, not .env.
    # Keep legacy S3 keys temporarily for upgrade compatibility; new installs
    # configure storage from the web UI and never write them here.
    local cleaned_env
    cleaned_env="$(mktemp)"
    sed -E '/^(PGPANEL_BOOTSTRAP_TOKEN|PGPANEL_ADMIN_USERNAME|PGPANEL_ADMIN_EMAIL|DATABASUS_ADMIN_EMAIL|DATABASUS_ADMIN_PASSWORD)=/d' \
      "$env_file" >"$cleaned_env"
    chmod 0600 "$cleaned_env"
    chown root:root "$cleaned_env" 2>/dev/null || true
    mv -f "$cleaned_env" "$env_file"
    return 0
  fi

  umask 077
  local master session csrf api_sign internal_api docker_suffix
  master="$(gen_secret)"
  session="$(gen_secret)"
  csrf="$(gen_secret)"
  api_sign="$(gen_secret)"
  internal_api="$(gen_secret)"
  docker_suffix="$(openssl rand -hex 4)"

  local cookie_secure="true"
  [[ "$ENABLE_HTTPS" -eq 0 ]] && cookie_secure="false"

  cat >"$env_file" <<EOF
# PgPanel environment — generated by install.sh ${INSTALLER_VERSION}
# Mode 0600, owner root. DO NOT commit. DO NOT log contents.

# ── Core ────────────────────────────────────────────────────────────────────
PGPANEL_MASTER_KEY=${master}
PGPANEL_SESSION_SECRET=${session}
PGPANEL_CSRF_SECRET=${csrf}
PGPANEL_API_SIGNING_SECRET=${api_sign}
PGPANEL_INTERNAL_API_TOKEN=${internal_api}
PGPANEL_DOCKER_NETWORK_SUFFIX=${docker_suffix}

PGPANEL_BIND=0.0.0.0:8080
PGPANEL_DATA_DIR=/var/lib/pgpanel
PGPANEL_STATIC_DIR=/app/static
PGPANEL_DATABASE_URL=sqlite:///var/lib/pgpanel/panel.db?mode=rwc
PGPANEL_COOKIE_SECURE=${cookie_secure}
PGPANEL_SESSION_TTL_HOURS=${SESSION_TTL_HOURS}
PGPANEL_LOGIN_MAX_ATTEMPTS=${LOGIN_MAX_ATTEMPTS}
PGPANEL_DOMAIN=${PANEL_DOMAIN}
CADDY_EMAIL=${LETSENCRYPT_EMAIL}
PGPANEL_UPDATE_CHANNEL=${UPDATE_CHANNEL}
PGPANEL_VERSION=${PGPANEL_VERSION}
PGPANEL_IMAGE=${PGPANEL_IMAGE}
PGPANEL_HOST_DATA=${DATA_DIR}
PGPANEL_PULL_POLICY=missing

# Shared Docker network: panel + PostgreSQL clusters (DNS for pgpanel_pg_*)
PGPANEL_MANAGEMENT_NETWORK=pgpanel_database_management
PGPANEL_NETWORK_PREFIX=pgpanel_net_
PGPANEL_VOLUME_PREFIX=pgpanel_vol_

# ── Native backup engine (Databasus removed) ────────────────────────────────
BACKUP_STORAGE_TYPE=${BACKUP_STORAGE_TYPE:-local}
BACKUP_ENCRYPT=1
PGPANEL_BACKUP_DIR=/var/lib/pgpanel/backups
BACKUP_RETENTION_DAYS=${BACKUP_RETENTION_DAYS}
BACKUP_MAX_COUNT=${BACKUP_MAX_COUNT}
EOF

  chmod 0600 "$env_file"
  chown root:root "$env_file"
  log_ok ".env written (${env_file})"

}

# ── Config file (non-secret) ─────────────────────────────────────────────────
save_installer_conf() {
  mkdir -p "$PGPANEL_ETC_DIR"
  cat >"$PGPANEL_CONF" <<EOF
# PgPanel installer configuration (non-secret)
# Generated: ${STARTED_AT}
# Installer version: ${INSTALLER_VERSION}

INSTALL_DIR=${INSTALL_DIR}
DATA_DIR=${DATA_DIR}
CLUSTER_DATA_DIR=${CLUSTER_DATA_DIR}
LOG_DIR=${LOG_DIR}
BACKUP_CACHE_DIR=${BACKUP_CACHE_DIR}
REPO_URL=${PGPANEL_OFFICIAL_REPO}
GIT_REF=${GIT_REF}
UPDATE_CHANNEL=${UPDATE_CHANNEL}
PGPANEL_VERSION=${PGPANEL_VERSION}
PGPANEL_IMAGE=${PGPANEL_IMAGE}

USE_DOMAIN=${USE_DOMAIN}
PANEL_DOMAIN=${PANEL_DOMAIN}
DATABASUS_DOMAIN=${DATABASUS_DOMAIN}
DATABASUS_PUBLIC=${DATABASUS_PUBLIC}
LETSENCRYPT_EMAIL=${LETSENCRYPT_EMAIL}
ENABLE_HTTPS=${ENABLE_HTTPS}
PUBLIC_PG_PORTS=${PUBLIC_PG_PORTS}
ADMIN_IP_ALLOWLIST=${ADMIN_IP_ALLOWLIST}
SSH_PORT=${SSH_PORT}
CONFIGURE_UFW=${CONFIGURE_UFW}
BEHIND_CLOUDFLARE=${BEHIND_CLOUDFLARE}
USE_CLOUDFLARE_TUNNEL=${USE_CLOUDFLARE_TUNNEL}

ADMIN_USERNAME=${ADMIN_USERNAME}
ADMIN_EMAIL=${ADMIN_EMAIL}
PREPARE_2FA=${PREPARE_2FA}
SESSION_TTL_HOURS=${SESSION_TTL_HOURS}
LOGIN_MAX_ATTEMPTS=${LOGIN_MAX_ATTEMPTS}

PG_DEFAULT_VERSION=${PG_DEFAULT_VERSION}
PG_ALLOWED_VERSIONS=${PG_ALLOWED_VERSIONS}
PG_DEFAULT_CPU=${PG_DEFAULT_CPU}
PG_DEFAULT_MEMORY_MB=${PG_DEFAULT_MEMORY_MB}
PG_DEFAULT_STORAGE_GB=${PG_DEFAULT_STORAGE_GB}
PG_TIMEZONE=${PG_TIMEZONE}
PG_LOCALE=${PG_LOCALE}
PG_MAX_CLUSTERS=${PG_MAX_CLUSTERS}
PG_CLUSTER_PREFIX=${PG_CLUSTER_PREFIX}
PG_AUTO_RESTART=${PG_AUTO_RESTART}

ENABLE_DATABASUS=${ENABLE_DATABASUS}
BACKUP_STORAGE_TYPE=${BACKUP_STORAGE_TYPE}
BACKUP_RETENTION_DAYS=${BACKUP_RETENTION_DAYS}
BACKUP_FULL_FREQ=${BACKUP_FULL_FREQ}
BACKUP_INCR_FREQ=${BACKUP_INCR_FREQ}
BACKUP_WAL_STREAMING=${BACKUP_WAL_STREAMING}
BACKUP_RESTORE_VERIFY=${BACKUP_RESTORE_VERIFY}
NOTIFY_TYPE=${NOTIFY_TYPE}
ENABLE_WATCHTOWER=${ENABLE_WATCHTOWER}
TIMEZONE_SET=${TIMEZONE_SET}
CREATE_SWAP=${CREATE_SWAP}
SWAP_SIZE_MB=${SWAP_SIZE_MB}
PRODUCTION_READY=0
INSTALLED_AT=${STARTED_AT}
EOF
  chmod 0640 "$PGPANEL_CONF"
  chown root:root "$PGPANEL_CONF"
  log_ok "Saved ${PGPANEL_CONF}"
}

load_installer_conf() {
  if [[ -f "$PGPANEL_CONF" ]]; then
    # Older installers wrote INSTALLER_VERSION into this file. It must not be
    # sourced here because the runtime constant above is readonly; keeping the
    # filter also makes upgrades from 1.5.0 safe and idempotent.
    local filtered_conf
    filtered_conf="$(mktemp)"
    sed '/^[[:space:]]*INSTALLER_VERSION[[:space:]]*=/d' "$PGPANEL_CONF" >"$filtered_conf"
    # shellcheck source=/dev/null
    set -a
    # shellcheck disable=SC1090
    source "$filtered_conf"
    set +a
    rm -f "$filtered_conf"
    log_info "Loaded configuration from ${PGPANEL_CONF}"
  fi
}

# ── Compose / Caddy generation ───────────────────────────────────────────────
render_compose() {
  local dest="${INSTALL_DIR}/deploy/compose.yml"
  mkdir -p "${INSTALL_DIR}/deploy"
  resolve_panel_image

  cat >"$dest" <<EOF
# Generated by PgPanel installer ${INSTALLER_VERSION}
# Native backups inside panel — no Databasus service.
# SECURITY: Docker socket mounted into panel only. See SECURITY.md.
#
# Networks:
#   pgpanel_frontend            — Caddy ↔ panel
#   pgpanel_internal            — stack services
#   pgpanel_database_management — panel ↔ PG clusters (internal DNS)

name: pgpanel

networks:
  pgpanel_frontend:
    name: pgpanel_frontend
    driver: bridge
  pgpanel_internal:
    name: pgpanel_internal
    driver: bridge
    internal: false
  pgpanel_database_management:
    name: pgpanel_database_management
    driver: bridge
    internal: true

volumes:
  caddy_data:
  caddy_config:

services:
  caddy:
    image: caddy:2.9-alpine
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    environment:
      PGPANEL_DOMAIN: \${PGPANEL_DOMAIN:-}
      CADDY_EMAIL: \${CADDY_EMAIL:-}
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy_data:/data
      - caddy_config:/config
    networks:
      - pgpanel_frontend
    depends_on:
      panel:
        condition: service_started
    logging:
      driver: json-file
      options:
        max-size: "10m"
        max-file: "5"
    security_opt:
      - no-new-privileges:true
    read_only: true
    tmpfs:
      - /tmp
      - /config/caddy
    cap_drop:
      - ALL
    cap_add:
      - NET_BIND_SERVICE

  panel:
    image: \${PGPANEL_IMAGE:-${PGPANEL_GHCR_IMAGE}:${PGPANEL_VERSION}}
    # The installer pulls the panel image explicitly before startup. `missing`
    # keeps a temporary registry outage from preventing a restart.
    pull_policy: \${PGPANEL_PULL_POLICY:-missing}
    restart: unless-stopped
    env_file:
      - ../.env
    environment:
      PGPANEL_BIND: 0.0.0.0:8080
      PGPANEL_DATA_DIR: /var/lib/pgpanel
      PGPANEL_STATIC_DIR: /app/static
      PGPANEL_DATABASE_URL: sqlite:///var/lib/pgpanel/panel.db?mode=rwc
      DOCKER_HOST: unix:///var/run/docker.sock
      PGPANEL_MANAGEMENT_NETWORK: \${PGPANEL_MANAGEMENT_NETWORK:-pgpanel_database_management}
      PGPANEL_BACKUP_DIR: /var/lib/pgpanel/backups
      BACKUP_STORAGE_TYPE: \${BACKUP_STORAGE_TYPE:-local}
      BACKUP_RETENTION_DAYS: \${BACKUP_RETENTION_DAYS:-14}
      BACKUP_MAX_COUNT: \${BACKUP_MAX_COUNT:-30}
      BACKUP_ENCRYPT: \${BACKUP_ENCRYPT:-1}
    volumes:
      - ${DATA_DIR}/panel:/var/lib/pgpanel
      - /var/run/docker.sock:/var/run/docker.sock
    networks:
      - pgpanel_frontend
      - pgpanel_internal
      - pgpanel_database_management
    expose:
      - "8080"
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://127.0.0.1:8080/health"]
      interval: 30s
      timeout: 5s
      retries: 5
      start_period: 40s
    logging:
      driver: json-file
      options:
        max-size: "20m"
        max-file: "5"
    security_opt:
      - no-new-privileges:true
EOF

  if [[ "${ENABLE_WATCHTOWER:-0}" -eq 1 ]]; then
    cat >>"$dest" <<'EOF'

  watchtower:
    image: containrrr/watchtower:1.7.1
    restart: unless-stopped
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
    command: --interval 86400 --cleanup --include-restarting
    networks:
      - pgpanel_internal
    security_opt:
      - no-new-privileges:true
EOF
  fi

  log_ok "compose.yml rendered → ${dest}"
}

render_caddyfile() {
  local dest="${INSTALL_DIR}/deploy/Caddyfile"
  mkdir -p "${INSTALL_DIR}/deploy"

  if [[ "$ENABLE_HTTPS" -eq 1 && "$USE_DOMAIN" -eq 1 && -n "$PANEL_DOMAIN" ]]; then
    cat >"$dest" <<EOF
# Generated by PgPanel installer — panel only (no Databasus public vhost)
{
	email ${LETSENCRYPT_EMAIL}
}

${PANEL_DOMAIN} {
	encode zstd gzip

	header {
		Strict-Transport-Security "max-age=31536000; includeSubDomains; preload"
		X-Content-Type-Options "nosniff"
		X-Frame-Options "DENY"
		Referrer-Policy "strict-origin-when-cross-origin"
		Permissions-Policy "geolocation=(), microphone=(), camera=()"
		Content-Security-Policy "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"
		-Server
	}

	request_body {
		max_size 10MB
	}

	reverse_proxy panel:8080

	log {
		output stdout
		format console
	}
}
EOF
  else
    log_warn "HTTP-only Caddy config (no domain / HTTPS disabled)"
    cat >"$dest" <<'EOF'
# HTTP-only lab mode — NOT for production admin panels
:80 {
	encode gzip
	header {
		X-Content-Type-Options "nosniff"
		X-Frame-Options "DENY"
		Referrer-Policy "no-referrer"
		-Server
	}
	reverse_proxy panel:8080
	log {
		output stdout
	}
}
EOF
  fi
  log_ok "Caddyfile rendered → ${dest}"
}

# ── Docker networks ──────────────────────────────────────────────────────────
# Compose must own these networks (labels com.docker.compose.*).
# Do NOT pre-create them with plain `docker network create` — that causes:
#   network X was found but has incorrect label com.docker.compose.network
prepare_compose_networks() {
  local nets=(pgpanel_frontend pgpanel_internal pgpanel_database_management)
  local n label
  for n in "${nets[@]}"; do
    if ! docker network inspect "$n" >/dev/null 2>&1; then
      continue
    fi
    label="$(docker network inspect -f '{{index .Labels "com.docker.compose.network"}}' "$n" 2>/dev/null || true)"
    if [[ -z "$label" || "$label" == "<no value>" ]]; then
      log_warn "Removing unlabeled network '$n' so Compose can recreate it"
      # Detach any leftover containers first (best-effort)
      docker network inspect -f '{{range .Containers}}{{.Name}} {{end}}' "$n" 2>/dev/null \
        | tr ' ' '\n' | while read -r c; do
            [[ -z "$c" ]] && continue
            docker network disconnect -f "$n" "$c" 2>/dev/null || true
          done
      docker network rm "$n" 2>/dev/null \
        || log_warn "Could not remove network $n (in use?) — run: docker compose -f ${INSTALL_DIR}/deploy/compose.yml down"
    else
      log_info "Compose network OK: $n"
    fi
  done
}

ensure_docker_networks() {
  log_info "Networks are managed by Docker Compose (not pre-created)"
  prepare_compose_networks
  log_ok "Network prep done"
}

# ── UFW ──────────────────────────────────────────────────────────────────────
configure_firewall() {
  if [[ "$CONFIGURE_UFW" -ne 1 ]]; then
    log_info "UFW configuration skipped"
    return 0
  fi

  if ! have_cmd ufw; then
    case "$PKG_MGR" in
      apt) pkg_install ufw || true ;;
      *) log_warn "ufw not available on this distro; configure firewall manually"; return 0 ;;
    esac
  fi
  have_cmd ufw || { log_warn "ufw still missing"; return 0; }

  cat <<EOF

${C_BOLD}Planned UFW rules:${C_RESET}
  allow ${SSH_PORT}/tcp   (SSH — FIRST)
  allow 80/tcp
  allow 443/tcp
  deny 5432/tcp from any (PostgreSQL not globally open)
EOF
  if [[ -n "$ADMIN_IP_ALLOWLIST" ]]; then
    log_info "Admin IP allowlist: ${ADMIN_IP_ALLOWLIST}"
  fi

  if [[ "$MINIMAL_INSTALL" -ne 1 ]] && ! confirm "Apply UFW rules now?" "Y"; then
    log_warn "UFW not applied"
    return 0
  fi

  # Never lock out SSH
  ufw allow "${SSH_PORT}/tcp" comment 'PgPanel SSH' || true
  ufw allow 80/tcp comment 'PgPanel HTTP' || true
  ufw allow 443/tcp comment 'PgPanel HTTPS' || true

  if [[ -n "$ADMIN_IP_ALLOWLIST" ]]; then
    local ip
    IFS=',' read -ra _ips <<<"$ADMIN_IP_ALLOWLIST"
    for ip in "${_ips[@]}"; do
      ip="$(echo "$ip" | xargs)"
      [[ -z "$ip" ]] && continue
      ufw allow from "$ip" to any port 80,443 proto tcp comment 'PgPanel admin allowlist' || true
    done
  fi

  ufw --force enable || true
  log_cmd_preview 12 ufw status verbose
  log_ok "UFW configured"
}

# ── Sysctl / swap (optional) ─────────────────────────────────────────────────
configure_system_tuning() {
  if [[ -n "$TIMEZONE_SET" ]] && have_cmd timedatectl; then
    timedatectl set-timezone "$TIMEZONE_SET" 2>/dev/null || true
  fi

  if [[ "$CREATE_SWAP" -eq 1 ]]; then
    local sw_total
    sw_total="$(awk '/SwapTotal/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)"
    if [[ "${sw_total:-0}" -lt 1024 ]]; then
      local swapfile="/swapfile-pgpanel"
      if [[ ! -f "$swapfile" ]]; then
        log_info "Creating ${SWAP_SIZE_MB}MB swap at ${swapfile}"
        dd if=/dev/zero of="$swapfile" bs=1M count="$SWAP_SIZE_MB" status=none
        chmod 600 "$swapfile"
        mkswap "$swapfile"
        swapon "$swapfile"
        grep -q "$swapfile" /etc/fstab || echo "$swapfile none swap sw 0 0" >>/etc/fstab
      fi
    else
      log_info "Adequate swap already present"
    fi
  fi

  # Mild sysctl
  if [[ -d /etc/sysctl.d ]]; then
    cat >/etc/sysctl.d/99-pgpanel.conf <<'EOF'
vm.swappiness=10
fs.file-max=2097152
net.core.somaxconn=1024
EOF
    sysctl --system >/dev/null 2>&1 || true
  fi

  # Docker daemon log rotation
  if [[ ! -f /etc/docker/daemon.json ]]; then
    mkdir -p /etc/docker
    cat >/etc/docker/daemon.json <<'EOF'
{
  "log-driver": "json-file",
  "log-opts": {
    "max-size": "20m",
    "max-file": "5"
  }
}
EOF
    systemctl restart docker 2>/dev/null || true
  fi
}

# ── Build & start ────────────────────────────────────────────────────────────
ensure_panel_image() {
  # Installer NEVER builds on the VPS — only pulls pre-built images.
  resolve_panel_image

  if docker image inspect "$PGPANEL_IMAGE" >/dev/null 2>&1; then
    log_ok "Panel image already local: ${PGPANEL_IMAGE}"
    return 0
  fi

  log_info "Pulling panel image: ${PGPANEL_IMAGE}"
  if docker pull "$PGPANEL_IMAGE"; then
    log_ok "Pulled ${PGPANEL_IMAGE}"
    return 0
  fi

  local latest_img="${PGPANEL_GHCR_IMAGE}:latest"
  if [[ "$PGPANEL_IMAGE" != "$latest_img" ]]; then
    log_warn "Pull failed for ${PGPANEL_IMAGE} — trying ${latest_img}"
    if docker pull "$latest_img"; then
      docker tag "$latest_img" "$PGPANEL_IMAGE"
      log_ok "Using ${latest_img}"
      return 0
    fi
  fi

  die "Cannot pull panel image: ${PGPANEL_IMAGE}

The installer never builds on the VPS. Publish a public image first (from your Mac):

  ./deploy/push-image.sh --latest
  # GitHub → Packages → pgpanel → Change visibility → Public

Then on the VPS:

  docker pull ${PGPANEL_IMAGE}
  sudo bash /opt/pgpanel/deploy/install.sh --mode resume"
}

build_and_start() {
  INSTALL_PHASE="start"
  resolve_panel_image
  upsert_env_key "${INSTALL_DIR}/.env" "PGPANEL_IMAGE" "$PGPANEL_IMAGE"
  upsert_env_key "${INSTALL_DIR}/.env" "PGPANEL_VERSION" "$PGPANEL_VERSION"
  upsert_env_key "${INSTALL_DIR}/.env" "PGPANEL_HOST_DATA" "$DATA_DIR"
  # Compose must not resolve the image remotely just to start a locally
  # verified image. Updates still pull the requested tag in ensure_panel_image.
  upsert_env_key "${INSTALL_DIR}/.env" "PGPANEL_PULL_POLICY" "missing"

  prepare_compose_networks
  render_compose

  ensure_panel_image

  (
    cd "${INSTALL_DIR}/deploy"
    export PGPANEL_IMAGE PGPANEL_HOST_DATA="$DATA_DIR" PGPANEL_PULL_POLICY=missing
    set -a
    # shellcheck source=/dev/null
    source "${INSTALL_DIR}/.env"
    set +a
    docker compose -f compose.yml config >/dev/null

    docker compose -f compose.yml down --remove-orphans 2>/dev/null || true
    prepare_compose_networks

    log_info "Pulling Caddy…"
    docker compose -f compose.yml pull caddy \
      || die "Failed to pull Caddy image (check network)"

    # Never use -v: data volumes must survive restarts/updates
    # Both images are now known to be local.  Do not let Compose make a second
    # GHCR request, which would turn a transient registry timeout into a failed
    # install or resume.
    if ! PGPANEL_PULL_POLICY=never docker compose -f compose.yml up -d --remove-orphans --no-build; then
      log_error "compose up failed — retrying after network cleanup"
      prepare_compose_networks
      docker compose -f compose.yml down --remove-orphans 2>/dev/null || true
      prepare_compose_networks
      ensure_panel_image
      PGPANEL_PULL_POLICY=never docker compose -f compose.yml up -d --remove-orphans --no-build
    fi
  )
  log_ok "Services started (images pulled, no local build)"
}

# ── Health checks ────────────────────────────────────────────────────────────
health_check() {
  INSTALL_PHASE="health"
  local failed=0
  sleep 5

  local svc
  for svc in caddy panel; do
    if docker compose -f "${INSTALL_DIR}/deploy/compose.yml" ps --status running 2>/dev/null | grep -qi "$svc"; then
      log_ok "Container running: $svc"
    else
      if docker ps --format '{{.Names}}' | grep -qi "$svc"; then
        log_ok "Container running: $svc"
      else
        log_error "Container not running: $svc"
        failed=1
      fi
    fi
  done

  local i ok=0
  for i in $(seq 1 40); do
    if timeout_cmd 5 curl -fsS http://127.0.0.1:8080/health >/dev/null 2>&1; then
      # panel may not bind host; try via docker exec
      ok=1
      break
    fi
    if docker compose -f "${INSTALL_DIR}/deploy/compose.yml" exec -T panel curl -fsS http://127.0.0.1:8080/health >/dev/null 2>&1; then
      ok=1
      break
    fi
    sleep 2
  done

  if [[ $ok -eq 1 ]]; then
    log_ok "Panel /health OK"
  else
    # Try docker network
    if docker run --rm --network pgpanel_frontend curlimages/curl:8.5.0 -fsS http://panel:8080/health >/dev/null 2>&1; then
      log_ok "Panel /health OK (via docker network)"
    else
      log_error "Panel /health failed"
      failed=1
    fi
  fi

  if docker run --rm --network pgpanel_frontend curlimages/curl:8.5.0 -fsS http://panel:8080/ready >/dev/null 2>&1; then
    log_ok "Panel /ready OK"
  else
    log_warn "Panel /ready not OK yet (Docker may still be warming)"
  fi

  if [[ -f "${INSTALL_DIR}/.env" ]]; then
    local mode
    mode="$(stat -c '%a' "${INSTALL_DIR}/.env" 2>/dev/null || stat -f '%OLp' "${INSTALL_DIR}/.env" 2>/dev/null || echo '?')"
    if [[ "$mode" == "600" ]]; then
      log_ok ".env permissions 0600"
    else
      log_error ".env permissions are ${mode}, expected 0600"
      failed=1
    fi
  fi

  if [[ "$ENABLE_HTTPS" -eq 1 && "$USE_DOMAIN" -eq 1 ]]; then
    if timeout_cmd 20 curl -fsSI "https://${PANEL_DOMAIN}/health" >/dev/null 2>&1; then
      log_ok "HTTPS ${PANEL_DOMAIN} reachable"
    else
      log_warn "HTTPS not yet reachable (DNS/cert propagation can take minutes)"
    fi
  fi

  if [[ $failed -ne 0 ]]; then
    log_error "Health check incomplete — system NOT marked ready"
    log_info "Logs: docker compose -f ${INSTALL_DIR}/deploy/compose.yml logs --tail=100"
    log_info "Repair: sudo pgpanel repair"
    return 1
  fi
  log_ok "Health checks passed"
  return 0
}

# ── S3 storage test (best-effort, no invented Databasus API) ─────────────────
test_s3_storage() {
  log_step 14 15 "Backup storage teszt"
  case "$BACKUP_STORAGE_TYPE" in
    s3|r2|b2|hetzner|minio)
      ;;
    local|later|none|"")
      log_info "Storage type '${BACKUP_STORAGE_TYPE}' — skip automated S3 test"
      log_warn "Databasus backup storage may need manual configuration (Pending manual setup)"
      return 0
      ;;
    *)
      log_warn "Unknown storage type; skipping"
      return 0
      ;;
  esac

  if [[ -z "$S3_ENDPOINT" || -z "$S3_BUCKET" || -z "$S3_ACCESS_KEY" || -z "$S3_SECRET_KEY" ]]; then
    log_warn "Incomplete S3 credentials — skip test"
    return 0
  fi

  if ! have_cmd aws && ! docker image inspect amazon/aws-cli:2.15.0 >/dev/null 2>&1; then
    log_info "Pulling amazon/aws-cli for storage probe…"
    docker pull amazon/aws-cli:2.15.0 >/dev/null || true
  fi

  local probe_key="pgpanel-install-probe-$(openssl rand -hex 6).txt"
  local aws_args=(--endpoint-url "$S3_ENDPOINT")
  [[ "$S3_PATH_STYLE" -eq 1 ]] && aws_args+=(--endpoint-url "$S3_ENDPOINT")
  [[ -n "$S3_REGION" ]] && aws_args+=(--region "$S3_REGION")

  # Use dockerized aws-cli to avoid host deps; secrets via env (not argv)
  if ! docker run --rm \
    -e AWS_ACCESS_KEY_ID="$S3_ACCESS_KEY" \
    -e AWS_SECRET_ACCESS_KEY="$S3_SECRET_KEY" \
    -e AWS_DEFAULT_REGION="${S3_REGION:-auto}" \
    amazon/aws-cli:2.15.0 \
    s3 ls "s3://${S3_BUCKET}" --endpoint-url "$S3_ENDPOINT" >/dev/null 2>&1; then
    log_error "S3 list bucket failed"
    return 1
  fi

  echo "pgpanel-probe" | docker run --rm -i \
    -e AWS_ACCESS_KEY_ID="$S3_ACCESS_KEY" \
    -e AWS_SECRET_ACCESS_KEY="$S3_SECRET_KEY" \
    -e AWS_DEFAULT_REGION="${S3_REGION:-auto}" \
    amazon/aws-cli:2.15.0 \
    s3 cp - "s3://${S3_BUCKET}/${S3_PREFIX}${probe_key}" --endpoint-url "$S3_ENDPOINT" >/dev/null 2>&1 || {
      log_error "S3 write probe failed"
      return 1
    }

  docker run --rm \
    -e AWS_ACCESS_KEY_ID="$S3_ACCESS_KEY" \
    -e AWS_SECRET_ACCESS_KEY="$S3_SECRET_KEY" \
    -e AWS_DEFAULT_REGION="${S3_REGION:-auto}" \
    amazon/aws-cli:2.15.0 \
    s3 rm "s3://${S3_BUCKET}/${S3_PREFIX}${probe_key}" --endpoint-url "$S3_ENDPOINT" >/dev/null 2>&1 || true

  log_ok "S3 storage probe succeeded (list/write/delete)"
  return 0
}

storage_test_menu() {
  while true; do
    if test_s3_storage; then
      return 0
    fi
    if [[ "$NON_INTERACTIVE" -eq 1 ]]; then
      die "Storage test failed in non-interactive mode"
    fi
    echo ""
    echo "1) Adatok újramegadása"
    echo "2) Storage kihagyása"
    echo "3) Telepítés megszakítása"
    local c
    read_user "Választás [1-3]: " || die "Input closed"; c="${REPLY}"
    case "$c" in
      1) prompt_backup_storage ;;
      2) BACKUP_STORAGE_TYPE="later"; return 0 ;;
      3) die "Aborted by user after storage failure" ;;
    esac
  done
}

# ── Install CLI wrapper ──────────────────────────────────────────────────────
install_cli() {
  local src="${INSTALL_DIR}/deploy/pgpanel"
  if [[ ! -f "$src" ]]; then
    src="${SCRIPT_DIR}/pgpanel"
  fi
  if [[ -f "$src" ]]; then
    install -m 0755 "$src" "$PGPANEL_CLI_PATH"
  else
    # Minimal embedded fallback
    cat >"$PGPANEL_CLI_PATH" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
CONF=/etc/pgpanel/installer.conf
# shellcheck disable=SC1090
[[ -f "$CONF" ]] && . "$CONF"
INSTALL_DIR="${INSTALL_DIR:-/opt/pgpanel}"
cd "${INSTALL_DIR}/deploy"
case "${1:-}" in
  status) docker compose ps ;;
  start) docker compose up -d ;;
  stop) docker compose stop ;;
  restart) docker compose restart ;;
  logs) shift; docker compose logs -f --tail=200 "$@" ;;
  update) exec bash "${INSTALL_DIR}/deploy/install.sh" --mode update ;;
  repair) exec bash "${INSTALL_DIR}/deploy/install.sh" --mode repair ;;
  configure) exec bash "${INSTALL_DIR}/deploy/install.sh" --mode configure ;;
  backup-test) exec bash "${INSTALL_DIR}/deploy/install.sh" --mode backup-test ;;
  security-check) exec bash "${INSTALL_DIR}/deploy/install.sh" --mode security ;;
  uninstall) exec bash "${INSTALL_DIR}/deploy/install.sh" --mode uninstall ;;
  *) echo "Usage: pgpanel {status|start|stop|restart|logs|update|repair|configure|backup-test|security-check|uninstall}"; exit 1 ;;
esac
EOF
    chmod 0755 "$PGPANEL_CLI_PATH"
  fi
  log_ok "CLI installed → ${PGPANEL_CLI_PATH}"
}

# ── Summary report ───────────────────────────────────────────────────────────
write_install_summary() {
  local out="${LOG_DIR}/install-summary.txt"
  mkdir -p "$LOG_DIR"
  {
    echo "PgPanel install summary (redacted)"
    echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "Installer: ${INSTALLER_VERSION}"
    echo ""
    echo "OS: ${OS_PRETTY}"
    echo "Kernel: $(uname -r)"
    echo "Arch: ${ARCH}"
    echo "Docker: $(docker --version 2>/dev/null || echo n/a)"
    echo "Compose: $(docker compose version 2>/dev/null || echo n/a)"
    echo "Install dir: ${INSTALL_DIR}"
    echo "Data dir: ${DATA_DIR}"
    echo "Panel domain: ${PANEL_DOMAIN}"
    if [[ "$DATABASUS_PUBLIC" -eq 1 ]]; then
      echo "Databasus domain: ${DATABASUS_DOMAIN} (public)"
    else
      echo "Databasus: internal only (http://databasus:8000)"
    fi
    echo "PostgreSQL public ports default: ${PUBLIC_PG_PORTS}"
    echo "HTTPS: ${ENABLE_HTTPS}"
    echo "Backup storage: ${BACKUP_STORAGE_TYPE}"
    echo "Notify: ${NOTIFY_TYPE}"
    echo "Production ready: 0 (requires successful backup+restore verification)"
    echo ""
    echo "Containers:"
    docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Image}}' 2>/dev/null || true
    echo ""
    echo "Networks:"
    docker network ls --format '{{.Name}}' | grep pgpanel || true
    echo ""
    echo "Volumes (names only):"
    docker volume ls --format '{{.Name}}' | grep -E 'pgpanel|caddy' || true
  } >"$out"
  chmod 0640 "$out"
  log_ok "Summary → ${out}"
}

print_final_message() {
  log_step 15 15 "Telepítés befejezése"
  INSTALL_PHASE="done"
  local panel_url
  if [[ "$ENABLE_HTTPS" -eq 1 && "$USE_DOMAIN" -eq 1 ]]; then
    panel_url="https://${PANEL_DOMAIN}"
  else
    panel_url="http://$(public_ip)"
  fi

  cat <<EOF

${C_GREEN}${C_BOLD}PgPanel telepítés kész.${C_RESET}

${C_BOLD}Panel:${C_RESET}   ${panel_url}
${C_BOLD}Admin:${C_RESET}   Első admin létrehozása a webes setup oldalon (e-mail + jelszó)
${C_BOLD}Backup:${C_RESET}  native engine (${BACKUP_STORAGE_TYPE:-local})

${C_BOLD}Konfiguráció:${C_RESET}
  ${INSTALL_DIR}/.env
  ${PGPANEL_CONF}

${C_BOLD}Adatok:${C_RESET}  ${DATA_DIR}
${C_BOLD}Logok:${C_RESET}   ${LOG_DIR}

${C_BOLD}Hasznos parancsok:${C_RESET}
  pgpanel status
  pgpanel logs
  pgpanel update
  pgpanel repair
  pgpanel security-check
  pgpanel uninstall

${C_YELLOW}FIGYELEM:${C_RESET}
  • Első belépés után a setup wizard végigvezet az első clusteren.
  • Production-ready csak sikeres backup + verify után.
  • Docker socket a panel konténerben = kiemelt host kockázat (SECURITY.md).
  • PostgreSQL clusterek: pgpanel_database_management hálózat (privát DNS).
  • Backup: beépített native engine (pg_dump), nem Databasus.

EOF
  PRODUCTION_READY=0
  # update conf flag
  if [[ -f "$PGPANEL_CONF" ]]; then
    sed -i.bak 's/^PRODUCTION_READY=.*/PRODUCTION_READY=0/' "$PGPANEL_CONF" 2>/dev/null || true
    rm -f "${PGPANEL_CONF}.bak"
  fi
}

# ── Interactive prompts ──────────────────────────────────────────────────────
prompt_base_settings() {
  echo ""
  echo "${C_BOLD}=== Alapbeállítások ===${C_RESET}"
  log_info "Hivatalos forrás: ${PGPANEL_OFFICIAL_REPO} (nem kérhető / nem módosítható a telepítőben)"
  REPO_URL="$PGPANEL_OFFICIAL_REPO"
  prompt_val INSTALL_DIR "Telepítési könyvtár" "$INSTALL_DIR"
  prompt_val DATA_DIR "Adatkönyvtár" "$DATA_DIR"
  CLUSTER_DATA_DIR="${DATA_DIR}/clusters"
  prompt_val CLUSTER_DATA_DIR "Cluster volume-ok könyvtára" "$CLUSTER_DATA_DIR"
  prompt_val LOG_DIR "Logkönyvtár" "$LOG_DIR"
  BACKUP_CACHE_DIR="${DATA_DIR}/backups"
  prompt_val BACKUP_CACHE_DIR "Backup cache könyvtár" "$BACKUP_CACHE_DIR"
  echo "Frissítési csatorna: 1) stable (verziózott image)  2) edge (latest)"
  local ch
  read_user "Választás [1]: " || die "Input closed"; ch="${REPLY}"
  case "${ch:-1}" in
    2)
      UPDATE_CHANNEL="edge"
      GIT_REF="main"
      ;;
    *)
      UPDATE_CHANNEL="stable"
      GIT_REF="main"
      ;;
  esac
  resolve_panel_image
  log_info "Panel image: ${PGPANEL_IMAGE}"
}

prompt_network() {
  echo ""
  echo "${C_BOLD}=== Domain ===${C_RESET}"
  echo "${C_DIM}Csak a panel domain kell. PostgreSQL és backup nem publikus domainen fut.${C_RESET}"
  prompt_yesno USE_DOMAIN "Használsz domaint a panelhez?" "y"
  if [[ "$USE_DOMAIN" -eq 1 ]]; then
    prompt_val PANEL_DOMAIN "Panel domain" "$PANEL_DOMAIN"
    prompt_val LETSENCRYPT_EMAIL "Let's Encrypt / admin e-mail" "${LETSENCRYPT_EMAIL:-$ADMIN_EMAIL}"
    prompt_yesno ENABLE_HTTPS "HTTPS (Let's Encrypt)?" "y"
    if check_dns "$PANEL_DOMAIN"; then
      if dns_points_to_this_host "$PANEL_DOMAIN"; then
        log_ok "DNS ${PANEL_DOMAIN} → this host"
      else
        log_warn "DNS for ${PANEL_DOMAIN} may not point to this VPS yet"
      fi
    else
      log_warn "Cannot resolve ${PANEL_DOMAIN}"
    fi
  else
    ENABLE_HTTPS=0
    log_warn "HTTP-only nem ajánlott production-ben."
    if ! confirm "Tényleg HTTP-only módban folytatod?" "N"; then
      die "Aborted"
    fi
  fi
  DATABASUS_PUBLIC=0
  DATABASUS_DOMAIN=""
  PUBLIC_PG_PORTS=0
  prompt_yesno CONFIGURE_UFW "UFW tűzfal (SSH + 80/443)?" "y"
  prompt_val SSH_PORT "SSH port" "22"
}

# Fresh installs intentionally ask only for the public panel identity. All
# operational settings (storage, retention, notifications, PostgreSQL
# defaults, firewall details) are managed after login in the web UI or via
# `pgpanel configure`.
prompt_minimal_install_identity() {
  echo ""
  echo "${C_BOLD}=== PgPanel alap telepítés ===${C_RESET}"
  echo "A telepítő csak a panel domaint és a tanúsítványhoz tartozó e-mail címet kéri."
  local domain_default="$PANEL_DOMAIN"
  local email_default="$LETSENCRYPT_EMAIL"
  [[ "$domain_default" == "db.example.com" ]] && domain_default=""
  [[ "$email_default" == "admin@example.com" ]] && email_default=""
  prompt_val PANEL_DOMAIN "Panel domain" "$domain_default"
  prompt_val LETSENCRYPT_EMAIL "E-mail cím" "$email_default"

  if [[ ! "$PANEL_DOMAIN" =~ ^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$ ]]; then
    die "Invalid panel domain: ${PANEL_DOMAIN}"
  fi
  if [[ ! "$LETSENCRYPT_EMAIL" =~ ^[^[:space:]@]+@[^[:space:]@]+\.[^[:space:]@]+$ ]]; then
    die "Invalid e-mail address: ${LETSENCRYPT_EMAIL}"
  fi

  USE_DOMAIN=1
  ENABLE_HTTPS=1
  CONFIGURE_UFW=1
  PUBLIC_PG_PORTS=0
  DATABASUS_PUBLIC=0
  DATABASUS_DOMAIN=""
  BACKUP_STORAGE_TYPE="local"
  NOTIFY_TYPE="none"
  ENABLE_DATABASUS=0
  CREATE_SWAP=0
  ENABLE_WATCHTOWER=0
}

prompt_admin() {
  echo ""
  echo "${C_BOLD}=== Adminfiók ===${C_RESET}"
  prompt_val ADMIN_USERNAME "Admin felhasználónév" "$ADMIN_USERNAME"
  prompt_val ADMIN_EMAIL "Admin e-mail" "$ADMIN_EMAIL"
  echo "Jelszó: 1) automatikus generálás  2) kézi megadás"
  local p
  read_user "Választás [1]: " || die "Input closed"; p="${REPLY}"
  if [[ "${p:-1}" == "2" ]]; then
    prompt_secret ADMIN_PASSWORD "Admin jelszó (min. 16 karakter)"
    local confirm_pw
    prompt_secret confirm_pw "Jelszó ismét"
    [[ "$ADMIN_PASSWORD" == "$confirm_pw" ]] || die "Passwords do not match"
    [[ ${#ADMIN_PASSWORD} -ge 16 ]] || die "Password too short"
  else
    ADMIN_PASSWORD=""
    ADMIN_PASSWORD_GENERATED=1
  fi
  prompt_yesno PREPARE_2FA "2FA előkészítés (dokumentáció)?" "y"
  prompt_val SESSION_TTL_HOURS "Session lejárat (óra)" "24"
  prompt_val LOGIN_MAX_ATTEMPTS "Sikertelen belépési limit" "5"
}

prompt_postgres_defaults() {
  echo ""
  echo "${C_BOLD}=== PostgreSQL alapbeállítások ===${C_RESET}"
  echo "Alapértelmezett verzió: 1) 16  2) 17  3) 18"
  local v
  read_user "Választás [2]: " || die "Input closed"; v="${REPLY}"
  case "${v:-2}" in
    1) PG_DEFAULT_VERSION="16" ;;
    3) PG_DEFAULT_VERSION="18" ;;
    *) PG_DEFAULT_VERSION="17" ;;
  esac
  prompt_val PG_DEFAULT_CPU "Alap CPU limit" "2"
  prompt_val PG_DEFAULT_MEMORY_MB "Alap RAM (MB)" "2048"
  prompt_val PG_DEFAULT_STORAGE_GB "Alap storage soft limit (GB)" "20"
  prompt_val PG_TIMEZONE "Timezone" "Europe/Budapest"
  prompt_val PG_LOCALE "Locale" "en_US.UTF-8"
  prompt_val PG_MAX_CLUSTERS "Max clusterek" "20"
  prompt_val PG_CLUSTER_PREFIX "Container név prefix" "pgpanel_pg_"
  log_info "Allowed images: postgres:16, postgres:17, postgres:18 (no :latest)"
}

prompt_backup_storage() {
  echo ""
  echo "${C_BOLD}=== Backup storage ===${C_RESET}"
  cat <<'EOF'
1) S3
2) Cloudflare R2
3) Backblaze B2
4) Hetzner Object Storage
5) MinIO
6) Helyi könyvtár
7) Később konfigurálom
EOF
  local c
  read_user "Választás [7]: " || die "Input closed"; c="${REPLY}"
  case "${c:-7}" in
    1) BACKUP_STORAGE_TYPE="s3" ;;
    2) BACKUP_STORAGE_TYPE="r2" ;;
    3) BACKUP_STORAGE_TYPE="b2" ;;
    4) BACKUP_STORAGE_TYPE="hetzner" ;;
    5) BACKUP_STORAGE_TYPE="minio" ;;
    6) BACKUP_STORAGE_TYPE="local" ;;
    *) BACKUP_STORAGE_TYPE="later" ;;
  esac

  if [[ "$BACKUP_STORAGE_TYPE" =~ ^(s3|r2|b2|hetzner|minio)$ ]]; then
    prompt_val S3_ENDPOINT "S3 endpoint URL" "$S3_ENDPOINT"
    prompt_val S3_REGION "Régió" "${S3_REGION:-auto}"
    prompt_val S3_BUCKET "Bucket" "$S3_BUCKET"
    prompt_val S3_ACCESS_KEY "Access key" "$S3_ACCESS_KEY"
    prompt_secret S3_SECRET_KEY "Secret key"
    prompt_yesno S3_PATH_STYLE "Path-style addressing?" "y"
    prompt_val S3_PREFIX "Storage prefix" "pgpanel/"
    prompt_yesno S3_TLS_VERIFY "TLS verify?" "y"
    prompt_yesno S3_ENCRYPT "Backup titkosítás?" "y"
  fi

  prompt_val BACKUP_RETENTION_DAYS "Retention (nap)" "14"
  prompt_val BACKUP_MAX_COUNT "Max backupok száma" "30"
  prompt_val BACKUP_FULL_FREQ "Full backup gyakoriság" "daily"
  prompt_val BACKUP_INCR_FREQ "Incremental gyakoriság" "hourly"
  prompt_yesno BACKUP_WAL_STREAMING "WAL streaming?" "y"
  prompt_val BACKUP_RESTORE_VERIFY "Restore verification" "daily"
  prompt_yesno BACKUP_FIRST_NOW "Első automatikus backup indítása később?" "y"
}

prompt_databasus() {
  echo ""
  echo "${C_BOLD}=== Databasus ===${C_RESET}"
  prompt_yesno ENABLE_DATABASUS "Databasus engedélyezése?" "y"
  if [[ "$ENABLE_DATABASUS" -ne 1 ]]; then
    return 0
  fi
  prompt_val DATABASUS_ADMIN_EMAIL "Databasus admin e-mail" "${ADMIN_EMAIL}"
  echo "Databasus jelszó: 1) auto  2) kézi"
  local p
  read_user "Választás [1]: " || die "Input closed"; p="${REPLY}"
  if [[ "${p:-1}" == "2" ]]; then
    prompt_secret DATABASUS_ADMIN_PASSWORD "Databasus admin jelszó"
  fi
  prompt_backup_storage
}

prompt_notifications() {
  echo ""
  echo "${C_BOLD}=== Értesítések ===${C_RESET}"
  cat <<'EOF'
1) E-mail SMTP
2) Discord webhook
3) Slack webhook
4) Telegram
5) Általános webhook
6) Nincs
EOF
  local c
  read_user "Választás [6]: " || die "Input closed"; c="${REPLY}"
  case "${c:-6}" in
    1)
      NOTIFY_TYPE="smtp"
      prompt_val SMTP_HOST "SMTP host" ""
      prompt_val SMTP_PORT "SMTP port" "587"
      prompt_val SMTP_TLS "TLS mód (starttls/tls/none)" "starttls"
      prompt_val SMTP_USER "SMTP username" ""
      prompt_secret SMTP_PASSWORD "SMTP password"
      prompt_val SMTP_FROM "Feladó e-mail" "$ADMIN_EMAIL"
      prompt_val SMTP_TO "Címzett e-mail" "$ADMIN_EMAIL"
      ;;
    2) NOTIFY_TYPE="discord"; prompt_val WEBHOOK_URL "Discord webhook URL" "" ;;
    3) NOTIFY_TYPE="slack"; prompt_val WEBHOOK_URL "Slack webhook URL" "" ;;
    4) NOTIFY_TYPE="telegram"; prompt_val WEBHOOK_URL "Telegram bot API URL/token path" "" ;;
    5) NOTIFY_TYPE="webhook"; prompt_val WEBHOOK_URL "Webhook URL" ""; prompt_secret WEBHOOK_TOKEN "Token (opcionális)" 1 ;;
    *) NOTIFY_TYPE="none" ;;
  esac
}

show_summary_and_confirm() {
  cat <<EOF

${C_BOLD}Telepítési összegzés${C_RESET}

  Panel domain:        ${PANEL_DOMAIN:-"(HTTP-only / no domain)"}
  Backup:              native (${BACKUP_STORAGE_TYPE:-local})
  PostgreSQL:          private Docker network only
  Telepítési könyvtár: ${INSTALL_DIR}
  Adatkönyvtár:        ${DATA_DIR}
  HTTPS:               ${ENABLE_HTTPS}
  UFW:                 ${CONFIGURE_UFW}
  Admin:               ${ADMIN_USERNAME} / ${ADMIN_EMAIL}
  OS:                  ${OS_PRETTY}
  Channel:             ${UPDATE_CHANNEL}

EOF
  if ! confirm "Folytatod a telepítést?" "N"; then
    die "Aborted by user"
  fi
}

# ── Main install flow ────────────────────────────────────────────────────────
collect_install_answers() {
  prompt_minimal_install_identity
  log_info "Advanced settings are available after login in Settings and the setup wizard."
  save_install_answers
  mark_step_done "prompts"
}

run_install_pipeline() {
  # $1 = resume (1) or fresh (0) — when resume, skip re-prompt
  local resume="${1:-0}"
  local total=15

  run_step preflight 1 "$total" "Rendszer ellenőrzése" preflight_checks

  if [[ "$resume" -eq 0 ]] || ! is_step_done prompts; then
    INSTALL_PHASE="prompts"
    write_progress_meta "in_progress"
    collect_install_answers
  else
    log_ok "Skip (already done): prompts — using saved answers"
    load_install_answers || die "Cannot load ${PGPANEL_ANSWERS_FILE}"
  fi

  # Always re-save conf paths after answers load
  mkdir -p "$LOG_DIR"
  LOG_FILE="${LOG_DIR}/installer.log"
  touch "$LOG_FILE"
  chmod 0640 "$LOG_FILE" 2>/dev/null || true

  run_step packages 2 "$total" "Csomagok telepítése" install_base_packages
  run_step docker 3 "$total" "Docker telepítése" install_docker
  run_step directories 4 "$total" "Könyvtárak létrehozása" create_directories
  run_step repository 5 "$total" "Hivatalos repository szinkronizálása" clone_or_update_repo
  run_step secrets 6 "$total" "Titkok generálása / betöltése" generate_or_load_secrets
  run_step config 7 "$total" "Konfiguráció mentése" save_installer_conf
  run_step tuning 8 "$total" "Rendszer beállítások" configure_system_tuning
  run_step networks 9 "$total" "Docker hálózatok előkészítése" ensure_docker_networks

  if ! is_step_done caddy; then
    INSTALL_PHASE="caddy"
    write_progress_meta "in_progress"
    log_step 10 "$total" "Caddy konfiguráció"
    render_caddyfile
    mark_step_done "caddy"
  else
    log_ok "Skip (already done): [10/${total}] Caddy konfiguráció"
  fi

  if ! is_step_done compose_file; then
    INSTALL_PHASE="compose_file"
    write_progress_meta "in_progress"
    log_step 11 "$total" "Compose konfiguráció"
    render_compose
    mark_step_done "compose_file"
  else
    log_ok "Skip (already done): [11/${total}] Compose konfiguráció"
    render_compose # refresh paths/image pins
  fi

  run_step firewall 12 "$total" "Tűzfal" configure_firewall
  run_step start 13 "$total" "Image pull / szolgáltatások indítása" build_and_start
  run_step cli 13 "$total" "CLI telepítése" install_cli

  if ! is_step_done health; then
    INSTALL_PHASE="health"
    write_progress_meta "in_progress"
    log_step 14 "$total" "Health check"
    health_check || log_warn "Health check incomplete — you can re-run: --mode resume"
    mark_step_done "health"
  else
    log_ok "Skip (already done): health"
  fi

  if ! is_step_done storage; then
    INSTALL_PHASE="storage"
    write_progress_meta "in_progress"
    if [[ "$BACKUP_STORAGE_TYPE" =~ ^(s3|r2|b2|hetzner|minio)$ ]]; then
      storage_test_menu || log_warn "Storage test skipped/failed"
    else
      log_step 15 "$total" "Backup storage (később / manuális)"
      log_info "Backup storage type: ${BACKUP_STORAGE_TYPE}"
    fi
    mark_step_done "storage"
  fi

  send_test_notification || true
  write_install_summary
  write_progress_meta "done"
  mark_step_done "complete"
  INSTALL_PHASE="done"
  print_final_message
}

do_fresh_install() {
  mkdir -p "$LOG_DIR" "$PGPANEL_ETC_DIR"
  LOG_FILE="${LOG_DIR}/installer.log"
  touch "$LOG_FILE"
  chmod 0640 "$LOG_FILE"
  log_info "PgPanel installer ${INSTALLER_VERSION} starting (fresh install)"

  if has_resumable_install || [[ -f "$PGPANEL_ANSWERS_FILE" ]]; then
    log_warn "Saved answers / incomplete install found."
    if confirm "Continue previous install (keep answers)?" "Y"; then
      do_resume_install
      return 0
    fi
    if confirm "Start completely fresh (discard saved install progress)?" "N"; then
      clear_install_progress
      # keep answers unless user wants wipe
      if confirm "Also discard saved answers (${PGPANEL_ANSWERS_FILE})?" "N"; then
        rm -f "$PGPANEL_ANSWERS_FILE"
      fi
    else
      do_resume_install
      return 0
    fi
  fi

  clear_install_progress
  write_progress_meta "in_progress"
  run_install_pipeline 0
}

do_resume_install() {
  mkdir -p "$LOG_DIR" "$PGPANEL_ETC_DIR"
  LOG_FILE="${LOG_DIR}/installer.log"
  touch "$LOG_FILE"
  chmod 0640 "$LOG_FILE"
  log_info "Resuming install (installer ${INSTALLER_VERSION})"

  if ! load_install_answers; then
    log_warn "No answers file — collecting answers again"
    clear_install_progress
    write_progress_meta "in_progress"
    run_install_pipeline 0
    return 0
  fi

  load_installer_conf 2>/dev/null || true
  REPO_URL="$PGPANEL_OFFICIAL_REPO"

  if [[ -f "$PGPANEL_PROGRESS_FILE" ]]; then
    log_info "Progress file: ${PGPANEL_PROGRESS_FILE}"
    grep -E '^(STATUS|LAST_PHASE|STEP_)' "$PGPANEL_PROGRESS_FILE" 2>/dev/null | while read -r l; do
      log_info "  $l"
    done || true
  fi

  if ! confirm "Resume install with saved answers (domain=${PANEL_DOMAIN})?" "Y"; then
    die "Resume cancelled"
  fi

  write_progress_meta "in_progress"
  run_install_pipeline 1
}

send_test_notification() {
  case "$NOTIFY_TYPE" in
    webhook|discord|slack|telegram)
      [[ -z "$WEBHOOK_URL" ]] && return 0
      timeout_cmd 15 curl -fsS -X POST "$WEBHOOK_URL" \
        -H "Content-Type: application/json" \
        -d '{"content":"PgPanel installer: test notification","text":"PgPanel installer: test notification"}' \
        >/dev/null 2>&1 && log_ok "Test webhook sent" || log_warn "Test webhook failed"
      ;;
    smtp)
      log_info "SMTP test: configure application-level mailer; installer does not embed raw passwords in sendmail"
      ;;
    *) ;;
  esac
}

# ── Update mode (no data loss) ───────────────────────────────────────────────
do_update() {
  load_installer_conf
  REPO_URL="$PGPANEL_OFFICIAL_REPO"
  LOG_FILE="${LOG_DIR}/installer.log"
  mkdir -p "$LOG_DIR"
  log_info "Starting production update (preserves data, secrets, volumes)…"

  ensure_interactive_stdin

  local local_v remote_v
  local_v="$(get_local_version)"
  remote_v="$(get_remote_version "$GIT_REF")"
  show_version_status || true

  if [[ "$FORCE_UPDATE" -ne 1 && "$local_v" == "$remote_v" && "$local_v" != "unknown" ]]; then
    if ! confirm "Already on ${local_v}. Update anyway (re-pull images)?" "N"; then
      log_info "Update cancelled"
      return 0
    fi
  fi

  if ! confirm "Continue update? PostgreSQL volumes, panel SQLite, .env secrets and Databasus data are kept." "Y"; then
    die "Update aborted"
  fi

  local backup_dir="${INSTALL_DIR}/backups/config/$(date +%Y%m%d%H%M%S)"
  mkdir -p "$backup_dir"
  cp -a "${INSTALL_DIR}/.env" "$backup_dir/" 2>/dev/null || true
  cp -a "$PGPANEL_CONF" "$backup_dir/" 2>/dev/null || true
  if [[ -f "${DATA_DIR}/panel/panel.db" ]]; then
    cp -a "${DATA_DIR}/panel/panel.db" "$backup_dir/panel.db" || true
  fi
  # Also backup WAL sidecar if present
  if [[ -f "${DATA_DIR}/panel/panel.db-wal" ]]; then
    cp -a "${DATA_DIR}/panel/panel.db-wal" "$backup_dir/" 2>/dev/null || true
    cp -a "${DATA_DIR}/panel/panel.db-shm" "$backup_dir/" 2>/dev/null || true
  fi
  log_ok "Config + panel DB backup → ${backup_dir}"

  clone_or_update_repo
  if [[ ! -f "${INSTALL_DIR}/.env" ]]; then
    die ".env missing — refuse to update without secrets. Restore from ${backup_dir}"
  fi

  # NEVER regenerate secrets / never docker compose down -v
  generate_or_load_secrets
  render_caddyfile
  render_compose
  save_installer_conf

  # Record previous version for rollback notes
  printf '%s\n' "$local_v" >"${backup_dir}/previous-version.txt"
  printf '%s\n' "$(get_local_version)" >"${backup_dir}/target-version.txt"

  build_and_start
  install_cli

  if ! health_check; then
    log_error "Update health check failed — rollback available from ${backup_dir}"
    if confirm "Restore previous .env and re-pull previous image?" "Y"; then
      cp -a "${backup_dir}/.env" "${INSTALL_DIR}/.env" 2>/dev/null || true
      if [[ -f "${backup_dir}/panel.db" ]]; then
        (
          cd "${INSTALL_DIR}/deploy"
          docker compose stop panel 2>/dev/null || true
        )
        cp -a "${backup_dir}/panel.db" "${DATA_DIR}/panel/panel.db" || true
      fi
      (
        cd "${INSTALL_DIR}/deploy"
        set -a
        # shellcheck source=/dev/null
        source "${INSTALL_DIR}/.env"
        set +a
        docker compose up -d --remove-orphans
      ) || true
      log_warn "Rollback attempted. Check: pgpanel status"
    fi
    return 1
  fi

  write_install_summary
  cat <<EOF

${C_GREEN}${C_BOLD}Update completed without data loss.${C_RESET}

  Previous version: ${local_v}
  Current version:  $(get_local_version)
  Panel image:      ${PGPANEL_IMAGE}
  Backup:           ${backup_dir}

Preserved:
  • /opt/pgpanel/.env secrets
  • panel SQLite (${DATA_DIR}/panel)
  • PostgreSQL Docker volumes
  • Databasus data (${DATA_DIR}/databasus)

EOF
  log_ok "Update completed"
}

do_version_check() {
  load_installer_conf
  REPO_URL="$PGPANEL_OFFICIAL_REPO"
  show_version_status || true
  echo ""
  echo "Commands:"
  echo "  pgpanel update          # safe upgrade"
  echo "  pgpanel version         # show installed versions"
  echo "  pgpanel status          # runtime health"
}

# ── Repair mode ──────────────────────────────────────────────────────────────
do_repair() {
  load_installer_conf
  LOG_FILE="${LOG_DIR}/installer.log"
  mkdir -p "$LOG_DIR"
  log_info "Repair mode"

  create_directories
  ensure_docker_networks

  if [[ ! -f "${INSTALL_DIR}/.env" ]]; then
    die ".env missing — cannot invent encryption keys. Restore secrets manually."
  fi
  chmod 0600 "${INSTALL_DIR}/.env"
  chown root:root "${INSTALL_DIR}/.env" 2>/dev/null || true

  # Ensure networking defaults exist on older installs (idempotent upserts)
  upsert_env_key "${INSTALL_DIR}/.env" "PGPANEL_MANAGEMENT_NETWORK" "pgpanel_database_management"
  upsert_env_key "${INSTALL_DIR}/.env" "BACKUP_STORAGE_TYPE" "${BACKUP_STORAGE_TYPE:-local}"
  upsert_env_key "${INSTALL_DIR}/.env" "PGPANEL_BACKUP_DIR" "/var/lib/pgpanel/backups"
  upsert_env_key "${INSTALL_DIR}/.env" "PGPANEL_PULL_POLICY" "missing"
  # Remove orphaned Databasus service if present from older installs
  (
    cd "${INSTALL_DIR}/deploy" 2>/dev/null || exit 0
    docker compose stop databasus 2>/dev/null || true
    docker compose rm -f databasus 2>/dev/null || true
  ) || true

  # Always re-render so network topology (management net, private Databasus) is correct
  render_caddyfile
  render_compose

  if have_cmd docker; then
    (
      cd "${INSTALL_DIR}/deploy"
      docker compose up -d --remove-orphans
    ) || log_error "compose up failed"
    # Attach existing PG clusters to management network (fixes DNS for older provisions)
    local c net="pgpanel_database_management"
    if docker network inspect "$net" >/dev/null 2>&1; then
      while IFS= read -r c; do
        [[ -z "$c" ]] && continue
        if docker network connect "$net" "$c" 2>/dev/null; then
          log_ok "Attached ${c} → ${net}"
        fi
      done < <(docker ps --format '{{.Names}}' | grep -E '^pgpanel_pg_' || true)
    fi
  fi

  # Disk space
  local disk
  disk="$(disk_free_gb /)"
  if (( disk < 5 )); then
    log_error "Critically low disk: ${disk} GB"
  fi

  # Large docker logs
  docker ps -aq | while read -r id; do
    [[ -z "$id" ]] && continue
    true
  done

  install_cli
  health_check || log_warn "Repair finished with health warnings"
  log_ok "Repair complete"
}

# ── Configure mode ───────────────────────────────────────────────────────────
do_configure() {
  load_installer_conf
  log_info "Configure mode — change selected settings"
  prompt_network
  prompt_notifications
  if confirm "Update Databasus/backup settings?" "N"; then
    prompt_databasus
  fi
  save_installer_conf
  upsert_env_key "${INSTALL_DIR}/.env" "PGPANEL_PULL_POLICY" "missing"
  # Refresh env non-secret fields carefully without wiping keys
  render_caddyfile
  render_compose
  (
    cd "${INSTALL_DIR}/deploy"
    docker compose up -d
  )
  log_ok "Configuration updated"
}

# ── Security audit ───────────────────────────────────────────────────────────
do_security_check() {
  load_installer_conf
  local score=100
  local issues=()

  echo ""
  echo "${C_BOLD}PgPanel security audit${C_RESET}"
  echo ""

  check_issue() {
    local ok="$1" msg="$2" penalty="$3"
    if [[ "$ok" -eq 1 ]]; then
      log_ok "$msg"
    else
      log_warn "$msg"
      issues+=("$msg")
      score=$((score - penalty))
    fi
  }

  local env_ok=0
  if [[ -f "${INSTALL_DIR}/.env" ]]; then
    local mode
    mode="$(stat -c '%a' "${INSTALL_DIR}/.env" 2>/dev/null || stat -f '%OLp' "${INSTALL_DIR}/.env")"
    [[ "$mode" == "600" ]] && env_ok=1
  fi
  check_issue "$env_ok" ".env mode 0600" 15

  local owner_ok=0
  if [[ -f "${INSTALL_DIR}/.env" ]]; then
    local ow
    ow="$(stat -c '%U' "${INSTALL_DIR}/.env" 2>/dev/null || echo root)"
    [[ "$ow" == "root" ]] && owner_ok=1
  fi
  check_issue "$owner_ok" ".env owned by root" 5

  local https_ok=0
  [[ "${ENABLE_HTTPS:-0}" -eq 1 && "${USE_DOMAIN:-0}" -eq 1 ]] && https_ok=1
  check_issue "$https_ok" "HTTPS + domain enabled" 15

  local pub_pg=1
  [[ "${PUBLIC_PG_PORTS:-0}" -eq 0 ]] && pub_pg=1 || pub_pg=0
  check_issue "$pub_pg" "Public PostgreSQL ports disabled by default" 10

  local ufw_ok=0
  if have_cmd ufw && ufw status 2>/dev/null | grep -qi 'Status: active'; then ufw_ok=1; fi
  check_issue "$ufw_ok" "UFW active" 5

  local sock_db=1
  if docker inspect "$(docker ps -qf name=databasus | head -1)" 2>/dev/null | grep -q docker.sock; then
    sock_db=0
  fi
  check_issue "$sock_db" "Databasus has no Docker socket" 20

  local privileged=1
  if docker ps --format '{{.Names}}' | while read -r n; do
    docker inspect -f '{{.HostConfig.Privileged}}' "$n" 2>/dev/null | grep -q true && exit 1
    true
  done; then
    privileged=1
  else
    privileged=0
  fi
  check_issue "$privileged" "No privileged containers" 10

  local backup_ok=0
  [[ "${BACKUP_STORAGE_TYPE:-later}" != "later" && "${BACKUP_STORAGE_TYPE:-}" != "none" ]] && backup_ok=1
  check_issue "$backup_ok" "Backup storage configured" 10

  local disk_ok=1
  local disk
  disk="$(disk_free_gb /)"
  (( disk < 10 )) && disk_ok=0
  check_issue "$disk_ok" "Disk free >= 10 GB (${disk} GB)" 5

  # Docker socket on panel is expected but scores down as residual risk documentation
  log_warn "Panel mounts Docker socket (known residual risk, -0 score but documented)"
  issues+=("Panel Docker socket mount — treat as host-root equivalent")

  if (( score < 0 )); then score=0; fi
  echo ""
  printf '%sSecurity score: %s/100%s\n' "$C_BOLD" "$score" "$C_RESET"
  if [[ ${#issues[@]} -gt 0 ]]; then
    echo "Issues to fix:"
    local i
    for i in "${issues[@]}"; do
      echo "  - $i"
    done
  fi
  echo ""
  echo "Production-ready flag: ${PRODUCTION_READY:-0}"
  echo "Requires successful backup + restore verification before setting PRODUCTION_READY=1"
}

# ── Backup integration test (no invented Databasus endpoints) ────────────────
do_backup_test() {
  load_installer_conf
  log_info "Backup integration test"

  if [[ "$ENABLE_DATABASUS" -ne 1 ]]; then
    log_warn "Databasus disabled"
    return 0
  fi

  # Probe Databasus container
  if docker ps --format '{{.Names}}' | grep -qi databasus; then
    log_ok "Databasus container is running"
  else
    log_error "Databasus container not running"
    return 1
  fi

  if [[ "$BACKUP_STORAGE_TYPE" =~ ^(s3|r2|b2|hetzner|minio)$ ]]; then
    test_s3_storage || log_warn "S3 probe failed"
  fi

  cat <<'EOF'

Databasus public provisioning API is version-dependent.
PgPanel uses:
  • HttpDatabasusAdapter (only after verifying your Databasus version)
  • ManualDatabasusAdapter → PendingManualSetup

Manual checklist:
  1. Open Databasus UI (if public) or port-forward internal service
  2. Add PostgreSQL storage with panel-generated backup role credentials
  3. Configure retention / WAL / verification in Databasus
  4. Create a test cluster in PgPanel, write data, trigger backup
  5. Verify restore in Databasus before marking production-ready

EOF
  log_warn "Automatic end-to-end restore verification not asserted (no invented API)"
}

# ── Uninstall ────────────────────────────────────────────────────────────────
do_uninstall() {
  load_installer_conf
  cat <<'EOF'

1) Csak alkalmazáskonténerek törlése
2) Alkalmazás és konfiguráció törlése
3) Minden törlése, PostgreSQL volume-ok megtartásával
4) Teljes törlés PostgreSQL volume-okkal együtt
5) Mégse
EOF
  local c
  read_user "Választás [5]: " || die "Input closed"; c="${REPLY}"
  case "${c:-5}" in
    1)
      (
        cd "${INSTALL_DIR}/deploy" 2>/dev/null && docker compose down
      ) || true
      log_ok "Containers stopped"
      ;;
    2)
      (
        cd "${INSTALL_DIR}/deploy" 2>/dev/null && docker compose down
      ) || true
      rm -f "$PGPANEL_CONF" "$PGPANEL_CLI_PATH"
      log_ok "App + conf removed (data dirs kept)"
      ;;
    3)
      (
        cd "${INSTALL_DIR}/deploy" 2>/dev/null && docker compose down -v
      ) || true
      # remove non-cluster volumes only
      rm -rf "$INSTALL_DIR" "$PGPANEL_ETC_DIR" "$PGPANEL_CLI_PATH"
      log_ok "App removed; cluster volumes retained under ${CLUSTER_DATA_DIR:-/var/lib/pgpanel/clusters}"
      ;;
    4)
      echo "This DESTROYS PostgreSQL data volumes managed by PgPanel."
      echo "External S3 backups are NOT deleted."
      local confirm1
      read_user "Type DELETE ALL DATA to continue: " || die "Input closed"; confirm1="${REPLY}"
      [[ "$confirm1" == "DELETE ALL DATA" ]] || die "Aborted"
      if ! confirm "Second confirmation: destroy all local PostgreSQL volumes?" "N"; then
        die "Aborted"
      fi
      (
        cd "${INSTALL_DIR}/deploy" 2>/dev/null && docker compose down -v
      ) || true
      # Remove pgpanel-managed docker volumes
      docker volume ls -q | grep -E '^pgpanel' | while read -r v; do
        docker volume rm -f "$v" 2>/dev/null || true
      done
      docker network ls --format '{{.Name}}' | grep '^pgpanel' | while read -r n; do
        docker network rm "$n" 2>/dev/null || true
      done
      rm -rf "$INSTALL_DIR" "$DATA_DIR" "$LOG_DIR" "$PGPANEL_ETC_DIR" "$PGPANEL_CLI_PATH"
      log_ok "Full local uninstall complete"
      ;;
    *)
      log_info "Cancelled"
      ;;
  esac
}

# ── Menu ─────────────────────────────────────────────────────────────────────
show_banner() {
  cat <<EOF
${C_CYAN}${C_BOLD}
  ____        ____                  _
 |  _ \ __ _ |  _ \ __ _ _ __   ___| |
 | |_) / _\` || |_) / _\` | '_ \ / _ \ |
 |  __/ (_| ||  __/ (_| | | | |  __/ |
 |_|   \__, ||_|   \__,_|_| |_|\___|_|
       |___/  Installer v${INSTALLER_VERSION}
${C_RESET}
  Developer: ${PGPANEL_AUTHOR}
  Source:    ${PGPANEL_OFFICIAL_REPO}
EOF
}

main_menu() {
  show_banner
  if have_cmd whiptail && [[ "$NON_INTERACTIVE" -eq 0 && -t 0 ]]; then
    USE_WHIPTAIL=1
  fi

  if [[ -n "$CLI_MODE" ]]; then
    case "$CLI_MODE" in
      install|new) do_fresh_install ;;
      resume|continue) do_resume_install ;;
      update) do_update ;;
      repair) do_repair ;;
      configure) do_configure ;;
      security) do_security_check ;;
      backup-test) do_backup_test ;;
      version|version-check) do_version_check ;;
      uninstall) do_uninstall ;;
      *) die "Unknown mode: $CLI_MODE" ;;
    esac
    return 0
  fi

  local bad_inputs=0
  while true; do
    echo ""
    echo "${C_BOLD}PgPanel Installer${C_RESET}"
    echo "  Fejlesztő: ${PGPANEL_AUTHOR}"
    local lv rv
    lv="$(get_local_version 2>/dev/null || echo '?')"
    rv="$(get_remote_version main 2>/dev/null || echo '?')"
    echo "  Telepített verzió: ${lv}  ·  Elérhető: ${rv}"
    if has_resumable_install 2>/dev/null; then
      local lastp
      lastp="$(grep '^LAST_PHASE=' "$PGPANEL_PROGRESS_FILE" 2>/dev/null | cut -d= -f2- || echo '?')"
      echo "  ${C_YELLOW}Félbeszakadt telepítés:${C_RESET} phase=${lastp} → válaszd a 2-est"
    fi
    echo ""
    echo "1. Új telepítés"
    echo "2. Telepítés folytatása (mentett válaszok / félbemaradt)"
    echo "3. Frissítés új verzióra (adatok megmaradnak)"
    echo "4. Telepítés javítása"
    echo "5. Konfiguráció módosítása"
    echo "6. Biztonsági ellenőrzés"
    echo "7. Verzió ellenőrzése"
    echo "8. Backup-integráció tesztelése"
    echo "9. Teljes rendszer eltávolítása"
    echo "0. Kilépés"
    echo ""
    local choice
    if ! read_user "Választás [0-9]: "; then
      die "Menu input closed (EOF). Use: sudo bash /opt/pgpanel/deploy/install.sh --mode resume"
    fi
    choice="${REPLY}"
    case "${choice}" in
      1) do_fresh_install; break ;;
      2) do_resume_install; break ;;
      3) do_update; break ;;
      4) do_repair; break ;;
      5) do_configure; break ;;
      6) do_security_check ;;
      7) do_version_check ;;
      8) do_backup_test ;;
      9) do_uninstall; break ;;
      0) exit 0 ;;
      *)
        bad_inputs=$((bad_inputs + 1))
        if [[ -z "$choice" ]]; then
          log_warn "Empty choice (${bad_inputs}/5) — type a number 0-9"
        else
          log_warn "Invalid choice: '${choice}' (${bad_inputs}/5)"
        fi
        if (( bad_inputs >= 5 )); then
          die "Too many invalid/empty menu inputs. Resume with: sudo bash /opt/pgpanel/deploy/install.sh --mode resume"
        fi
        ;;
    esac
  done
}

# ── CLI args ─────────────────────────────────────────────────────────────────
parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --non-interactive) NON_INTERACTIVE=1; shift ;;
      --mode) CLI_MODE="$2"; shift 2 ;;
      --mode=*) CLI_MODE="${1#*=}"; shift ;;
      --install-dir) INSTALL_DIR="$2"; shift 2 ;;
      --force-update) FORCE_UPDATE=1; shift ;;
      --help|-h)
        cat <<EOF
PgPanel installer v${INSTALLER_VERSION}
Developer: ${PGPANEL_AUTHOR}

Usage:
  curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash

  sudo bash deploy/install.sh
  sudo bash deploy/install.sh --mode install|resume|update|repair|configure|security|version|uninstall
  sudo bash deploy/install.sh --mode resume    # continue after failure (keeps answers)
  sudo bash deploy/install.sh --mode update --force-update

Never builds on the VPS — only pulls images and configures.

Official repo: ${PGPANEL_OFFICIAL_REPO}
Panel image:   ${PGPANEL_GHCR_IMAGE}:<version>
Answers:       ${PGPANEL_ANSWERS_FILE}
Progress:      ${PGPANEL_PROGRESS_FILE}
EOF
        exit 0
        ;;
      *) die "Unknown argument: $1" ;;
    esac
  done
}

# ── Test mode helpers (sourced by bats) ──────────────────────────────────────
if [[ "${PGPANEL_INSTALLER_LIB_ONLY:-0}" == "1" ]]; then
  return 0 2>/dev/null || exit 0
fi

# ── Entry ────────────────────────────────────────────────────────────────────
main() {
  parse_args "$@"
  ensure_root "$@"
  ensure_interactive_stdin
  TMPDIR_INSTALL="$(mktemp -d /tmp/pgpanel-install.XXXXXX)"
  chmod 700 "$TMPDIR_INSTALL"

  # Default log location early
  mkdir -p "$LOG_DIR" 2>/dev/null || true
  LOG_FILE="${LOG_DIR}/installer.log"
  touch "$LOG_FILE" 2>/dev/null || LOG_FILE="${TMPDIR_INSTALL}/installer.log"
  touch "$LOG_FILE"
  chmod 0640 "$LOG_FILE" 2>/dev/null || true

  main_menu
}

main "$@"
