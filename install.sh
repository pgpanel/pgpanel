#!/usr/bin/env bash
# =============================================================================
# PgPanel entrypoint
#
# One-line VPS install (after you push to GitHub and set PGPANEL_REPO):
#
#   sudo apt-get update && sudo apt-get install -y curl && \
#   curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
#
# Local checkout:
#
#   sudo bash install.sh
#   sudo bash deploy/install.sh
# =============================================================================
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Prefer full remote bootstrap (clone + install) when available
if [[ -f "${ROOT}/install-pgpanel.sh" ]]; then
  exec bash "${ROOT}/install-pgpanel.sh" "$@"
fi

# Fallback: local deploy installer only
if [[ -f "${ROOT}/deploy/install.sh" ]]; then
  exec bash "${ROOT}/deploy/install.sh" "$@"
fi

echo "install.sh: cannot find install-pgpanel.sh or deploy/install.sh" >&2
exit 1
