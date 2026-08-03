#!/usr/bin/env bash
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly INSTALLER="${REPO_ROOT}/scripts/install.sh"

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}

assert_rejected() {
    local description="$1"
    shift
    if (
        # shellcheck source=install.sh
        source "${INSTALLER}"
        parse_args "$@"
    ) >/dev/null 2>&1; then
        fail "${description}"
    fi
}

test_argument_validation() {
    (
        # shellcheck source=install.sh
        source "${INSTALLER}"
        parse_args --exposure local --without-databasus
        [[ "${EXPOSURE}" == "local" ]]
        [[ "${DATABASUS_CHOICE}" == "no" ]]
        is_valid_hostname "panel.example.com"
        ! is_valid_hostname "https://panel.example.com"
        ! is_valid_hostname "panel.example.com:443"
        is_valid_cloudflare_token "abcdefghijklmnopqrstuvwxyz.0123456789"
        ! is_valid_cloudflare_token "token with spaces and insufficient safety"
    ) || fail "valid installer options were not accepted"

    assert_rejected "invalid exposure was accepted" --exposure internet
    assert_rejected "public mode without DNS name was accepted" \
        --exposure public --hostname localhost
    assert_rejected "hostname was accepted for local mode" \
        --exposure local --hostname panel.example.com
    assert_rejected "short Cloudflare token was accepted" \
        --exposure cloudflare --cloudflare-token short
}

test_cloudflared_management_decisions() {
    (
        # shellcheck source=install.sh
        source "${INSTALLER}"
        [[ "$(cloudflared_management_decision cloudflare 0 0)" == "install" ]]
        [[ "$(cloudflared_management_decision cloudflare 1 1)" == "reinstall" ]]
        [[ "$(cloudflared_management_decision cloudflare 0 1)" == "conflict" ]]
        [[ "$(cloudflared_management_decision local 1 1)" == "remove" ]]
        [[ "$(cloudflared_management_decision public 0 1)" == "preserve" ]]
    ) || fail "cloudflared ownership decision was unsafe"
}

test_release_path_output_discipline() {
    local tmp installer_copy release_root required rel output
    tmp="$(mktemp -d)"
    installer_copy="${tmp}/install.sh"
    sed \
        -e "s|readonly INSTALL_ROOT=\"/opt/pgpanel\"|readonly INSTALL_ROOT=\"${tmp}/opt/pgpanel\"|" \
        -e "s|readonly CONFIG_DIR=\"/etc/pgpanel\"|readonly CONFIG_DIR=\"${tmp}/etc/pgpanel\"|" \
        "${INSTALLER}" >"${installer_copy}"

    release_root="${tmp}/release"
    required=(
        bin/pgpanel-web
        bin/pgpanel-helper
        bin/pgpanel-updater
        config/pgpanel.toml.example
        packaging/caddy/Caddyfile
        packaging/caddy/pgpanel-upstream.caddy
        packaging/systemd/pgpanel-blue.service
        packaging/systemd/pgpanel-green.service
        packaging/systemd/pgpanel-helper.service
        packaging/systemd/pgpanel-updater.service
        packaging/systemd/pgpanel-update-check.service
        packaging/systemd/pgpanel-update-check.timer
    )
    for rel in "${required[@]}"; do
        mkdir -p "$(dirname "${release_root}/${rel}")"
        : >"${release_root}/${rel}"
    done
    chmod 0755 "${release_root}/bin/"*
    mkdir -p "${tmp}/opt/pgpanel/releases"

    # Run in the current shell: the installed path is returned through the
    # global, while stdout remains exclusively human-readable logging.
    # shellcheck source=/dev/null
    source "${installer_copy}"
    # Keep the filesystem test portable to macOS, where the system group is
    # "wheel" rather than "root".
    install() {
        local -a filtered=()
        while [[ $# -gt 0 ]]; do
            case "$1" in
                -o | -g)
                    shift 2
                    ;;
                *)
                    filtered+=("$1")
                    shift
                    ;;
            esac
        done
        command install "${filtered[@]}"
    }
    chown() {
        :
    }
    find() {
        chmod 0755 "$1"/*
    }
    install_release_tree "${release_root}" "0.1.3" >"${tmp}/install.log"
    [[ "${INSTALLED_RELEASE_PATH}" == "${tmp}/opt/pgpanel/releases/0.1.3" ]] \
        || fail "installed release path was not returned through the global"
    [[ -x "${INSTALLED_RELEASE_PATH}/bin/pgpanel-web" ]] \
        || fail "release tree was not installed"

    output="$(declare -f prepare_bundle_dir)"
    [[ "${output}" != *'$(install_release_tree '* ]] \
        || fail "prepare_bundle_dir still captures install logs in command substitution"

    mkdir -p "${tmp}/etc/pgpanel"
    cat >"${tmp}/etc/pgpanel/pgpanel.toml" <<'EOF'
[databasus]
enabled = false
enabled = false

[server] # externally visible URL
listen = "127.0.0.1:8081"
public_base_url = "https://old.example.com"

[session]   # cookies
secure = true

[trusted_proxies] # proxy trust
enabled = true
proxies = ["127.0.0.1", "::1"]
prefer_cf_connecting_ip = true
EOF
    local config_file="${tmp}/etc/pgpanel/pgpanel.toml"
    set_toml_value "${config_file}" databasus enabled true
    set_toml_value "${config_file}" databasus base_url '"http://127.0.0.1:4005"'
    set_toml_value "${config_file}" session secure true
    remove_toml_key "${config_file}" server public_base_url
    [[ "$(awk '
        /^[[:space:]]*\[/ { in_databasus = ($0 ~ /^[[:space:]]*\[databasus\]/) }
        in_databasus && /^[[:space:]]*enabled[[:space:]]*=/ { count++ }
        END { print count+0 }
    ' "${config_file}")" == "1" ]] \
        || fail "TOML update left duplicate Databasus keys"
    [[ "$(awk '/^[[:space:]]*base_url[[:space:]]*=/{count++} END{print count+0}' "${config_file}")" == "1" ]] \
        || fail "TOML update did not add exactly one Databasus base_url"
    [[ "$(awk '/^[[:space:]]*\[session\]/{count++} END{print count+0}' "${config_file}")" == "1" ]] \
        || fail "commented TOML section caused a duplicate table"
    [[ "$(awk '/^[[:space:]]*secure[[:space:]]*=/{count++} END{print count+0}' "${config_file}")" == "1" ]] \
        || fail "commented TOML section caused a duplicate key"
    [[ "$(awk '/^[[:space:]]*public_base_url[[:space:]]*=/{count++} END{print count+0}' "${config_file}")" == "0" ]] \
        || fail "TOML key removal did not remove stale public_base_url"
    awk '$0 == "[session]   # cookies" { found=1 } END { exit !found }' "${config_file}" \
        || fail "TOML section comments were not preserved"

    EXPOSURE="local"
    configure_selected_settings
    awk '
        /^[[:space:]]*secure[[:space:]]*=[[:space:]]*false/ { secure=1 }
        /^[[:space:]]*prefer_cf_connecting_ip[[:space:]]*=[[:space:]]*false/ { cf=1 }
        /^[[:space:]]*public_base_url[[:space:]]*=/ { stale=1 }
        END { exit !(secure && cf && !stale) }
    ' "${config_file}" || fail "local exposure transition left incompatible settings"

    EXPOSURE="public"
    HOSTNAME="panel.example.com"
    configure_selected_settings
    awk '
        /^[[:space:]]*public_base_url[[:space:]]*=[[:space:]]*"https:\/\/panel.example.com"/ { url=1 }
        /^[[:space:]]*secure[[:space:]]*=[[:space:]]*true/ { secure=1 }
        /^[[:space:]]*prefer_cf_connecting_ip[[:space:]]*=[[:space:]]*false/ { cf=1 }
        END { exit !(url && secure && cf) }
    ' "${config_file}" || fail "public exposure transition was incomplete"

    EXPOSURE="cloudflare"
    configure_selected_settings
    awk '
        /^[[:space:]]*public_base_url[[:space:]]*=/ { stale=1 }
        /^[[:space:]]*secure[[:space:]]*=[[:space:]]*true/ { secure=1 }
        /^[[:space:]]*prefer_cf_connecting_ip[[:space:]]*=[[:space:]]*false/ { cf=1 }
        END { exit !(!stale && secure && cf) }
    ' "${config_file}" || fail "Cloudflare exposure transition left incompatible settings"

    if command -v python3 >/dev/null 2>&1; then
        python3 - "${config_file}" <<'PY'
import pathlib
import sys
import tomllib

tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
PY
    fi
    rm -rf "${tmp}"
}

test_argument_validation
test_cloudflared_management_decisions
test_release_path_output_discipline
printf 'installer regression tests passed\n'
