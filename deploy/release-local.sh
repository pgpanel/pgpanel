#!/usr/bin/env bash
# =============================================================================
# Local release from Mac (Apple Silicon M-series OK)
#
# 1) Builds Docker image (linux/amd64 for typical VPS; optional multi)
# 2) Pushes image to ghcr.io/pgpanel/pgpanel
# 3) Optionally commits + pushes source files to GitHub
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
#   ./deploy/release-local.sh --image-only    # only Docker push
#   ./deploy/release-local.sh --git-only      # only git push
#   ./deploy/release-local.sh --multi         # amd64+arm64 image
#   ./deploy/release-local.sh --tag 0.1.1     # override VERSION
# =============================================================================
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(tr -d ' \n\r' < VERSION)"
TAG=""
MULTI=0
IMAGE_ONLY=0
GIT_ONLY=0
YES=0
MSG=""
PLATFORMS="linux/amd64"   # VPS was x86_64 — default amd64 even on M5

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
    --message|-m) MSG="${2:-}"; shift 2 ;;
    --platform) PLATFORMS="${2:-}"; shift 2 ;;
    -h|--help) sed -n '2,35p' "$0"; exit 0 ;;
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
    confirm "Continue anyway?" || die "Aborted"
  fi

  log "Building ${FULL_IMAGE}:${VERSION}  platforms=${PLATFORMS}"
  export TAG="$VERSION"
  # Reuse push-image.sh
  local args=(--tag "$VERSION" --latest --platform "$PLATFORMS")
  if [[ "$MULTI" -eq 1 ]]; then
    args=(--tag "$VERSION" --latest --multi)
  fi
  bash "${ROOT}/deploy/push-image.sh" "${args[@]}"
  ok "Image pushed: ${FULL_IMAGE}:${VERSION} and :latest"
}

# ── Git push (source files) ──────────────────────────────────────────────────
do_git() {
  need_cmd git
  [[ -d .git ]] || die "Not a git repository. Run: git init && git remote add origin https://github.com/pgpanel/pgpanel.git"

  local branch
  branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo main)"
  log "Git status on branch: ${branch}"
  git status -sb

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
  # Don't commit secrets if any slipped in
  git reset HEAD -- .env 2>/dev/null || true
  git reset HEAD -- '**/.env' 2>/dev/null || true

  local cmsg="${MSG:-Release ${VERSION}: installer, deploy, image ${FULL_IMAGE}:${VERSION}}"
  git commit -m "$cmsg" || warn "Nothing new to commit (maybe only .env was staged)"
  git push -u origin "$branch"
  ok "GitHub source updated: origin/${branch}"
}

# ── Main ─────────────────────────────────────────────────────────────────────
cat <<EOF
${C_BOLD}${C_CYAN}
  PgPanel local release (Mac)
${C_RESET}
  Version:   ${VERSION}
  Image:     ${FULL_IMAGE}:${VERSION} (+ :latest)
  Platforms: ${PLATFORMS}
  Image:     $([ "$GIT_ONLY" -eq 1 ] && echo skip || echo yes)
  Git push:  $([ "$IMAGE_ONLY" -eq 1 ] && echo skip || echo yes)

EOF

if [[ "$GIT_ONLY" -eq 0 ]]; then
  do_image
fi

if [[ "$IMAGE_ONLY" -eq 0 ]]; then
  do_git
fi

cat <<EOF

${C_GREEN}${C_BOLD}Release steps done.${C_RESET}

On the VPS (install / repair / finish after network fix):

  curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash

Or if already installed — update without data loss:

  cd /opt/pgpanel && sudo git pull
  # pin image in .env if needed:
  #   PGPANEL_IMAGE=${FULL_IMAGE}:${VERSION}
  #   PGPANEL_VERSION=${VERSION}
  sudo pgpanel update
  # or:
  cd /opt/pgpanel/deploy && sudo docker compose pull && sudo docker compose up -d

EOF
