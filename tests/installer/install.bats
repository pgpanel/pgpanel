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
