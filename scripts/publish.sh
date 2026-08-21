#!/usr/bin/env bash
# Publishes the api + web images to GitHub Packages (GHCR) as a perpetual
# watchdog loop with permanent root privileges:
#
#   1. Re-executes itself via sudo, so the whole loop stays elevated (the
#      sudo password is asked exactly once).
#   2. Checks for NEW COMMITS every POLL_INTERVAL (default 5s) — only
#      committed changes count: the fingerprint is the HEAD sha, so a
#      rebuild/push happens when new commits land locally (git pull or
#      local commits). Uncommitted working-tree edits do NOT trigger it.
#   3. On change: builds + pushes the images, then fires the deploy webhook
#      2s later.
#   4. Repeats forever. The last-built fingerprint is kept in
#      .last-published-sha, so a restart does not rebuild an unchanged tree.
#
# Tagging scheme (applied to every change):
#   no args                 -> dev build:  dev (rolling) + dev-<short-sha>
#   ./publish.sh vX.Y.Z     -> release:    latest + vX.Y.Z + X.Y + <short-sha>
#
# Prerequisites:
#   sudo docker login ghcr.io -u <user> -p <PAT>   # PAT scope: write:packages
#     (the loop runs as root, so the docker login must be root's)
#   The git remote must be fetchable by the invoking user (git is run as that
#     user, so existing credentials/SSH keys are reused).
#
# Usage:
#   ./scripts/publish.sh            # watchdog: build+push on every change
#   ./scripts/publish.sh --no-push  # watchdog, build+tag only (smoke-test)
#   ./scripts/publish.sh v1.2.3     # watchdog with release tags
#
# Tuning (edit in place):
#   POLL_INTERVAL  seconds between change checks (default 5)
#   STATE_FILE     where the last-built fingerprint is stored
set -euo pipefail

# --- permanently switch to root (single sudo prompt, whole loop elevated) ---
if [[ "$(id -u)" -ne 0 ]]; then
    echo "==> re-executing with sudo: the whole publish loop runs as root"
    SCRIPT_PATH="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
    exec sudo bash "$SCRIPT_PATH" "$@"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$SCRIPT_DIR"
while [[ ! -e "$REPO_ROOT/.git" && "$REPO_ROOT" != "/" ]]; do
    REPO_ROOT="$(dirname "$REPO_ROOT")"
done
if [[ ! -e "$REPO_ROOT/.git" ]]; then
    echo "error: no git repository found above $SCRIPT_DIR" >&2
    exit 1
fi
cd "$REPO_ROOT"

REGISTRY="ghcr.io/shu-red"
# Package names MUST match the compose image: fields (ephorix-api, ephorix-web).
IMAGES=(ephorix-api:backend ephorix-web:frontend)

# Deploy webhook fired after every successful push (not a secret, kept here).
WEBHOOK_URL="http://192.168.178.55:3552/api/webhooks/trigger/arc_wh_e3de7c0172972cade81730d17d06d03fc97a7e432366e55869a392da6189efac39a6cdbfe9a220ffb8a559cd8338f3dc4142f5dab367d63123d26ade3db6add0eb9ae70609d3a244b7fc70526c044410fa005cd1971f28aa961e09ff"

POLL_INTERVAL="${POLL_INTERVAL:-5}"
STATE_FILE="${STATE_FILE:-$REPO_ROOT/.last-published-sha}"

NO_PUSH=0
RELEASE_TAG=""
for arg in "$@"; do
    case "$arg" in
        --no-push) NO_PUSH=1 ;;
        -*) echo "error: unknown option '$arg'" >&2; exit 1 ;;
        *) RELEASE_TAG="$arg" ;;
    esac
done

if [[ -n "$RELEASE_TAG" ]]; then
    if [[ "$RELEASE_TAG" != v* ]]; then
        echo "error: release tag must start with 'v', got '$RELEASE_TAG'" >&2
        exit 1
    fi
    MAJOR_MINOR="${RELEASE_TAG%.*}"
    BASE_TAGS=("latest" "$RELEASE_TAG" "$MAJOR_MINOR")
else
    BASE_TAGS=("dev")
fi

# git runs as the invoking user (keeps their credentials/SSH setup); the
# safe.directory override covers repos not owned by the current uid.
if [[ -n "${SUDO_USER:-}" && "$SUDO_USER" != "root" ]]; then
    GIT=(sudo -u "$SUDO_USER" git -c "safe.directory=$REPO_ROOT")
else
    GIT=(git -c "safe.directory=$REPO_ROOT")
fi

BRANCH="$("${GIT[@]}" rev-parse --abbrev-ref HEAD)"
UPSTREAM="$("${GIT[@]}" rev-parse --abbrev-ref --symbolic-full-name @{u} 2>/dev/null || echo "origin/$BRANCH")"
REMOTE="${UPSTREAM%%/*}"

# Fingerprint = the local HEAD commit only (full + short sha). Committed
# changes only: a rebuild/push happens when new commits land (git pull or
# local commits); uncommitted working-tree edits are ignored. Provenance is
# baked into the image as frontend/version.json (see build loop below).
build_fingerprint() {
    local sha short
    sha="$("${GIT[@]}" rev-parse HEAD 2>/dev/null || echo none)"
    short="$("${GIT[@]}" rev-parse --short=7 HEAD 2>/dev/null || echo none)"
    echo "${sha}:${short}"
}

build_and_push() {
    local name="$1" ctx="$2" base_tag="$3"
    shift 3
    local image="$REGISTRY/$name"
    echo "==> [$(date '+%F %T')] building $image:$base_tag from $ctx"
    docker build -t "$image:$base_tag" "$ctx" || return 1
    local t
    for t in "$@"; do
        docker tag "$image:$base_tag" "$image:$t" || return 1
    done
    if [[ "$NO_PUSH" == "1" ]]; then
        echo "==> --no-push: tagged locally as"
        for t in "$base_tag" "$@"; do
            echo "    $image:$t"
        done
        return 0
    fi
    for t in "$base_tag" "$@"; do
        echo "==> [$(date '+%F %T')] pushing $image:$t"
        docker push "$image:$t" || return 1
    done
}

LAST_FP="$(cat "$STATE_FILE" 2>/dev/null || true)"
echo "==> publish watchdog up: branch=$BRANCH remote=$REMOTE poll=${POLL_INTERVAL}s"
echo "==> last published fingerprint: ${LAST_FP:-<none>}  state: $STATE_FILE"

while true; do
    # Pull remote commits (best-effort): only committed changes count, and
    # ff-only keeps the local history linear (fails safely on conflicts).
    "${GIT[@]}" fetch --quiet "$REMOTE" 2>/dev/null || true
    "${GIT[@]}" merge --ff-only "$UPSTREAM" >/dev/null 2>&1 || true

    FP="$(build_fingerprint)"
    if [[ "$FP" == "$LAST_FP" ]]; then
        echo "==> [$(date '+%F %T')] no change; next check in ${POLL_INTERVAL}s"
        sleep "$POLL_INTERVAL"
        continue
    fi

    SHA="$("${GIT[@]}" rev-parse --short=7 HEAD)"
    FULL_SHA="$("${GIT[@]}" rev-parse HEAD)"

    # Bake build provenance into the web image: the Docker build copies the
    # working tree, so version.json placed here lands in the image and is
    # served by nginx + rendered in the footer. Git-ignored (see .gitignore).
    {
        printf '{"sha":"%s","fullSha":"%s","branch":"%s","builtAt":"%s"}\n' \
            "$SHA" "$FULL_SHA" "$BRANCH" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    } > frontend/version.json

    if [[ -n "$RELEASE_TAG" ]]; then
        TAGS=("${BASE_TAGS[@]}" "$SHA")
    else
        TAGS=("dev" "dev-$SHA")
    fi

    failed=0
    for spec in "${IMAGES[@]}"; do
        name="${spec%%:*}"
        ctx="${spec##*:}"
        if ! build_and_push "$name" "$ctx" "${TAGS[0]}" "${TAGS[@]:1}"; then
            failed=1
            break
        fi
    done
    if [[ "$failed" == "1" ]]; then
        echo "==> [$(date '+%F %T')] build/push failed; retrying on next poll" >&2
        sleep "$POLL_INTERVAL"
        continue
    fi

    if [[ "$NO_PUSH" == "1" ]]; then
        echo "==> [$(date '+%F %T')] --no-push: built locally, push + webhook skipped"
        sleep "$POLL_INTERVAL"
        continue
    fi

    echo "$FP" > "$STATE_FILE"
    LAST_FP="$FP"
    echo "==> [$(date '+%F %T')] published $SHA; firing deploy webhook in 2s"
    sleep 2
    if curl -sS -m 30 -X POST "$WEBHOOK_URL"; then
        echo "==> [$(date '+%F %T')] webhook triggered"
    else
        echo "==> [$(date '+%F %T')] webhook request failed" >&2
    fi
    echo "==> [$(date '+%F %T')] next check in ${POLL_INTERVAL}s"
    sleep "$POLL_INTERVAL"
done
