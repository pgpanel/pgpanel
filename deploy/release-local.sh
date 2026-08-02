#!/usr/bin/env bash
# =============================================================================
# Local release from Mac (Apple Silicon M-series OK)
#
# 1) Builds Docker image (linux/amd64 for typical VPS; optional multi)
# 2) Pushes image to ghcr.io/pgpanel/pgpanel  (+ auto-bumps VERSION patch)
# 3) Commits VERSION + source and pushes to GitHub
#
# First-time setup on Mac:
#   brew install docker git
#   # Docker Desktop running + buildx available
#   echo "ghp_XXX" | docker login ghcr.io -u YOUR_GITHUB_USER --password-stdin
#   # PAT scopes: write:packages, read:packages, repo
#   git remote -v   # origin → https://github.com/pgpanel/pgpanel.git
#
# Usage:
#   ./deploy/release-local.sh                 # image + :latest, then git push (asks)
#   ./deploy/release-local.sh --yes           # no confirmations
#   ./deploy/release-local.sh --image-only    # only Docker push (still bumps VERSION)
#   ./deploy/release-local.sh --git-only      # only git push
#   ./deploy/release-local.sh --multi         # amd64+arm64 image
#   ./deploy/release-local.sh --tag 0.1.1     # force this version (then auto +1)
#   ./deploy/release-local.sh --no-bump       # keep VERSION unchanged after push
# =============================================================================
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(tr -d ' \n\r' < VERSION 2>/dev/null || echo 0.1.0)"
TAG=""
MULTI=0
IMAGE_ONLY=0
GIT_ONLY=0
YES=0
NO_BUMP=0
MSG=""
PLATFORMS="linux/amd64"   # VPS was x86_64 — default amd64 even on M5
PUBLISHED_VERSION=""

C_CYAN=$'\033[36m'
C_GREEN=$'\033[32m'
C_YELLOW=$'\033[33m'
C_BOLD=$'\033[1m'
C_RESET=$'\033[0m'

log()  { printf '%s==>%s %s\n' "$C_CYAN" "$C_RESET" "$*"; }
ok()   { printf '%sOK%s  %s\n' "$C_GREEN" "$C_RESET" "$*"; }
warn() { printf '%s!!%s  %s\n' "$C_YELLOW" "$C_RESET" "$*"; }
die()  { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag) TAG="${2:-}"; shift 2 ;;
    --tag=*) TAG="${1#*=}"; shift ;;
    --multi) MULTI=1; PLATFORMS="linux/amd64,linux/arm64"; shift ;;
    --image-only) IMAGE_ONLY=1; shift ;;
    --git-only) GIT_ONLY=1; shift ;;
    --yes|-y) YES=1; shift ;;
    --no-bump) NO_BUMP=1; shift ;;
    --message|-m) MSG="${2:-}"; shift 2 ;;
    --platform) PLATFORMS="${2:-}"; shift 2 ;;
    -h|--help) sed -n '2,40p' "$0"; exit 0 ;;
    *) die "Unknown arg: $1" ;;
  esac
done

VERSION="${TAG:-$VERSION}"
VERSION="${VERSION#v}"
FULL_IMAGE="ghcr.io/pgpanel/pgpanel"

confirm() {
  [[ "$YES" -eq 1 ]] && return 0
  local a
  read -r -p "$1 [y/N] " a || true
  [[ "${a,,}" == "y" || "${a,,}" == "yes" ]]
}

need_cmd() { command -v "$1" >/dev/null 2>&1 || die "Missing command: $1"; }

# ── Image build + push ───────────────────────────────────────────────────────
do_image() {
  need_cmd docker
  docker info >/dev/null 2>&1 || die "Docker is not running (start Docker Desktop)"

  if ! grep -q 'ghcr.io' ~/.docker/config.json 2>/dev/null; then
    warn "Not logged in to ghcr.io?"
    echo "  echo \"\$GHCR_TOKEN\" | docker login ghcr.io -u YOUR_GITHUB_USER --password-stdin"
    echo "  PAT needs: write:packages, read:packages, repo"
    confirm "Continue anyway?" || die "Aborted"
  fi

  log "Building via push-image.sh (auto patch bump after success)"
  local args=(--latest --platform "$PLATFORMS")
  if [[ "$MULTI" -eq 1 ]]; then
    args=(--latest --multi)
  fi
  if [[ -n "$TAG" ]]; then
    args+=(--tag "$VERSION")
  fi
  if [[ "$NO_BUMP" -eq 1 ]]; then
    args+=(--no-bump)
  fi

  bash "${ROOT}/deploy/push-image.sh" "${args[@]}"

  if [[ -f "${ROOT}/.last-published-version" ]]; then
    PUBLISHED_VERSION="$(tr -d ' \n\r' <"${ROOT}/.last-published-version")"
  else
    PUBLISHED_VERSION="$VERSION"
  fi
  VERSION="$(tr -d ' \n\r' < VERSION)"
  ok "Image pushed: ${FULL_IMAGE}:${PUBLISHED_VERSION} and :latest"
  ok "VERSION file now: ${VERSION}"
}

# ── Git push (source files) ──────────────────────────────────────────────────
do_git() {
  need_cmd git
  [[ -d .git ]] || die "Not a git repository. Run: git init && git remote add origin https://github.com/pgpanel/pgpanel.git"

  local branch
  branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo main)"
  log "Git status on branch: ${branch}"
  git status -sb

  VERSION="$(tr -d ' \n\r' < VERSION 2>/dev/null || echo "$VERSION")"
  if [[ -z "$PUBLISHED_VERSION" && -f .last-published-version ]]; then
    PUBLISHED_VERSION="$(tr -d ' \n\r' <.last-published-version)"
  fi
  PUBLISHED_VERSION="${PUBLISHED_VERSION:-$VERSION}"

  if [[ -z "$(git status --porcelain 2>/dev/null)" ]]; then
    warn "No local file changes to commit."
    if git rev-parse "@{u}" >/dev/null 2>&1; then
      if [[ -n "$(git log --oneline '@{u}..HEAD' 2>/dev/null)" ]]; then
        log "Unpushed commits exist — pushing…"
        git push -u origin "$branch"
        ok "Git push done"
      else
        ok "Already up to date with remote"
      fi
    else
      log "No upstream — pushing branch…"
      git push -u origin "$branch"
    fi
    return 0
  fi

  echo ""
  echo "${C_BOLD}Will stage & commit these changes:${C_RESET}"
  git status --short
  echo ""
  confirm "Commit and push to origin/${branch}?" || die "Git push skipped/aborted"

  git add -A
  # Don't commit secrets / local sidecar noise
  git reset HEAD -- .env 2>/dev/null || true
  git reset HEAD -- '**/.env' 2>/dev/null || true
  git reset HEAD -- .last-published-version .next-version 2>/dev/null || true
  # ensure VERSION is committed
  git add VERSION 2>/dev/null || true

  local cmsg="${MSG:-Release ${PUBLISHED_VERSION}: image ${FULL_IMAGE}:${PUBLISHED_VERSION} (next VERSION ${VERSION})}"
  git commit -m "$cmsg" || warn "Nothing new to commit (maybe only .env was staged)"
  git push -u origin "$branch"
  ok "GitHub source updated: origin/${branch}"
}

# ── Main ─────────────────────────────────────────────────────────────────────
cat <<EOF
${C_BOLD}${C_CYAN}
  PgPanel local release (Mac)
${C_RESET}
  Starting VERSION: ${VERSION}
  Image:            ${FULL_IMAGE}:… (+ :latest)
  Platforms:        ${PLATFORMS}
  Image step:       $([ "$GIT_ONLY" -eq 1 ] && echo skip || echo yes)
  Git push:         $([ "$IMAGE_ONLY" -eq 1 ] && echo skip || echo yes)
  Auto-bump:        $([ "$NO_BUMP" -eq 1 ] && echo no || echo "yes (patch +1 per push)")

EOF

if [[ "$GIT_ONLY" -eq 0 ]]; then
  do_image
fi

if [[ "$IMAGE_ONLY" -eq 0 ]]; then
  do_git
fi

VERSION="$(tr -d ' \n\r' < VERSION 2>/dev/null || echo "$VERSION")"
PUBLISHED_VERSION="${PUBLISHED_VERSION:-$VERSION}"

cat <<EOF

${C_GREEN}${C_BOLD}Release steps done.${C_RESET}

  Published image: ${FULL_IMAGE}:${PUBLISHED_VERSION}
  VERSION file:    ${VERSION}

${C_YELLOW}If GitHub → Packages still shows "No packages published":${C_RESET}
  • Push did not land under ghcr.io/pgpanel/pgpanel (auth / org rights).
  • Re-login with write:packages on the pgpanel org:
      echo "\$GHCR_TOKEN" | docker login ghcr.io -u DezBenedek --password-stdin
  • Re-run:  ./deploy/push-image.sh --latest
  • Then: Package settings → Visibility → Public

On the VPS:

  curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash

  # or resume after a failed pull:
  sudo bash /opt/pgpanel/deploy/install.sh --mode resume

EOF
