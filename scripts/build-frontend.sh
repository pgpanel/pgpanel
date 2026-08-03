#!/usr/bin/env bash
# Build Tailwind CSS and vendor HTMX/Alpine for PgPanel.
# Build-time only; Node is not required at runtime (vendored assets are committed).
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly INPUT_CSS="${REPO_ROOT}/static/css/input.css"
readonly OUTPUT_CSS="${REPO_ROOT}/static/css/app.css"
readonly STATIC_JS="${REPO_ROOT}/static/js"
readonly HTMX_SRC="${REPO_ROOT}/node_modules/htmx.org/dist/htmx.min.js"
readonly ALPINE_SRC="${REPO_ROOT}/node_modules/alpinejs/dist/cdn.min.js"
readonly HTMX_DST="${STATIC_JS}/htmx.min.js"
readonly ALPINE_DST="${STATIC_JS}/alpine.min.js"

cd "${REPO_ROOT}"

if [[ ! -f package.json ]]; then
    echo "package.json not found in ${REPO_ROOT}" >&2
    exit 1
fi

if ! command -v npm >/dev/null 2>&1; then
    echo "npm is required to build frontend assets (build-time only)" >&2
    exit 1
fi

if [[ -f package-lock.json ]]; then
    npm ci
else
    npm install
fi

mkdir -p "$(dirname "${OUTPUT_CSS}")" "${STATIC_JS}"

npx tailwindcss \
    -i "${INPUT_CSS}" \
    -o "${OUTPUT_CSS}" \
    --minify

echo "Built ${OUTPUT_CSS}"

# Alpine CDN build (not CSP): inline x-data="{ ... }" / @click expressions need eval.
# Switch to alpinejs CSP build only if templates use Alpine.data() components exclusively.
if [[ ! -f "${HTMX_SRC}" ]]; then
    echo "missing ${HTMX_SRC}" >&2
    exit 1
fi
if [[ ! -f "${ALPINE_SRC}" ]]; then
    echo "missing ${ALPINE_SRC}" >&2
    exit 1
fi

cp "${HTMX_SRC}" "${HTMX_DST}"
cp "${ALPINE_SRC}" "${ALPINE_DST}"

echo "Vendored ${HTMX_DST}"
echo "Vendored ${ALPINE_DST}"
