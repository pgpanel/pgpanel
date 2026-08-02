#!/usr/bin/env bash
# Run ShellCheck on installer scripts
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPTS=(
  "$ROOT/deploy/install.sh"
  "$ROOT/deploy/pgpanel"
  "$ROOT/tests/installer/shellcheck.sh"
)

if ! command -v shellcheck >/dev/null 2>&1; then
  echo "shellcheck not installed — skip (install via apt/brew)"
  exit 0
fi

shellcheck -x -s bash "${SCRIPTS[@]}"
echo "ShellCheck OK"
