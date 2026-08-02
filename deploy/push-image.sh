#!/usr/bin/env bash
# Build PgPanel Docker image on your machine and push to GHCR.
# Faster than GitHub Actions multi-arch QEMU builds.
#
# Every successful publish auto-increments the patch by +1, then builds that
# version (0.1.0 → publish 0.1.1, VERSION file becomes 0.1.1).
# VERSION always matches the image just published — required for VPS pull.
#
# Prerequisites:
#   - Docker Buildx
#   - Logged in to ghcr.io (PAT with write:packages, or gh auth)
#
# Usage:
#   ./deploy/push-image.sh                  # bump patch, build, push, write VERSION
#   ./deploy/push-image.sh --multi          # amd64 + arm64
#   ./deploy/push-image.sh --tag 0.2.0      # publish exactly 0.2.0 (no auto-bump)
#   ./deploy/push-image.sh --no-bump        # publish current VERSION as-is
#   ./deploy/push-image.sh --no-push        # build only (no bump)
#   ./deploy/push-image.sh --latest         # also tag :latest
#
# Login examples:
#   echo "$GHCR_TOKEN" | docker login ghcr.io -u YOUR_GITHUB_USER --password-stdin
#   # token needs: write:packages, read:packages, (optional) delete:packages
#   # For org images (ghcr.io/pgpanel/...): org package write + visibility Public for VPS pulls
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

REGISTRY="${REGISTRY:-ghcr.io}"
IMAGE_NAME="${IMAGE_NAME:-pgpanel/pgpanel}"
TAG=""
PLATFORMS="linux/amd64"
PUSH=1
ALSO_LATEST=0
NO_BUMP=0
BUILDER_NAME="pgpanel-builder"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag) TAG="${2:-}"; shift 2 ;;
    --tag=*) TAG="${1#*=}"; shift ;;
    --multi) PLATFORMS="linux/amd64,linux/arm64"; shift ;;
    --platform) PLATFORMS="${2:-}"; shift 2 ;;
    --platform=*) PLATFORMS="${1#*=}"; shift ;;
    --no-push) PUSH=0; shift ;;
    --no-bump) NO_BUMP=1; shift ;;
    --latest) ALSO_LATEST=1; shift ;;
    --registry) REGISTRY="${2:-}"; shift 2 ;;
    --image) IMAGE_NAME="${2:-}"; shift 2 ;;
    -h|--help)
      sed -n '2,30p' "$0"
      exit 0
      ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

# ── Version helpers ──────────────────────────────────────────────────────────
read_version_file() {
  local f="${1:-VERSION}"
  [[ -f "$f" ]] || { echo "0.1.0"; return; }
  tr -d ' \n\r' <"$f"
}

# Semver patch +1: 0.1.0 → 0.1.1 ; 1.2 → 1.2.1 ; unknown → <v>.1
bump_patch() {
  local v="${1#v}"
  if [[ "$v" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)([.-].*)?$ ]]; then
    printf '%s.%s.%s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}" "$((BASH_REMATCH[3] + 1))"
  elif [[ "$v" =~ ^([0-9]+)\.([0-9]+)$ ]]; then
    printf '%s.%s.1\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
  elif [[ "$v" =~ ^([0-9]+)$ ]]; then
    printf '%s.0.1\n' "${BASH_REMATCH[1]}"
  else
    printf '%s.1\n' "$v"
  fi
}

write_version_file() {
  local v="$1"
  printf '%s\n' "$v" >VERSION
}

PREV_VERSION="$(read_version_file VERSION)"
PREV_VERSION="${PREV_VERSION#v}"

# Decide version to publish:
#   --tag X     → exact X (no auto-bump)
#   --no-bump   → current VERSION file
#   default     → patch +1 from VERSION, then publish that (so each push is new)
if [[ -n "$TAG" ]]; then
  VERSION="${TAG#v}"
  BUMPED=0
elif [[ "$NO_BUMP" -eq 1 || "$PUSH" -eq 0 ]]; then
  VERSION="$PREV_VERSION"
  BUMPED=0
else
  VERSION="$(bump_patch "$PREV_VERSION")"
  BUMPED=1
fi

# Persist target version before build so a crash mid-build still records intent
write_version_file "$VERSION"

FULL_IMAGE="${REGISTRY}/${IMAGE_NAME}"
TAGS=(-t "${FULL_IMAGE}:${VERSION}")
if [[ "$ALSO_LATEST" -eq 1 ]]; then
  TAGS+=(-t "${FULL_IMAGE}:latest")
fi

echo "Image:     ${FULL_IMAGE}"
if [[ "$BUMPED" -eq 1 ]]; then
  echo "Version:   ${PREV_VERSION} → ${VERSION}  (auto patch +1)"
else
  echo "Version:   ${VERSION}  (from $([ -n "$TAG" ] && echo "--tag" || echo "VERSION / --no-bump"))"
fi
echo "Platforms: ${PLATFORMS}"
echo "Push:      ${PUSH}"
echo ""

if [[ "$PUSH" -eq 1 ]]; then
  if ! cat ~/.docker/config.json 2>/dev/null | grep -q "ghcr.io"; then
    echo "WARN: no ghcr.io entry in ~/.docker/config.json — login first:"
    echo "  echo \$GHCR_TOKEN | docker login ghcr.io -u YOUR_USER --password-stdin"
    echo "  # PAT scopes: write:packages, read:packages, repo"
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

# VERSION already written to the published tag — must match GHCR for VPS pulls
write_version_file "$VERSION"
printf '%s\n' "$VERSION" >"${ROOT}/.last-published-version"

echo ""
echo "Done."
if [[ "$PUSH" -eq 1 ]]; then
  if [[ "$BUMPED" -eq 1 ]]; then
    echo "VERSION: ${PREV_VERSION} → ${VERSION} (auto +1), image published."
  else
    echo "VERSION: ${VERSION} published (no auto-bump)."
  fi
  echo "Published image:"
  echo "  ${FULL_IMAGE}:${VERSION}"
  [[ "$ALSO_LATEST" -eq 1 ]] && echo "  ${FULL_IMAGE}:latest"
  echo ""
  echo "IMPORTANT — VPS pull needs a PUBLIC package:"
  echo "  1) Open: https://github.com/orgs/pgpanel/packages  (or user Packages)"
  echo "  2) Package → Package settings → Change visibility → Public"
  echo "  3) If 'No packages published', the push failed auth — re-login and re-run."
  echo ""
  echo "Verify from any machine (no login if Public):"
  echo "  docker pull ${FULL_IMAGE}:${VERSION}"
  echo ""
  echo "Commit VERSION to GitHub so the installer pin matches:"
  echo "  git add VERSION && git commit -m \"chore: release ${VERSION}\" && git push"
  echo "  # or: ./deploy/release-local.sh --yes   (image+git together)"
  echo ""
  echo "On VPS:"
  echo "  sudo bash /opt/pgpanel/deploy/install.sh --mode resume"
else
  echo "Local image loaded: ${FULL_IMAGE}:${VERSION}"
fi

export PGPANEL_PUBLISHED_VERSION="$VERSION"
