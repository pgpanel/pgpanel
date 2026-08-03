#!/usr/bin/env bash
# PgPanel installer for Ubuntu Server 24.04 LTS (amd64 / arm64).
# Idempotent where practical. Requires root.
#
# Designed for curl|bash and clean hosts: packaging assets are taken from the
# verified release archive, not from a local git checkout.
set -euo pipefail

readonly INSTALL_ROOT="/opt/pgpanel"
readonly CONFIG_DIR="/etc/pgpanel"
readonly STATE_DIR="/var/lib/pgpanel"
readonly RUN_DIR="/run/pgpanel"
readonly CADDY_DIR="/etc/caddy"
readonly DATABASUS_DIR="/opt/databasus"
readonly CLOUDFLARED_MANAGED_MARKER="${CONFIG_DIR}/cloudflared-managed"
readonly PGPANEL_USER="pgpanel"
readonly PGPANEL_GROUP="pgpanel"
readonly GITHUB_OWNER="${PGPANEL_GITHUB_OWNER:-pgpanel}"
readonly GITHUB_REPO="${PGPANEL_GITHUB_REPO:-pgpanel}"
readonly RELEASE_CHANNEL="${PGPANEL_CHANNEL:-stable}"
readonly CADDY_LISTEN="127.0.0.1:8080"
readonly BLUE_LISTEN="127.0.0.1:8081"
readonly GREEN_LISTEN="127.0.0.1:8082"
readonly DATABASUS_IMAGE="databasus/databasus:v3.51.0"

# Bootstrap trust anchor: 64-char lowercase/uppercase hex of the raw 32-byte
# Ed25519 public key. Leave empty until a real production key is configured for
# Pinned Ed25519 public key for official pgpanel/pgpanel releases (raw 32-byte hex).
# Installs can still override via --signing-key or PGPANEL_SIGNING_PUBLIC_KEY_HEX.
readonly PGPANEL_RELEASE_PUBLIC_KEY_HEX="abef81cdf0b0eded4f0593cff7f02d2a595a366724521608dcfbee66d45fd156"

readonly MAX_ARTIFACT_BYTES=$((512 * 1024 * 1024))
readonly MAX_SHA256SUMS_BYTES=$((1024 * 1024))
readonly MAX_SIGNATURE_BYTES=$((64 * 1024))
readonly MAX_MANIFEST_BYTES=$((1024 * 1024))
readonly MAX_API_BYTES=$((5 * 1024 * 1024))

VERSION=""
FROM_LOCAL=""
SKIP_RELEASE="0"
DRY_RUN="0"
REPAIR="0"
ALLOW_UNSUPPORTED_OS="0"
EXPOSURE=""
HOSTNAME=""
CLOUDFLARE_TOKEN="${PGPANEL_CLOUDFLARE_TUNNEL_TOKEN:-}"
# Keep the secret out of every unrelated child process spawned by the installer.
unset PGPANEL_CLOUDFLARE_TUNNEL_TOKEN
DATABASUS_CHOICE=""
DATABASUS_EXPLICIT="0"
SIGNING_KEY_PATH=""
ASSET_ROOT=""
DOWNLOAD_DIR=""
EXTRACT_DIR=""
RESOLVED_KEY_HEX=""
INSTALLED_RELEASE_PATH=""

log() {
    printf '[pgpanel-install] %s\n' "$*"
}

die() {
    printf '[pgpanel-install] ERROR: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: install.sh [OPTIONS]

Install PgPanel on Ubuntu Server 24.04 LTS.

Options:
  --version VERSION           Install a specific release tag (e.g. v0.1.0)
  --from-local PATH           Install from a local release tarball (requires
                              SHA256SUMS, SHA256SUMS.sig, manifest.json beside it)
  --skip-release              Configure systemd/Caddy only (binaries must exist)
  --repair                    Repair/reinstall the selected release and services
  --exposure MODE             local, cloudflare, or public
  --hostname HOST             Required for public exposure and automatic HTTPS
  --cloudflare-token TOKEN    Remotely managed tunnel token (environment is safer)
  --with-databasus            Install Databasus with Docker (optional)
  --without-databasus         Do not install Databasus
  --signing-key PATH          Path to pinned Ed25519 public key (32-byte hex)
  --allow-unsupported-os      Allow non-Ubuntu-24.04 (unsupported)
  --dry-run                   Print actions without making changes
  -h, --help                  Show this help

Environment:
  PGPANEL_GITHUB_OWNER            GitHub owner (default: pgpanel)
  PGPANEL_GITHUB_REPO             GitHub repository (default: pgpanel)
  PGPANEL_CHANNEL                 Release channel: stable or prerelease
  PGPANEL_SIGNING_PUBLIC_KEY_HEX  Raw 32-byte Ed25519 public key as hex
  PGPANEL_CLOUDFLARE_TUNNEL_TOKEN Remotely managed Cloudflare Tunnel token

Examples:
  sudo ./scripts/install.sh --version v0.1.0 --signing-key ./keys/signing.pub
  sudo PGPANEL_SIGNING_PUBLIC_KEY_HEX=<64-hex> ./scripts/install.sh
  sudo ./scripts/install.sh --from-local /tmp/pgpanel-linux-amd64.tar.gz \
       --signing-key /tmp/signing.pub
  sudo PGPANEL_CLOUDFLARE_TUNNEL_TOKEN='<token>' ./scripts/install.sh \
       --exposure cloudflare
  sudo ./scripts/install.sh --exposure public --hostname panel.example.com
EOF
}

cleanup() {
    if [[ -n "${DOWNLOAD_DIR}" && -d "${DOWNLOAD_DIR}" ]]; then
        rm -rf "${DOWNLOAD_DIR}"
    fi
    if [[ -n "${EXTRACT_DIR}" && -d "${EXTRACT_DIR}" ]]; then
        rm -rf "${EXTRACT_DIR}"
    fi
}

trap cleanup EXIT

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --version)
                VERSION="${2:?--version requires a value}"
                shift 2
                ;;
            --from-local)
                FROM_LOCAL="${2:?--from-local requires a path}"
                shift 2
                ;;
            --skip-release)
                SKIP_RELEASE="1"
                shift
                ;;
            --repair)
                REPAIR="1"
                shift
                ;;
            --exposure)
                EXPOSURE="${2:?--exposure requires a value}"
                shift 2
                ;;
            --hostname)
                HOSTNAME="${2:?--hostname requires a value}"
                shift 2
                ;;
            --cloudflare-token)
                CLOUDFLARE_TOKEN="${2:?--cloudflare-token requires a value}"
                log "WARNING: --cloudflare-token can be visible in process history; prefer PGPANEL_CLOUDFLARE_TUNNEL_TOKEN"
                shift 2
                ;;
            --with-databasus)
                [[ -z "${DATABASUS_CHOICE}" ]] || die "Databasus choice supplied more than once"
                DATABASUS_CHOICE="yes"
                DATABASUS_EXPLICIT="1"
                shift
                ;;
            --without-databasus)
                [[ -z "${DATABASUS_CHOICE}" ]] || die "Databasus choice supplied more than once"
                DATABASUS_CHOICE="no"
                DATABASUS_EXPLICIT="1"
                shift
                ;;
            --signing-key)
                SIGNING_KEY_PATH="${2:?--signing-key requires a path}"
                shift 2
                ;;
            --allow-unsupported-os)
                ALLOW_UNSUPPORTED_OS="1"
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

    if [[ "${SKIP_RELEASE}" == "1" && -n "${FROM_LOCAL}" ]]; then
        die "--skip-release and --from-local are mutually exclusive"
    fi
    validate_options
}

is_valid_hostname() {
    local hostname="$1"
    [[ ${#hostname} -le 253 ]] || return 1
    [[ "${hostname}" == *.* ]] || return 1
    [[ "${hostname}" != *..* ]] || return 1
    [[ "${hostname}" =~ ^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$ ]] || return 1
    local label
    local -a labels
    local IFS='.'
    read -r -a labels <<<"${hostname}"
    for label in "${labels[@]}"; do
        [[ -n "${label}" && ${#label} -le 63 ]] || return 1
        [[ "${label}" =~ ^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?$ ]] || return 1
    done
}

is_valid_cloudflare_token() {
    local token="$1"
    [[ ${#token} -ge 20 && ${#token} -le 4096 ]] || return 1
    [[ "${token}" != *[[:space:]]* ]] || return 1
    [[ "${token}" =~ ^[A-Za-z0-9._~+/=-]+$ ]]
}

validate_options() {
    if [[ -n "${EXPOSURE}" ]] && [[ ! "${EXPOSURE}" =~ ^(local|cloudflare|public)$ ]]; then
        die "--exposure must be local, cloudflare, or public"
    fi
    if [[ -n "${HOSTNAME}" ]] && ! is_valid_hostname "${HOSTNAME}"; then
        die "invalid hostname: use a DNS hostname without scheme, path, wildcard, or port"
    fi
    if [[ -n "${CLOUDFLARE_TOKEN}" ]] && ! is_valid_cloudflare_token "${CLOUDFLARE_TOKEN}"; then
        die "invalid Cloudflare Tunnel token"
    fi
    if [[ -n "${HOSTNAME}" && -n "${EXPOSURE}" && "${EXPOSURE}" != "public" ]]; then
        die "--hostname is only valid with --exposure public"
    fi
    if [[ -n "${CLOUDFLARE_TOKEN}" && -n "${EXPOSURE}" && "${EXPOSURE}" != "cloudflare" ]]; then
        die "Cloudflare token is only valid with --exposure cloudflare"
    fi
}

resolve_install_choices() {
    local choice
    if [[ -z "${EXPOSURE}" ]]; then
        if [[ -t 0 ]]; then
            printf 'Exposure mode [local/cloudflare/public] (local): '
            read -r choice
            EXPOSURE="${choice:-local}"
        else
            EXPOSURE="local"
        fi
    fi
    validate_options

    if [[ "${EXPOSURE}" == "public" && -z "${HOSTNAME}" ]]; then
        if [[ -t 0 ]]; then
            printf 'Public DNS hostname: '
            read -r HOSTNAME
        fi
        [[ -n "${HOSTNAME}" ]] || die "--exposure public requires --hostname HOST"
        is_valid_hostname "${HOSTNAME}" \
            || die "invalid hostname: use a DNS hostname without scheme, path, wildcard, or port"
    fi

    if [[ "${EXPOSURE}" == "cloudflare" && -z "${CLOUDFLARE_TOKEN}" ]]; then
        if [[ -t 0 ]]; then
            printf 'Cloudflare Tunnel token (input hidden): '
            read -r -s CLOUDFLARE_TOKEN
            printf '\n'
        fi
        [[ -n "${CLOUDFLARE_TOKEN}" ]] \
            || die "cloudflare exposure requires PGPANEL_CLOUDFLARE_TUNNEL_TOKEN or --cloudflare-token"
        is_valid_cloudflare_token "${CLOUDFLARE_TOKEN}" || die "invalid Cloudflare Tunnel token"
    fi

    if [[ -z "${DATABASUS_CHOICE}" ]]; then
        if [[ -t 0 ]]; then
            printf 'Install Databasus using Docker? [y/N]: '
            read -r choice
            case "${choice}" in
                y | Y | yes | YES)
                    DATABASUS_CHOICE="yes"
                    DATABASUS_EXPLICIT="1"
                    ;;
                n | N | no | NO | "")
                    DATABASUS_CHOICE="no"
                    DATABASUS_EXPLICIT="1"
                    ;;
                *) die "answer yes or no for Databasus installation" ;;
            esac
        else
            DATABASUS_CHOICE="no"
        fi
    fi
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
        die "this installer must be run as root"
    fi
}

detect_arch() {
    local machine
    machine="$(uname -m)"
    case "${machine}" in
        x86_64)
            echo "amd64"
            ;;
        aarch64 | arm64)
            echo "arm64"
            ;;
        *)
            die "unsupported architecture: ${machine}"
            ;;
    esac
}

verify_ubuntu() {
    # /etc/os-release defines VERSION; keep all sourced names local so it
    # cannot overwrite the requested PgPanel release tag.
    local ID=""
    local VERSION=""
    local VERSION_ID=""

    if [[ ! -f /etc/os-release ]]; then
        die "cannot detect OS: /etc/os-release missing"
    fi
    # shellcheck source=/dev/null
    source /etc/os-release
    if [[ "${ID:-}" != "ubuntu" ]]; then
        if [[ "${ALLOW_UNSUPPORTED_OS}" == "1" ]]; then
            log "WARNING: unsupported OS ${ID:-unknown}; continuing due to --allow-unsupported-os"
            return 0
        fi
        die "PgPanel installer supports Ubuntu 24.04 only (detected: ${ID:-unknown}). Re-run with --allow-unsupported-os to override."
    fi
    if [[ "${VERSION_ID:-}" != "24.04" ]]; then
        if [[ "${ALLOW_UNSUPPORTED_OS}" == "1" ]]; then
            log "WARNING: Ubuntu ${VERSION_ID:-unknown} is unsupported; continuing due to --allow-unsupported-os"
            return 0
        fi
        die "PgPanel installer requires Ubuntu 24.04 LTS (detected: ${VERSION_ID:-unknown}). Re-run with --allow-unsupported-os to override."
    fi
}

check_commands() {
    local missing=()
    local cmd
    for cmd in curl tar gzip systemctl install useradd getent openssl sha256sum; do
        if ! command -v "${cmd}" >/dev/null 2>&1; then
            missing+=("${cmd}")
        fi
    done
    if [[ ${#missing[@]} -gt 0 ]]; then
        die "missing required commands: ${missing[*]}"
    fi
}

check_post_deps_commands() {
    local missing=()
    local cmd
    for cmd in jq xxd; do
        if ! command -v "${cmd}" >/dev/null 2>&1; then
            missing+=("${cmd}")
        fi
    done
    if [[ ${#missing[@]} -gt 0 ]]; then
        die "missing required commands after dependency install: ${missing[*]}"
    fi
}

install_dependencies() {
    log "installing system dependencies"
    export DEBIAN_FRONTEND=noninteractive
    run apt-get update -qq
    run apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        gnupg \
        jq \
        xxd \
        debian-archive-keyring \
        apt-transport-https \
        postgresql-common \
        sqlite3 \
        openssl

    if ! command -v caddy >/dev/null 2>&1; then
        log "installing Caddy from official repository"
        if [[ ! -f /usr/share/keyrings/caddy-stable-archive-keyring.gpg ]]; then
            run curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
                | run gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
        fi
        if [[ ! -f /etc/apt/sources.list.d/caddy-stable.list ]]; then
            run curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
                | run tee /etc/apt/sources.list.d/caddy-stable.list >/dev/null
        fi
        run apt-get update -qq
        run apt-get install -y caddy
    else
        log "Caddy already installed"
    fi
}

ensure_user() {
    if ! getent group "${PGPANEL_GROUP}" >/dev/null 2>&1; then
        log "creating group ${PGPANEL_GROUP}"
        run groupadd --system "${PGPANEL_GROUP}"
    fi
    if ! getent passwd "${PGPANEL_USER}" >/dev/null 2>&1; then
        log "creating user ${PGPANEL_USER}"
        run useradd \
            --system \
            --gid "${PGPANEL_GROUP}" \
            --home "${STATE_DIR}" \
            --shell /usr/sbin/nologin \
            --comment "PgPanel web service" \
            "${PGPANEL_USER}"
    fi
}

ensure_directories() {
    log "creating directories"
    run install -d -m 0750 -o root -g "${PGPANEL_GROUP}" "${CONFIG_DIR}"
    run install -d -m 0750 -o "${PGPANEL_USER}" -g "${PGPANEL_GROUP}" "${STATE_DIR}"
    run install -d -m 0755 -o root -g root "${INSTALL_ROOT}"
    run install -d -m 0755 -o root -g root "${INSTALL_ROOT}/releases"
    run install -d -m 0755 -o root -g root "${INSTALL_ROOT}/slots"
    run install -d -m 0750 -o "${PGPANEL_USER}" -g "${PGPANEL_GROUP}" "${INSTALL_ROOT}/state"
    run install -d -m 0750 -o "${PGPANEL_USER}" -g "${PGPANEL_GROUP}" "${INSTALL_ROOT}/state/slots"
    run install -d -m 0750 -o "${PGPANEL_USER}" -g "${PGPANEL_GROUP}" "${INSTALL_ROOT}/state/staging"
    run install -d -m 0750 -o "${PGPANEL_USER}" -g "${PGPANEL_GROUP}" "${INSTALL_ROOT}/state/backups"
    run install -d -m 0750 -o root -g "${PGPANEL_GROUP}" "${RUN_DIR}"
}

is_valid_ed25519_hex_key() {
    local hex="$1"
    [[ "${hex}" =~ ^[0-9a-fA-F]{64}$ ]] || return 1
    local lower
    lower="$(printf '%s' "${hex}" | tr '[:upper:]' '[:lower:]')"
    case "${lower}" in
        *placeholder* | 0000000000000000000000000000000000000000000000000000000000000000)
            return 1
            ;;
    esac
    return 0
}

normalize_key_hex() {
    local raw="$1"
    local trimmed
    trimmed="$(printf '%s' "${raw}" | tr -d '[:space:]')"
    if [[ "${trimmed}" == "-----BEGIN"* ]]; then
        die "PEM public keys are not supported for bootstrap; use raw 32-byte hex (64 hex chars)"
    fi
    printf '%s' "${trimmed}" | tr '[:upper:]' '[:lower:]'
}

resolve_signing_key_hex() {
    local candidate=""

    if [[ -n "${SIGNING_KEY_PATH}" ]]; then
        [[ -f "${SIGNING_KEY_PATH}" ]] || die "signing key file not found: ${SIGNING_KEY_PATH}"
        candidate="$(normalize_key_hex "$(cat "${SIGNING_KEY_PATH}")")"
        is_valid_ed25519_hex_key "${candidate}" \
            || die "signing key file must contain exactly 64 hex characters (32-byte Ed25519 public key)"
        RESOLVED_KEY_HEX="${candidate}"
        log "using signing public key from --signing-key ${SIGNING_KEY_PATH}"
        return 0
    fi

    if [[ -n "${PGPANEL_SIGNING_PUBLIC_KEY_HEX:-}" ]]; then
        candidate="$(normalize_key_hex "${PGPANEL_SIGNING_PUBLIC_KEY_HEX}")"
        is_valid_ed25519_hex_key "${candidate}" \
            || die "PGPANEL_SIGNING_PUBLIC_KEY_HEX must be exactly 64 hex characters"
        RESOLVED_KEY_HEX="${candidate}"
        log "using signing public key from PGPANEL_SIGNING_PUBLIC_KEY_HEX"
        return 0
    fi

    if [[ -n "${PGPANEL_RELEASE_PUBLIC_KEY_HEX}" ]]; then
        candidate="$(normalize_key_hex "${PGPANEL_RELEASE_PUBLIC_KEY_HEX}")"
        is_valid_ed25519_hex_key "${candidate}" \
            || die "embedded PGPANEL_RELEASE_PUBLIC_KEY_HEX is invalid; set a real 64-char hex key before publishing"
        RESOLVED_KEY_HEX="${candidate}"
        log "using embedded release signing public key"
        return 0
    fi

    cat >&2 <<'EOF'
[pgpanel-install] ERROR: No trusted Ed25519 release signing public key configured.

Bootstrap verification is fail-closed: the installer will not trust a key from
the download, git checkout, or keys/signing.pub.example.

Configure one of:
  1. Embed a real key in PGPANEL_RELEASE_PUBLIC_KEY_HEX inside install.sh before publish
  2. Pass --signing-key /path/to/signing.pub  (file containing 64 hex chars)
  3. Export PGPANEL_SIGNING_PUBLIC_KEY_HEX=<64-hex-chars>

See keys/README.md for generating an Ed25519 key pair and exporting the raw
public key as hex for /etc/pgpanel/signing.pub.
EOF
    exit 1
}

hex_pubkey_to_pem() {
    local hex="$1"
    local pem_out="$2"
    local der
    der="$(mktemp)"
    # SubjectPublicKeyInfo prefix for Ed25519, then 32 raw public key bytes.
    {
        printf '\x30\x2a\x30\x05\x06\x03\x2b\x65\x70\x03\x21\x00'
        printf '%s' "${hex}" | xxd -r -p
    } >"${der}"
    openssl pkey -pubin -inform DER -in "${der}" -outform PEM -out "${pem_out}" >/dev/null 2>&1 \
        || {
            rm -f "${der}"
            die "failed to parse Ed25519 public key"
        }
    rm -f "${der}"
}

normalize_signature_file() {
    local sig_in="$1"
    local sig_out="$2"
    local raw
    raw="$(tr -d '[:space:]' <"${sig_in}")"
    if [[ "${raw}" =~ ^[0-9a-fA-F]{128}$ ]]; then
        printf '%s' "${raw}" | xxd -r -p >"${sig_out}"
    else
        cp "${sig_in}" "${sig_out}"
    fi
    local size
    size="$(wc -c <"${sig_out}" | tr -d ' ')"
    if [[ "${size}" -ne 64 ]]; then
        die "SHA256SUMS.sig must be 64 raw bytes (or 128 hex chars); got ${size} bytes"
    fi
}

verify_ed25519_sums_signature() {
    local sums_file="$1"
    local sig_file="$2"
    local key_hex="$3"
    local pem sig_bin
    pem="$(mktemp)"
    sig_bin="$(mktemp)"
    hex_pubkey_to_pem "${key_hex}" "${pem}"
    normalize_signature_file "${sig_file}" "${sig_bin}"
    if ! openssl pkeyutl -verify -rawin -pubin -inkey "${pem}" -sigfile "${sig_bin}" -in "${sums_file}" >/dev/null 2>&1; then
        rm -f "${pem}" "${sig_bin}"
        die "Ed25519 signature verification failed for SHA256SUMS"
    fi
    rm -f "${pem}" "${sig_bin}"
    log "Ed25519 signature over SHA256SUMS verified"
}

version_bare() {
    local tag="$1"
    printf '%s' "${tag#v}"
}

artifact_name_for_arch() {
    local arch="$1"
    case "${arch}" in
        amd64) echo "pgpanel-linux-amd64.tar.gz" ;;
        arm64) echo "pgpanel-linux-arm64.tar.gz" ;;
        *) die "unknown arch: ${arch}" ;;
    esac
}

download_https() {
    local url="$1"
    local dest="$2"
    local max_bytes="$3"

    case "${url}" in
        https://*) ;;
        *) die "refusing non-HTTPS URL: ${url}" ;;
    esac

    log "downloading ${url}"
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] curl --fail --location --silent --show-error --max-filesize ${max_bytes} -o ${dest} ${url}"
        : >"${dest}"
        return 0
    fi

    curl \
        --fail \
        --location \
        --silent \
        --show-error \
        --proto '=https' \
        --tlsv1.2 \
        --max-filesize "${max_bytes}" \
        --output "${dest}" \
        "${url}" \
        || die "download failed: ${url}"

    local size
    size="$(wc -c <"${dest}" | tr -d ' ')"
    if [[ "${size}" -le 0 ]]; then
        die "downloaded empty file from ${url}"
    fi
    if [[ "${size}" -gt "${max_bytes}" ]]; then
        die "downloaded ${url} exceeds max size ${max_bytes}"
    fi
}

resolve_release_version() {
    local api_url="https://api.github.com/repos/${GITHUB_OWNER}/${GITHUB_REPO}/releases"
    local releases_file
    DOWNLOAD_DIR="${DOWNLOAD_DIR:-$(mktemp -d)}"
    releases_file="${DOWNLOAD_DIR}/releases.json"
    download_https "${api_url}" "${releases_file}" "${MAX_API_BYTES}"

    if [[ "${DRY_RUN}" == "1" ]]; then
        VERSION="${VERSION:-v0.0.0-dry-run}"
        log "selected release ${VERSION} (dry-run)"
        return 0
    fi

    if [[ "${RELEASE_CHANNEL}" == "prerelease" ]]; then
        VERSION="$(jq -r '[.[] | select(.draft == false)][0].tag_name // empty' "${releases_file}")"
    else
        VERSION="$(jq -r '[.[] | select(.draft == false and .prerelease == false)][0].tag_name // empty' "${releases_file}")"
    fi
    if [[ -z "${VERSION}" ]]; then
        die "could not determine latest release from GitHub (${GITHUB_OWNER}/${GITHUB_REPO})"
    fi
    log "selected release ${VERSION}"
}

verify_sha256_artifact() {
    local sums_file="$1"
    local artifact_path="$2"
    local artifact_name="$3"
    local expected actual
    expected="$(awk -v name="${artifact_name}" '
        $2 == name || $2 == ("*" name) || $2 == ("./" name) { print tolower($1); found=1; exit }
        END { if (!found) exit 1 }
    ' "${sums_file}")" || die "artifact ${artifact_name} not listed in SHA256SUMS"

    [[ "${expected}" =~ ^[0-9a-f]{64}$ ]] || die "invalid SHA-256 in SHA256SUMS for ${artifact_name}"

    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] skip checksum compare for ${artifact_name}"
        return 0
    fi

    actual="$(sha256sum "${artifact_path}" | awk '{print tolower($1)}')"
    if [[ "${actual}" != "${expected}" ]]; then
        die "SHA-256 mismatch for ${artifact_name}: expected ${expected}, got ${actual}"
    fi
    log "SHA-256 verified for ${artifact_name}"
}

validate_manifest() {
    local manifest_file="$1"
    local expected_version="$2"
    local arch="$3"
    local artifact_name="$4"
    local platform="linux-${arch}"

    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] skip manifest validation"
        return 0
    fi

    jq -e 'type == "object"' "${manifest_file}" >/dev/null 2>&1 \
        || die "manifest.json is not valid JSON"

    local version released artifact
    version="$(jq -r '.version // empty' "${manifest_file}")"
    [[ -n "${version}" ]] || die "manifest.json missing version"
    if [[ "${version}" != "${expected_version}" ]]; then
        die "manifest version ${version} does not match release ${expected_version}"
    fi

    released="$(jq -r '.released_at // .published_at // empty' "${manifest_file}")"
    [[ -n "${released}" ]] || die "manifest.json missing released_at (or published_at)"

    artifact="$(jq -r --arg k "${platform}" '
        .artifacts[$k] as $a
        | if $a == null then empty
          elif ($a | type) == "string" then $a
          elif ($a | type) == "object" then ($a.file // empty)
          else empty
          end
    ' "${manifest_file}")"
    [[ -n "${artifact}" ]] || die "manifest.json missing artifacts.${platform}"
    if [[ "${artifact}" != "${artifact_name}" ]]; then
        die "manifest artifact for ${platform} is ${artifact}, expected ${artifact_name}"
    fi

    log "manifest.json validated for ${version} / ${platform}"
}

validate_archive_entries() {
    local tarball="$1"
    local entry path type_char

    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] skip archive entry validation"
        return 0
    fi

    while IFS= read -r entry; do
        [[ -z "${entry}" ]] && continue
        path="${entry%/}"
        case "${path}" in
            /*)
                die "archive contains absolute path: ${entry}"
                ;;
        esac
        if [[ "${path}" == ".." || "${path}" == ../* || "${path}" == */.. || "${path}" == */../* ]]; then
            die "archive contains path traversal: ${entry}"
        fi
    done < <(tar -tzf "${tarball}")

    while IFS= read -r line; do
        [[ -z "${line}" ]] && continue
        type_char="${line:0:1}"
        case "${type_char}" in
            l | h)
                die "archive contains symlink/hardlink entries (forbidden)"
                ;;
        esac
        if [[ "${line}" == *" link to "* ]]; then
            die "archive contains hardlink entries (forbidden)"
        fi
    done < <(tar -tvzf "${tarball}")

    log "archive entry paths validated"
}

find_release_root() {
    local staging="$1"
    local bare_version="$2"
    local expected="${staging}/pgpanel-${bare_version}"
    local -a candidates=()
    local d

    # Official release archives use a flat root so the updater can extract
    # directly into /opt/pgpanel/releases/<version>.
    if [[ -x "${staging}/bin/pgpanel-web" || -f "${staging}/bin/pgpanel-web" ]]; then
        if [[ -f "${staging}/VERSION" ]]; then
            local archive_version
            archive_version="$(tr -d '[:space:]' < "${staging}/VERSION")"
            [[ "${archive_version}" == "${bare_version}" ]] \
                || die "archive VERSION ${archive_version} does not match ${bare_version}"
        fi
        printf '%s\n' "${staging}"
        return 0
    fi

    # Also accept a single versioned root for locally produced compatible
    # bundles.
    if [[ -d "${expected}" ]]; then
        printf '%s\n' "${expected}"
        return 0
    fi

    for d in "${staging}"/pgpanel-*; do
        if [[ -d "${d}" ]]; then
            candidates+=("${d}")
        fi
    done

    if [[ ${#candidates[@]} -eq 1 ]]; then
        printf '%s\n' "${candidates[0]}"
        return 0
    fi

    die "expected a single pgpanel-<version>/ root in the archive (found ${#candidates[@]})"
}

require_release_assets() {
    local root="$1"
    local required=(
        "bin/pgpanel-web"
        "bin/pgpanel-helper"
        "bin/pgpanel-updater"
        "config/pgpanel.toml.example"
        "packaging/caddy/Caddyfile"
        "packaging/caddy/pgpanel-upstream.caddy"
        "packaging/systemd/pgpanel-blue.service"
        "packaging/systemd/pgpanel-green.service"
        "packaging/systemd/pgpanel-helper.service"
        "packaging/systemd/pgpanel-updater.service"
        "packaging/systemd/pgpanel-update-check.service"
        "packaging/systemd/pgpanel-update-check.timer"
    )
    local rel
    for rel in "${required[@]}"; do
        if [[ ! -e "${root}/${rel}" ]]; then
            die "release archive missing required path: ${rel}"
        fi
    done
}

install_release_tree() {
    local release_root="$1"
    local bare_version="$2"
    local release_path="${INSTALL_ROOT}/releases/${bare_version}"
    local staging_path="${INSTALL_ROOT}/releases/.${bare_version}.install.$$"
    local old_path="${INSTALL_ROOT}/releases/.${bare_version}.replaced.$$"

    INSTALLED_RELEASE_PATH="${release_path}"
    log "installing verified release to ${release_path}"
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] atomically replace ${release_path} from ${release_root}"
        return 0
    fi

    rm -rf "${staging_path}" "${old_path}"
    install -d -m 0755 -o root -g root "${staging_path}"
    cp -a "${release_root}/." "${staging_path}/"
    chown -R root:root "${staging_path}"
    find "${staging_path}/bin" -type f -exec chmod 0755 {} +
    require_release_assets "${staging_path}"

    if [[ -e "${release_path}" || -L "${release_path}" ]]; then
        mv "${release_path}" "${old_path}"
    fi
    if ! mv "${staging_path}" "${release_path}"; then
        if [[ -e "${old_path}" ]]; then
            mv "${old_path}" "${release_path}"
        fi
        die "failed to activate repaired release ${bare_version}"
    fi
    rm -rf "${old_path}"
}

setup_slot_symlinks() {
    local release_path="$1"

    if [[ ! -d "${release_path}/bin" && "${DRY_RUN}" != "1" ]]; then
        die "verified release layout has no bin/: ${release_path}"
    fi

    log "configuring blue/green slot layout"
    # Migrate the early preview layout where the slot itself was a symlink.
    if [[ -L "${INSTALL_ROOT}/slots/blue" ]]; then
        run rm -f "${INSTALL_ROOT}/slots/blue"
    fi
    if [[ -L "${INSTALL_ROOT}/slots/green" ]]; then
        run rm -f "${INSTALL_ROOT}/slots/green"
    fi
    run install -d -m 0755 -o root -g root \
        "${INSTALL_ROOT}/slots" \
        "${INSTALL_ROOT}/slots/blue" \
        "${INSTALL_ROOT}/slots/green"

    if [[ -e "${INSTALL_ROOT}/slots/blue/current" && ! -L "${INSTALL_ROOT}/slots/blue/current" ]]; then
        log "replacing invalid blue/current path"
        run rm -rf "${INSTALL_ROOT}/slots/blue/current"
    fi
    if [[ -e "${INSTALL_ROOT}/current" && ! -L "${INSTALL_ROOT}/current" ]]; then
        log "replacing invalid global current path"
        run rm -rf "${INSTALL_ROOT}/current"
    fi

    if [[ -L "${INSTALL_ROOT}/current" ]]; then
        local current_target
        current_target="$(readlink "${INSTALL_ROOT}/current" || true)"
        if [[ -n "${current_target}" && "${current_target}" != "${release_path}" ]]; then
            run ln -sfn "${current_target}" "${INSTALL_ROOT}/previous"
        fi
    fi

    # Systemd units execute through slot-specific current links.
    run ln -sfn "${release_path}" "${INSTALL_ROOT}/slots/blue/current"
    # The global link is used by the long-lived helper/updater and CLI tools.
    run ln -sfn "${release_path}" "${INSTALL_ROOT}/current"
}

prepare_bundle_dir() {
    local arch="$1"
    local artifact
    artifact="$(artifact_name_for_arch "${arch}")"
    local bundle_dir sums_file sig_file manifest_file artifact_path bare

    DOWNLOAD_DIR="$(mktemp -d)"

    if [[ -n "${FROM_LOCAL}" ]]; then
        [[ -f "${FROM_LOCAL}" ]] || die "local tarball not found: ${FROM_LOCAL}"
        local local_dir
        local_dir="$(cd "$(dirname "${FROM_LOCAL}")" && pwd)"
        artifact_path="${FROM_LOCAL}"
        sums_file="${local_dir}/SHA256SUMS"
        sig_file="${local_dir}/SHA256SUMS.sig"
        manifest_file="${local_dir}/manifest.json"
        [[ -f "${sums_file}" ]] || die "missing ${sums_file} beside local tarball"
        [[ -f "${sig_file}" ]] || die "missing ${sig_file} beside local tarball"
        [[ -f "${manifest_file}" ]] || die "missing ${manifest_file} beside local tarball"

        if [[ -z "${VERSION}" ]]; then
            VERSION="$(jq -r '.version // empty' "${manifest_file}" 2>/dev/null || true)"
            if [[ -n "${VERSION}" && "${VERSION}" != v* ]]; then
                VERSION="v${VERSION}"
            fi
        fi
        [[ -n "${VERSION}" ]] || die "--from-local requires --version or a manifest.json with version"
        bare="$(version_bare "${VERSION}")"
        cp "${sums_file}" "${DOWNLOAD_DIR}/SHA256SUMS"
        cp "${sig_file}" "${DOWNLOAD_DIR}/SHA256SUMS.sig"
        cp "${manifest_file}" "${DOWNLOAD_DIR}/manifest.json"
        # Keep artifact path as-is (may be large); verify in place.
        sums_file="${DOWNLOAD_DIR}/SHA256SUMS"
        sig_file="${DOWNLOAD_DIR}/SHA256SUMS.sig"
        manifest_file="${DOWNLOAD_DIR}/manifest.json"
    else
        if [[ -z "${VERSION}" ]]; then
            resolve_release_version
        fi
        bare="$(version_bare "${VERSION}")"
        local base="https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}/releases/download/${VERSION}"
        artifact_path="${DOWNLOAD_DIR}/${artifact}"
        sums_file="${DOWNLOAD_DIR}/SHA256SUMS"
        sig_file="${DOWNLOAD_DIR}/SHA256SUMS.sig"
        manifest_file="${DOWNLOAD_DIR}/manifest.json"
        download_https "${base}/${artifact}" "${artifact_path}" "${MAX_ARTIFACT_BYTES}"
        download_https "${base}/SHA256SUMS" "${sums_file}" "${MAX_SHA256SUMS_BYTES}"
        download_https "${base}/SHA256SUMS.sig" "${sig_file}" "${MAX_SIGNATURE_BYTES}"
        download_https "${base}/manifest.json" "${manifest_file}" "${MAX_MANIFEST_BYTES}"
    fi

    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] skip signature, checksum, manifest, and archive validation"
    else
        verify_ed25519_sums_signature "${sums_file}" "${sig_file}" "${RESOLVED_KEY_HEX}"
        verify_sha256_artifact "${sums_file}" "${artifact_path}" "${artifact}"
        validate_manifest "${manifest_file}" "${bare}" "${arch}" "${artifact}"
        validate_archive_entries "${artifact_path}"
    fi

    EXTRACT_DIR="$(mktemp -d)"
    log "extracting release archive"
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] tar -xzf ${artifact_path} -C ${EXTRACT_DIR}"
        ASSET_ROOT="${EXTRACT_DIR}/pgpanel-${bare}"
        run mkdir -p "${ASSET_ROOT}"
    else
        tar -xzf "${artifact_path}" -C "${EXTRACT_DIR}"
        ASSET_ROOT="$(find_release_root "${EXTRACT_DIR}" "${bare}")"
        require_release_assets "${ASSET_ROOT}"
    fi

    install_release_tree "${ASSET_ROOT}" "${bare}"
    local release_path="${INSTALLED_RELEASE_PATH}"
    [[ -n "${release_path}" ]] || die "internal error: installed release path unset"
    # Prefer installed tree for subsequent packaging copies.
    if [[ "${DRY_RUN}" != "1" && -d "${release_path}" ]]; then
        ASSET_ROOT="${release_path}"
    fi
    setup_slot_symlinks "${release_path}"
}

install_release() {
    local arch="$1"

    if [[ "${SKIP_RELEASE}" == "1" ]]; then
        if [[ ! -e "${INSTALL_ROOT}/current" ]]; then
            die "--skip-release requires ${INSTALL_ROOT}/current to exist"
        fi
        ASSET_ROOT="$(readlink -f "${INSTALL_ROOT}/current")"
        [[ -d "${ASSET_ROOT}" ]] || die "cannot resolve ${INSTALL_ROOT}/current"
        require_release_assets "${ASSET_ROOT}"
        log "using existing release at ${ASSET_ROOT}"
        setup_slot_symlinks "${ASSET_ROOT}"
        return 0
    fi

    if [[ "${REPAIR}" == "1" && -z "${VERSION}" && -z "${FROM_LOCAL}" \
        && -f "${INSTALL_ROOT}/current/VERSION" ]]; then
        VERSION="$(tr -d '[:space:]' <"${INSTALL_ROOT}/current/VERSION")"
        [[ "${VERSION}" == v* ]] || VERSION="v${VERSION}"
        log "repair selected currently installed release ${VERSION}"
    fi

    resolve_signing_key_hex
    prepare_bundle_dir "${arch}"
}

generate_secret_key() {
    openssl rand -base64 48 | tr -d '/+=' | head -c 48
}

install_config() {
    local config_file="${CONFIG_DIR}/pgpanel.toml"
    local example="${ASSET_ROOT}/config/pgpanel.toml.example"
    local secret_key

    if [[ -f "${config_file}" ]]; then
        log "configuration already exists: ${config_file} (preserving)"
        return 0
    fi

    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] generate ${config_file} from ${example}"
        return 0
    fi

    if [[ ! -f "${example}" ]]; then
        die "missing config template: ${example}"
    fi

    secret_key="$(generate_secret_key)"
    log "generating initial configuration at ${config_file}"
    run install -m 0640 -o root -g "${PGPANEL_GROUP}" "${example}" "${config_file}"
    sed -i "s|CHANGE_ME_GENERATE_A_SECURE_SECRET_KEY_AT_LEAST_32_CHARS|${secret_key}|g" "${config_file}"
}

set_toml_value() {
    local config_file="$1"
    local section="$2"
    local key="$3"
    local value="$4"
    local tmp
    tmp="$(mktemp)"

    awk -v section="${section}" -v key="${key}" -v value="${value}" '
        BEGIN {
            in_target = 0
            saw_section = 0
            inserted = 0
        }
        /^[[:space:]]*\[[^]]+\][[:space:]]*(#.*)?$/ {
            if (in_target && !inserted) {
                print key " = " value
                inserted = 1
            }
            normalized = $0
            sub(/[[:space:]]*#.*$/, "", normalized)
            gsub(/^[[:space:]]+|[[:space:]]+$/, "", normalized)
            in_target = (normalized == "[" section "]")
            if (in_target) {
                saw_section = 1
            }
            print
            next
        }
        {
            if (in_target && $0 ~ "^[[:space:]]*" key "[[:space:]]*=") {
                if (!inserted) {
                    print key " = " value
                    inserted = 1
                }
                next
            }
            print
        }
        END {
            if (in_target && !inserted) {
                print key " = " value
                inserted = 1
            }
            if (!saw_section) {
                print ""
                print "[" section "]"
                print key " = " value
            }
        }
    ' "${config_file}" >"${tmp}"
    install -m 0640 -o root -g "${PGPANEL_GROUP}" "${tmp}" "${config_file}"
    rm -f "${tmp}"
}

remove_toml_key() {
    local config_file="$1"
    local section="$2"
    local key="$3"
    local tmp
    tmp="$(mktemp)"

    awk -v section="${section}" -v key="${key}" '
        /^[[:space:]]*\[[^]]+\][[:space:]]*(#.*)?$/ {
            normalized = $0
            sub(/[[:space:]]*#.*$/, "", normalized)
            gsub(/^[[:space:]]+|[[:space:]]+$/, "", normalized)
            in_target = (normalized == "[" section "]")
            print
            next
        }
        {
            if (in_target && $0 ~ "^[[:space:]]*" key "[[:space:]]*=") {
                next
            }
            print
        }
    ' "${config_file}" >"${tmp}"
    install -m 0640 -o root -g "${PGPANEL_GROUP}" "${tmp}" "${config_file}"
    rm -f "${tmp}"
}

configure_selected_settings() {
    local config_file="${CONFIG_DIR}/pgpanel.toml"
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] update explicitly selected exposure/Databasus settings in ${config_file}"
        return 0
    fi

    case "${EXPOSURE}" in
        local)
            set_toml_value "${config_file}" "session" "secure" "false"
            set_toml_value "${config_file}" "trusted_proxies" "enabled" "true"
            set_toml_value "${config_file}" "trusted_proxies" "proxies" '["127.0.0.1", "::1"]'
            set_toml_value "${config_file}" "trusted_proxies" "prefer_cf_connecting_ip" "false"
            remove_toml_key "${config_file}" "server" "public_base_url"
            ;;
        cloudflare)
            set_toml_value "${config_file}" "session" "secure" "true"
            set_toml_value "${config_file}" "trusted_proxies" "enabled" "true"
            set_toml_value "${config_file}" "trusted_proxies" "proxies" '["127.0.0.1", "::1"]'
            set_toml_value "${config_file}" "trusted_proxies" "prefer_cf_connecting_ip" "false"
            remove_toml_key "${config_file}" "server" "public_base_url"
            ;;
        public)
            set_toml_value "${config_file}" "server" "public_base_url" "\"https://${HOSTNAME}\""
            set_toml_value "${config_file}" "session" "secure" "true"
            set_toml_value "${config_file}" "trusted_proxies" "enabled" "true"
            set_toml_value "${config_file}" "trusted_proxies" "proxies" '["127.0.0.1", "::1"]'
            set_toml_value "${config_file}" "trusted_proxies" "prefer_cf_connecting_ip" "false"
            ;;
    esac

    if [[ "${DATABASUS_CHOICE}" == "yes" ]]; then
        set_toml_value "${config_file}" "databasus" "enabled" "true"
        set_toml_value "${config_file}" "databasus" "base_url" '"http://127.0.0.1:4005"'
        set_toml_value "${config_file}" "databasus" "tls_verify" "true"
    elif [[ "${DATABASUS_EXPLICIT}" == "1" ]]; then
        set_toml_value "${config_file}" "databasus" "enabled" "false"
    fi
}

install_signing_key() {
    local dest="${CONFIG_DIR}/signing.pub"
    if [[ -f "${dest}" ]]; then
        log "signing public key already exists: ${dest} (preserving)"
        return 0
    fi
    if [[ -z "${RESOLVED_KEY_HEX}" ]]; then
        if [[ "${SKIP_RELEASE}" == "1" && -f "${ASSET_ROOT}/keys/signing.pub" ]]; then
            local shipped
            shipped="$(normalize_key_hex "$(cat "${ASSET_ROOT}/keys/signing.pub")")"
            is_valid_ed25519_hex_key "${shipped}" \
                || die "existing release keys/signing.pub is not a valid 32-byte hex key"
            RESOLVED_KEY_HEX="${shipped}"
        else
            die "internal error: signing key not resolved"
        fi
    fi
    log "installing pinned signing public key to ${dest}"
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] write ${dest}"
        return 0
    fi
    printf '%s\n' "${RESOLVED_KEY_HEX}" >"${dest}"
    chown root:root "${dest}"
    chmod 0644 "${dest}"
}

install_caddy() {
    log "installing Caddy configuration"
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] install Caddyfile assets from ${ASSET_ROOT}/packaging/caddy"
        return 0
    fi
    run install -d -m 0755 -o root -g root "${CADDY_DIR}"
    run install -m 0644 -o root -g root \
        "${ASSET_ROOT}/packaging/caddy/Caddyfile" \
        "${CADDY_DIR}/Caddyfile"
    run install -m 0644 -o root -g root \
        "${ASSET_ROOT}/packaging/caddy/pgpanel-upstream.caddy" \
        "${CADDY_DIR}/pgpanel-upstream.caddy"

    if [[ "${EXPOSURE}" == "public" ]]; then
        cat >"${CADDY_DIR}/Caddyfile" <<EOF
# Managed by the PgPanel installer. Caddy obtains and renews HTTPS certificates.
# Active upstream remains managed by pgpanel-updater.
{
	admin off
}

http://127.0.0.1:8080 {
	import pgpanel-upstream.caddy
}

${HOSTNAME} {
	import pgpanel-upstream.caddy

	header {
		-Server
		X-Content-Type-Options nosniff
		X-Frame-Options DENY
		Referrer-Policy strict-origin-when-cross-origin
	}

	log {
		output file /var/log/caddy/pgpanel-access.log {
			roll_size 10MiB
			roll_keep 5
		}
		format json
	}
}
EOF
    fi

    caddy validate --config "${CADDY_DIR}/Caddyfile" --adapter caddyfile >/dev/null \
        || die "generated Caddy configuration is invalid"
}

configure_public_firewall() {
    [[ "${EXPOSURE}" == "public" ]] || return 0
    if command -v ufw >/dev/null 2>&1 && ufw status | awk 'NR == 1 && $2 == "active" { found=1 } END { exit !found }'; then
        log "UFW is active; allowing HTTPS and certificate-validation traffic"
        run ufw allow 80/tcp
        run ufw allow 443/tcp
    else
        log "UFW is not active; no firewall rules changed"
    fi
}

cloudflared_management_decision() {
    local exposure="$1"
    local managed="$2"
    local service_present="$3"

    if [[ "${exposure}" == "cloudflare" ]]; then
        if [[ "${managed}" == "1" ]]; then
            printf '%s\n' "reinstall"
        elif [[ "${service_present}" == "1" ]]; then
            printf '%s\n' "conflict"
        else
            printf '%s\n' "install"
        fi
    elif [[ "${managed}" == "1" ]]; then
        printf '%s\n' "remove"
    else
        printf '%s\n' "preserve"
    fi
}

cloudflared_service_present() {
    systemctl cat cloudflared.service >/dev/null 2>&1
}

print_cloudflared_diagnostics() {
    [[ "${DRY_RUN}" == "1" ]] && return 0
    printf '\n--- cloudflared service state ---\n' >&2
    systemctl show cloudflared.service \
        --property=LoadState,ActiveState,SubState,Result \
        --no-pager >&2 || true
    printf '%s\n' \
        "cloudflared journal omitted to guarantee tunnel tokens cannot enter installer output" >&2
}

validate_cloudflared_ownership() {
    [[ "${EXPOSURE}" == "cloudflare" ]] || return 0
    if [[ ! -f "${CLOUDFLARED_MANAGED_MARKER}" ]] && cloudflared_service_present; then
        die "cloudflare mode found a pre-existing cloudflared service not managed by PgPanel; back up and remove that service manually, or use local/public mode. PgPanel will not take ownership automatically."
    fi
}

configure_cloudflared_lifecycle() {
    local managed="0"
    local service_present="0"
    local action
    [[ -f "${CLOUDFLARED_MANAGED_MARKER}" ]] && managed="1"
    cloudflared_service_present && service_present="1"
    action="$(cloudflared_management_decision "${EXPOSURE}" "${managed}" "${service_present}")"

    case "${action}" in
        preserve)
            if [[ "${service_present}" == "1" ]]; then
                log "preserving pre-existing cloudflared service (not managed by PgPanel)"
            fi
            return 0
            ;;
        conflict)
            die "cloudflare mode found a pre-existing cloudflared service not managed by PgPanel; back up and remove that service manually, or use local/public mode. PgPanel will not take ownership automatically."
            ;;
        remove)
            log "removing PgPanel-managed cloudflared service for ${EXPOSURE} exposure"
            if [[ "${DRY_RUN}" == "1" ]]; then
                log "[dry-run] uninstall PgPanel-managed cloudflared service and remove marker"
                return 0
            fi
            if [[ "${service_present}" == "1" ]]; then
                if command -v cloudflared >/dev/null 2>&1; then
                    if ! cloudflared service uninstall >/dev/null 2>&1; then
                        print_cloudflared_diagnostics
                        die "failed to uninstall PgPanel-managed cloudflared service"
                    fi
                else
                    systemctl disable --now cloudflared.service >/dev/null 2>&1 || true
                    rm -f /etc/systemd/system/cloudflared.service
                    systemctl daemon-reload
                fi
            fi
            if cloudflared_service_present; then
                print_cloudflared_diagnostics
                die "cloudflared service remains installed after PgPanel-managed removal"
            fi
            rm -f "${CLOUDFLARED_MANAGED_MARKER}"
            return 0
            ;;
        install | reinstall)
            ;;
        *)
            die "internal error: unknown cloudflared management action ${action}"
            ;;
    esac

    if ! command -v cloudflared >/dev/null 2>&1; then
        log "installing cloudflared from Cloudflare's Ubuntu 24.04 repository"
        run install -d -m 0755 -o root -g root /usr/share/keyrings
        if [[ "${DRY_RUN}" == "1" ]]; then
            log "[dry-run] install Cloudflare repository key and cloudflared package"
        else
            curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
                --output /usr/share/keyrings/cloudflare-main.gpg \
                https://pkg.cloudflare.com/cloudflare-main.gpg
            chmod 0644 /usr/share/keyrings/cloudflare-main.gpg
            printf '%s\n' \
                'deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared any main' \
                >/etc/apt/sources.list.d/cloudflared.list
        fi
        run apt-get update -qq
        run apt-get install -y cloudflared
    fi

    log "configuring remotely managed Cloudflare Tunnel service"
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] ${action} cloudflared system service using supplied token (redacted)"
        return 0
    fi

    if [[ "${action}" == "reinstall" && "${service_present}" == "1" ]]; then
        # Keep the working service up until the replacement binary and token
        # have passed all local validation.
        if ! cloudflared service uninstall >/dev/null 2>&1; then
            print_cloudflared_diagnostics
            die "failed to uninstall the existing PgPanel-managed cloudflared service"
        fi
    fi
    if ! cloudflared service install "${CLOUDFLARE_TOKEN}" >/dev/null 2>&1; then
        CLOUDFLARE_TOKEN=""
        print_cloudflared_diagnostics
        die "cloudflared service installation failed (token was not logged or retained by PgPanel)"
    fi
    CLOUDFLARE_TOKEN=""
    if ! systemctl enable cloudflared.service >/dev/null 2>&1 \
        || ! systemctl restart cloudflared.service >/dev/null 2>&1 \
        || ! systemctl is-active --quiet cloudflared.service; then
        print_cloudflared_diagnostics
        die "cloudflared service is not active after installation"
    fi
    install -m 0644 -o root -g root /dev/null "${CLOUDFLARED_MANAGED_MARKER}"
}

install_docker_engine() {
    local engine_ready="0"
    local compose_ready="0"

    if command -v docker >/dev/null 2>&1; then
        if [[ "${DRY_RUN}" != "1" ]]; then
            systemctl enable --now docker.service >/dev/null 2>&1 || true
        fi
        if docker info >/dev/null 2>&1; then
            engine_ready="1"
        fi
        if docker compose version >/dev/null 2>&1; then
            compose_ready="1"
        fi
    fi

    if [[ "${engine_ready}" == "1" && "${compose_ready}" == "1" ]]; then
        log "Docker Engine and Compose plugin already installed"
        return 0
    fi

    log "installing or repairing Docker from Docker's official apt repository"
    run apt-get install -y --no-install-recommends ca-certificates curl
    run install -m 0755 -d /etc/apt/keyrings
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] configure Docker's official Ubuntu apt repository"
    else
        curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
            --output /etc/apt/keyrings/docker.asc \
            https://download.docker.com/linux/ubuntu/gpg
        chmod a+r /etc/apt/keyrings/docker.asc
        printf '%s\n' \
            "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu noble stable" \
            >/etc/apt/sources.list.d/docker.list
    fi
    run apt-get update -qq
    if [[ "${engine_ready}" != "1" ]]; then
        run apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
    elif [[ "${compose_ready}" != "1" ]]; then
        run apt-get install -y docker-compose-plugin
    fi
    run systemctl enable --now docker.service
    if [[ "${DRY_RUN}" == "1" ]]; then
        return 0
    fi
    docker info >/dev/null 2>&1 \
        || die "Docker Engine is installed but the root daemon is not usable; check systemctl status docker"
    docker compose version >/dev/null 2>&1 \
        || die "Docker Compose plugin is installed but not usable"
}

install_databasus() {
    [[ "${DATABASUS_CHOICE}" == "yes" ]] || return 0
    log "installing optional Docker-based Databasus ${DATABASUS_IMAGE}"
    install_docker_engine
    run install -d -m 0750 -o root -g root "${DATABASUS_DIR}"
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] write ${DATABASUS_DIR}/docker-compose.yml and start Databasus"
        return 0
    fi

    cat >"${DATABASUS_DIR}/docker-compose.yml" <<EOF
services:
  databasus:
    container_name: databasus
    image: ${DATABASUS_IMAGE}
    ports:
      - "127.0.0.1:4005:4005"
    volumes:
      - databasus-data:/databasus-data
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "databasus", "healthcheck"]
      interval: 30s
      timeout: 5s
      retries: 5
      start_period: 60s

volumes:
  databasus-data:
    name: databasus-data
EOF
    chmod 0640 "${DATABASUS_DIR}/docker-compose.yml"
    docker compose -f "${DATABASUS_DIR}/docker-compose.yml" pull
    docker compose -f "${DATABASUS_DIR}/docker-compose.yml" up -d

    local i health
    log "waiting for Databasus health"
    for ((i = 1; i <= 90; i++)); do
        health="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}missing{{end}}' databasus 2>/dev/null || true)"
        if [[ "${health}" == "healthy" ]] \
            && curl -fsS http://127.0.0.1:4005/api/v1/system/health >/dev/null 2>&1; then
            log "Databasus health check passed"
            return 0
        fi
        sleep 2
    done
    docker compose -f "${DATABASUS_DIR}/docker-compose.yml" ps >&2 || true
    docker logs --tail 40 databasus >&2 || true
    die "Databasus did not become healthy"
}

install_systemd() {
    log "installing systemd units"
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] install systemd units from ${ASSET_ROOT}/packaging/systemd"
        return 0
    fi
    local unit
    for unit in \
        pgpanel-blue.service \
        pgpanel-green.service \
        pgpanel-helper.service \
        pgpanel-updater.service \
        pgpanel-update-check.service \
        pgpanel-update-check.timer; do
        run install -m 0644 -o root -g root \
            "${ASSET_ROOT}/packaging/systemd/${unit}" \
            "/etc/systemd/system/${unit}"
    done
    run systemctl daemon-reload
}

run_migrations() {
    # Migrations are embedded in pgpanel-web and run transactionally before it
    # starts accepting requests.
    log "database migrations will run during first web service startup"
}

start_services() {
    log "enabling and restarting PgPanel services"
    if ! run systemctl enable pgpanel-helper.service pgpanel-updater.service pgpanel-blue.service; then
        print_service_diagnostics
        die "failed to enable PgPanel services"
    fi
    if ! run systemctl restart pgpanel-helper.service pgpanel-updater.service pgpanel-blue.service; then
        print_service_diagnostics
        die "failed to restart PgPanel services"
    fi
    run systemctl stop pgpanel-green.service 2>/dev/null || true
    run systemctl disable pgpanel-green.service 2>/dev/null || true

    log "enabling update check timer"
    run systemctl enable --now pgpanel-update-check.timer
}

print_service_diagnostics() {
    [[ "${DRY_RUN}" == "1" ]] && return 0
    local unit
    printf '\n[pgpanel-install] Service diagnostics:\n' >&2
    for unit in pgpanel-helper.service pgpanel-updater.service pgpanel-blue.service caddy.service; do
        printf '\n--- systemctl status %s ---\n' "${unit}" >&2
        systemctl status "${unit}" --no-pager --full --lines=12 >&2 || true
        printf '%s\n' "--- recent journal: ${unit} ---" >&2
        journalctl -u "${unit}" --no-pager --lines=25 --output=short-precise >&2 || true
    done
}

wait_for_health() {
    local url="$1"
    local attempts=30
    local i
    if [[ "${DRY_RUN}" == "1" ]]; then
        log "[dry-run] skip health wait at ${url}"
        return 0
    fi
    log "waiting for health at ${url}"
    for ((i = 1; i <= attempts; i++)); do
        if curl -fsS "${url}/health/live" >/dev/null 2>&1 \
            && curl -fsS "${url}/health/ready" >/dev/null 2>&1; then
            log "health check passed"
            return 0
        fi
        sleep 2
    done
    print_service_diagnostics
    die "health check failed at ${url}"
}

start_caddy() {
    log "enabling and restarting Caddy"
    if ! run systemctl enable caddy.service || ! run systemctl restart caddy.service; then
        print_service_diagnostics
        die "failed to restart Caddy"
    fi
    wait_for_health "http://${CADDY_LISTEN}"
}

check_services() {
    [[ "${DRY_RUN}" == "1" ]] && return 0
    local units=(pgpanel-helper.service pgpanel-updater.service pgpanel-blue.service caddy.service)
    if [[ "${EXPOSURE}" == "cloudflare" ]]; then
        units+=(cloudflared.service)
    fi
    local unit
    for unit in "${units[@]}"; do
        if ! systemctl is-active --quiet "${unit}"; then
            print_service_diagnostics
            if [[ "${unit}" == "cloudflared.service" ]]; then
                print_cloudflared_diagnostics
            fi
            die "${unit} is not active after installation"
        fi
    done
}

print_summary() {
    local access
    case "${EXPOSURE}" in
        local)
            access="http://${CADDY_LISTEN} (host loopback only)"
            ;;
        cloudflare)
            access="Cloudflare dashboard hostname; origin http://${CADDY_LISTEN}"
            ;;
        public)
            access="https://${HOSTNAME}"
            ;;
    esac

    cat <<EOF

PgPanel installation complete.

  Access:        ${access}
  Local health:  http://${CADDY_LISTEN}
  Active slot:   blue (${BLUE_LISTEN})
  Config:        ${CONFIG_DIR}/pgpanel.toml
  State:         ${STATE_DIR}
  Install root:  ${INSTALL_ROOT}
  Release root:  ${ASSET_ROOT}
  Exposure:      ${EXPOSURE}

PgPanel itself remains loopback-only. Caddy exposure is configured for
${EXPOSURE}. Do not expose PostgreSQL or the blue/green slot ports.

Existing /etc/pgpanel configuration, /var/lib/pgpanel state, and PostgreSQL
clusters are preserved across re-runs.

Next steps:
  1. Open the mode-specific access URL and complete administrator setup.
  2. Review ${CONFIG_DIR}/pgpanel.toml and service status.
  3. See INSTALL.md for exposure-mode and Databasus-specific guidance.

EOF

    if [[ "${DATABASUS_CHOICE}" == "yes" ]]; then
        cat <<EOF
Databasus is loopback-only at http://127.0.0.1:4005.
For remote administration, use an SSH tunnel:
  ssh -L 4005:127.0.0.1:4005 <user>@<server>
Then open http://127.0.0.1:4005 and manage backups in the Databasus UI.

EOF
    fi
}

main() {
    parse_args "$@"
    resolve_install_choices
    require_root
    verify_ubuntu
    check_commands
    local arch
    arch="$(detect_arch)"
    log "detected architecture: ${arch}"
    log "release source: github.com/${GITHUB_OWNER}/${GITHUB_REPO}"
    log "exposure mode: ${EXPOSURE}"
    if [[ "${REPAIR}" == "1" ]]; then
        log "repair mode: reinstalling release packaging and restarting services"
    fi

    validate_cloudflared_ownership
    install_dependencies
    check_post_deps_commands
    ensure_user
    ensure_directories

    # Verify and extract the release before copying packaging assets. curl|bash
    # hosts have no local repo tree; assets come from the signed archive.
    install_release "${arch}"
    [[ -n "${ASSET_ROOT}" ]] || die "internal error: ASSET_ROOT unset"

    install_config
    configure_selected_settings
    install_signing_key
    install_caddy
    install_systemd
    install_databasus
    run_migrations
    start_services
    configure_public_firewall
    start_caddy
    configure_cloudflared_lifecycle
    check_services
    print_summary
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
