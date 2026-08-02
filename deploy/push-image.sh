#!/usr/bin/env bash
# Build PgPanel Docker image on your machine and push to GHCR.
# Faster than GitHub Actions multi-arch QEMU builds.
#
# Prerequisites:
#   - Docker Buildx
#   - Logged in to ghcr.io (PAT with write:packages, or gh auth)
#
# Usage:
#   ./deploy/push-image.sh                  # version from VERSION, platform linux/amd64
#   ./deploy/push-image.sh --multi          # amd64 + arm64 (slow via QEMU if not native)
#   ./deploy/push-image.sh --tag 0.1.1
#   ./deploy/push-image.sh --no-push        # build only
#   ./deploy/push-image.sh --latest         # also tag :latest
#
# Login examples:
#   echo "$GHCR_TOKEN" | docker login ghcr.io -u YOUR_GITHUB_USER --password-stdin
#   # token needs: write:packages, read:packages, (optional) delete:packages
#   # For org images: package visibility + your user must have push rights.
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

REGISTRY="${REGISTRY:-ghcr.io}"
IMAGE_NAME="${IMAGE_NAME:-pgpanel/pgpanel}"
VERSION="$(tr -d ' \n\r' < VERSION)"
TAG=""
PLATFORMS="linux/amd64"
PUSH=1
ALSO_LATEST=0
BUILDER_NAME="pgpanel-builder"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag) TAG="${2:-}"; shift 2 ;;
    --tag=*) TAG="${1#*=}"; shift ;;
    --multi) PLATFORMS="linux/amd64,linux/arm64"; shift ;;
    --platform) PLATFORMS="${2:-}"; shift 2 ;;
    --platform=*) PLATFORMS="${1#*=}"; shift ;;
    --no-push) PUSH=0; shift ;;
    --latest) ALSO_LATEST=1; shift ;;
    --registry) REGISTRY="${2:-}"; shift 2 ;;
    --image) IMAGE_NAME="${2:-}"; shift 2 ;;
    -h|--help)
      sed -n '2,25p' "$0"
      exit 0
      ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

VERSION="${TAG:-$VERSION}"
# strip leading v
VERSION="${VERSION#v}"

FULL_IMAGE="${REGISTRY}/${IMAGE_NAME}"
TAGS=(-t "${FULL_IMAGE}:${VERSION}")
if [[ "$ALSO_LATEST" -eq 1 ]]; then
  TAGS+=(-t "${FULL_IMAGE}:latest")
fi

echo "Image:     ${FULL_IMAGE}"
echo "Version:   ${VERSION}"
echo "Platforms: ${PLATFORMS}"
echo "Push:      ${PUSH}"
echo ""

if [[ "$PUSH" -eq 1 ]]; then
  if ! docker buildx imagetools inspect "${REGISTRY}/${IMAGE_NAME}:${VERSION}" >/dev/null 2>&1; then
    :
  fi
  if ! docker system info 2>/dev/null | grep -qi 'Username'; then
    # Not reliable for buildx auth — just warn
    true
  fi
  if ! cat ~/.docker/config.json 2>/dev/null | grep -q "ghcr.io"; then
    echo "WARN: no ghcr.io entry in ~/.docker/config.json — login first:"
    echo "  echo \$GHCR_TOKEN | docker login ghcr.io -u YOUR_USER --password-stdin"
    echo ""
  fi
fi

# Ensure buildx builder (docker-container driver required for multi-platform + push)
if ! docker buildx inspect "$BUILDER_NAME" >/dev/null 2>&1; then
  echo "Creating buildx builder: ${BUILDER_NAME}"
  docker buildx create --name "$BUILDER_NAME" --driver docker-container --use
  docker buildx inspect --bootstrap
else
  docker buildx use "$BUILDER_NAME"
fi

OUTPUT_FLAGS=(--load)
if [[ "$PUSH" -eq 1 ]]; then
  OUTPUT_FLAGS=(--push)
elif [[ "$PLATFORMS" == *","* ]]; then
  echo "ERROR: multi-platform build cannot --load into local docker; use --no-push only with single platform, or enable push."
  exit 1
fi

# Local cache dir (optional, speeds rebuilds on same machine)
CACHE_DIR="${HOME}/.cache/pgpanel-buildx"
mkdir -p "$CACHE_DIR"

set -x
docker buildx build \
  --platform "$PLATFORMS" \
  -f deploy/Dockerfile \
  --build-arg "PGPANEL_VERSION=${VERSION}" \
  "${TAGS[@]}" \
  --cache-from "type=local,src=${CACHE_DIR}" \
  --cache-to "type=local,dest=${CACHE_DIR},mode=max" \
  "${OUTPUT_FLAGS[@]}" \
  .
set +x

echo ""
echo "Done."
if [[ "$PUSH" -eq 1 ]]; then
  echo "Pulled on VPS with:"
  echo "  docker pull ${FULL_IMAGE}:${VERSION}"
  echo "  # or set in /opt/pgpanel/.env:"
  echo "  PGPANEL_IMAGE=${FULL_IMAGE}:${VERSION}"
  echo "  PGPANEL_VERSION=${VERSION}"
  echo "  cd /opt/pgpanel/deploy && docker compose pull && docker compose up -d"
else
  echo "Local image loaded: ${FULL_IMAGE}:${VERSION}"
fi
