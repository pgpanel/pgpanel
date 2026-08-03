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
readonly PGPANEL_USER="pgpanel"
readonly PGPANEL_GROUP="pgpanel"
readonly GITHUB_OWNER="${PGPANEL_GITHUB_OWNER:-pgpanel}"
readonly GITHUB_REPO="${PGPANEL_GITHUB_REPO:-pgpanel}"
readonly RELEASE_CHANNEL="${PGPANEL_CHANNEL:-stable}"
readonly CADDY_LISTEN="127.0.0.1:8080"
readonly BLUE_LISTEN="127.0.0.1:8081"
readonly GREEN_LISTEN="127.0.0.1:8082"

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
ALLOW_UNSUPPORTED_OS="0"
SIGNING_KEY_PATH=""
ASSET_ROOT=""
DOWNLOAD_DIR=""
EXTRACT_DIR=""
RESOLVED_KEY_HEX=""

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
  --signing-key PATH          Path to pinned Ed25519 public key (32-byte hex)
  --allow-unsupported-os      Allow non-Ubuntu-24.04 (unsupported)
  --dry-run                   Print actions without making changes
  -h, --help                  Show this help

Environment:
  PGPANEL_GITHUB_OWNER            GitHub owner (default: pgpanel)
  PGPANEL_GITHUB_REPO             GitHub repository (default: pgpanel)
  PGPANEL_CHANNEL                 Release channel: stable or prerelease
  PGPANEL_SIGNING_PUBLIC_KEY_HEX  Raw 32-byte Ed25519 public key as hex

Examples:
  sudo ./scripts/install.sh --version v0.1.0 --signing-key ./keys/signing.pub
  sudo PGPANEL_SIGNING_PUBLIC_KEY_HEX=<64-hex> ./scripts/install.sh
  sudo ./scripts/install.sh --from-local /tmp/pgpanel-linux-amd64.tar.gz \
       --signing-key /tmp/signing.pub
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
    if ! openssl pkeyutl -verify -pubin -inkey "${pem}" -sigfile "${sig_bin}" -in "${sums_file}" >/dev/null 2>&1; then
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

    if [[ -d "${release_path}" ]]; then
        log "release ${bare_version} already present at ${release_path}"
    else
        log "installing release to ${release_path}"
        if [[ "${DRY_RUN}" == "1" ]]; then
            log "[dry-run] cp -a ${release_root} ${release_path}"
        else
            run cp -a "${release_root}" "${release_path}"
            run chown -R root:root "${release_path}"
            if [[ -d "${release_path}/bin" ]]; then
                run find "${release_path}/bin" -type f -exec chmod 0755 {} +
            fi
        fi
    fi

    printf '%s\n' "${release_path}"
}

setup_slot_symlinks() {
    local release_path="$1"

    if [[ ! -d "${release_path}/bin" && "${DRY_RUN}" != "1" ]]; then
        log "release layout has no bin/; skipping slot symlinks"
        run ln -sfn "${release_path}" "${INSTALL_ROOT}/current"
        return 0
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

    local release_path
    release_path="$(install_release_tree "${ASSET_ROOT}" "${bare}")"
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
        return 0
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
    log "starting pgpanel-helper"
    run systemctl enable --now pgpanel-helper.service

    log "starting pgpanel-updater daemon"
    run systemctl enable --now pgpanel-updater.service

    log "starting pgpanel-blue (active slot)"
    run systemctl enable --now pgpanel-blue.service
    run systemctl stop pgpanel-green.service 2>/dev/null || true
    run systemctl disable pgpanel-green.service 2>/dev/null || true

    log "enabling update check timer"
    run systemctl enable --now pgpanel-update-check.timer
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
    die "health check failed at ${url}"
}

start_caddy() {
    log "starting Caddy"
    run systemctl enable --now caddy.service
    wait_for_health "http://${CADDY_LISTEN}"
}

print_summary() {
    cat <<EOF

PgPanel installation complete.

  Local URL:     http://${CADDY_LISTEN}
  Active slot:   blue (${BLUE_LISTEN})
  Config:        ${CONFIG_DIR}/pgpanel.toml
  State:         ${STATE_DIR}
  Install root:  ${INSTALL_ROOT}
  Release root:  ${ASSET_ROOT}

PgPanel listens on loopback only. To expose it remotely, use Cloudflare Tunnel
or another reverse proxy — do not bind PgPanel to a public interface.

Databasus is not installed by this script. Backup integration is optional and
external; configure it later in pgpanel.toml if you already run Databasus.
See docs/databasus.md.

Existing /etc/pgpanel configuration, /var/lib/pgpanel state, and PostgreSQL
clusters are preserved across re-runs.

Next steps:
  1. Open http://${CADDY_LISTEN} and complete first-run administrator setup.
  2. Configure Cloudflare Tunnel to forward to http://${CADDY_LISTEN}
  3. Before exposing an HTTPS hostname, set session.secure=true and
     server.public_base_url in ${CONFIG_DIR}/pgpanel.toml, then restart both
     web slots.

EOF
}

main() {
    parse_args "$@"
    require_root
    verify_ubuntu
    check_commands
    local arch
    arch="$(detect_arch)"
    log "detected architecture: ${arch}"
    log "release source: github.com/${GITHUB_OWNER}/${GITHUB_REPO}"

    install_dependencies
    check_post_deps_commands
    ensure_user
    ensure_directories

    # Verify and extract the release before copying packaging assets. curl|bash
    # hosts have no local repo tree; assets come from the signed archive.
    install_release "${arch}"
    [[ -n "${ASSET_ROOT}" ]] || die "internal error: ASSET_ROOT unset"

    install_config
    install_signing_key
    install_caddy
    install_systemd
    run_migrations
    start_services
    start_caddy
    print_summary
}

main "$@"
