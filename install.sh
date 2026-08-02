#!/usr/bin/env bash
# PgPanel entrypoint — production one-liner uses install-pgpanel.sh
# Developer: Dezső Benedek Péter
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "${ROOT}/install-pgpanel.sh" ]]; then
  exec bash "${ROOT}/install-pgpanel.sh" "$@"
fi
if [[ -f "${ROOT}/deploy/install.sh" ]]; then
  exec bash "${ROOT}/deploy/install.sh" "$@"
fi
echo "install.sh: installer not found" >&2
exit 1
