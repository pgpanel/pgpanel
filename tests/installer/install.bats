#!/usr/bin/env bats
# Bats tests for PgPanel installer helpers (no root / no Docker required for unit suite)
# Run: bats tests/installer/install.bats

setup() {
  export PGPANEL_INSTALLER_LIB_ONLY=1
  # shellcheck source=/dev/null
  source "${BATS_TEST_DIRNAME}/../../deploy/install.sh"
}

@test "mask_secrets redacts password assignments" {
  run mask_secrets "export password=supersecret123 token=abc"
  [ "$status" -eq 0 ]
  [[ "$output" != *supersecret123* ]]
  [[ "$output" == *REDACTED* ]]
}

@test "mask_secrets redacts postgres connection strings" {
  run mask_secrets "postgresql://user:hiddens3cret@host:5432/db"
  [ "$status" -eq 0 ]
  [[ "$output" != *hiddens3cret* ]]
  [[ "$output" == *postgresql://user:***@* ]]
}

@test "gen_secret produces long non-empty output" {
  run gen_secret
  [ "$status" -eq 0 ]
  [ "${#output}" -ge 32 ]
}

@test "gen_password produces at least 24 chars" {
  run gen_password
  [ "$status" -eq 0 ]
  [ "${#output}" -ge 24 ]
}

@test "cpu_cores returns a positive integer" {
  run cpu_cores
  [ "$status" -eq 0 ]
  [[ "$output" =~ ^[0-9]+$ ]]
  [ "$output" -ge 1 ]
}

@test "ram_mb returns a number" {
  run ram_mb
  [ "$status" -eq 0 ]
  [[ "$output" =~ ^[0-9]+$ ]]
}

@test "detect_os sets PKG_MGR" {
  detect_os
  [ -n "$OS_ID" ]
  [ -n "$PKG_MGR" ]
}

@test "mask_secrets leaves clean text alone" {
  run mask_secrets "hello world"
  [ "$status" -eq 0 ]
  [ "$output" = "hello world" ]
}

@test "have_cmd finds bash" {
  run have_cmd bash
  [ "$status" -eq 0 ]
}

@test "have_cmd fails for nonsense binary" {
  run have_cmd this-binary-should-not-exist-pgpanel-xyz
  [ "$status" -ne 0 ]
}

@test "is_valid_hostname accepts a public hostname" {
  run is_valid_hostname "backup.example.com"
  [ "$status" -eq 0 ]
}

@test "is_valid_hostname rejects URLs and malformed labels" {
  run is_valid_hostname "https://backup.example.com"
  [ "$status" -ne 0 ]
  run is_valid_hostname "backup..example.com"
  [ "$status" -ne 0 ]
}

@test "is_valid_tunnel_token rejects short or whitespace values" {
  run is_valid_tunnel_token "short-token"
  [ "$status" -ne 0 ]
  run is_valid_tunnel_token "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa token"
  [ "$status" -ne 0 ]
}

@test "is_valid_tunnel_token accepts a complete token-shaped value" {
  run is_valid_tunnel_token "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abcdefghijklmnopqrstuvwxyz"
  [ "$status" -eq 0 ]
}

@test "Databasus data directory permits internal postgres traversal" {
  tmp="$(mktemp -d)"
  DATA_DIR="$tmp/data"
  ENABLE_DATABASUS=1
  ensure_databasus_data_dir
  [ -d "$DATA_DIR/databasus" ]
  run stat -c '%a' "$DATA_DIR/databasus"
  [ "$status" -eq 0 ]
  [ "$output" = "755" ]
  rm -rf "$tmp"
}

@test "Tunnel rendering omits host ports and includes both sidecars" {
  tmp="$(mktemp -d)"
  INSTALL_DIR="$tmp"
  DATA_DIR="$tmp/data"
  PGPANEL_VERSION="0.1.14"
  PGPANEL_IMAGE=""
  UPDATE_CHANNEL="stable"
  ENABLE_DATABASUS=1
  USE_CLOUDFLARE_TUNNEL=1
  DATABASUS_PUBLIC=1
  PANEL_DOMAIN="panel.example.com"
  DATABASUS_DOMAIN="backup.example.com"
  LETSENCRYPT_EMAIL="admin@example.com"
  render_compose
  render_caddyfile
  run grep -q '^    ports:' "$tmp/deploy/compose.yml"
  [ "$status" -ne 0 ]
  run grep -q 'databasus/databasus:v3.51.0' "$tmp/deploy/compose.yml"
  [ "$status" -eq 0 ]
  run grep -q 'cloudflare/cloudflared:2026.7.3' "$tmp/deploy/compose.yml"
  [ "$status" -eq 0 ]
  run grep -q 'backup.example.com' "$tmp/deploy/Caddyfile"
  [ "$status" -eq 0 ]
  rm -rf "$tmp"
}

@test "Tunnel rendering never writes the token into generated Compose" {
  tmp="$(mktemp -d)"
  INSTALL_DIR="$tmp"
  DATA_DIR="$tmp/data"
  PGPANEL_VERSION="0.1.14"
  PGPANEL_IMAGE=""
  UPDATE_CHANNEL="stable"
  ENABLE_DATABASUS=1
  USE_CLOUDFLARE_TUNNEL=1
  DATABASUS_PUBLIC=1
  PANEL_DOMAIN="panel.example.com"
  DATABASUS_DOMAIN="backup.example.com"
  CLOUDFLARE_TUNNEL_TOKEN="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.secret-value"
  render_compose
  run grep -q 'secret-value' "$tmp/deploy/compose.yml"
  [ "$status" -ne 0 ]
  rm -rf "$tmp"
}
