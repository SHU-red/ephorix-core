#!/usr/bin/env bash
# Builds the api + web images locally and publishes them to GitHub Packages
# (GHCR) under the shu-red/ephorix-* names.
#
# Tagging (mirrors .github/workflows/build.yml):
#   no args                 -> dev build:  dev (rolling) + dev-<short-sha>
#   ./publish.sh vX.Y.Z     -> release:    latest + vX.Y.Z + X.Y + <short-sha>
#
# Prerequisites:
#   docker login ghcr.io -u <user> -p <PAT>   # PAT scope: write:packages
#
# Usage:
#   ./scripts/publish.sh            # dev build (dev, dev-<sha>)
#   ./scripts/publish.sh --no-push  # build + tag only, no push (smoke-test)
#   ./scripts/publish.sh v1.2.3     # release (latest, v1.2.3, 1.2, <sha>)
set -euo pipefail

REGISTRY="ghcr.io/shu-red"
IMAGES=(api:backend web:frontend)
NO_PUSH=0

if [[ "${1:-}" == "--no-push" ]]; then
    NO_PUSH=1
    shift || true
fi
RELEASE_TAG="${1:-}"

SHA="$(git rev-parse --short=7 HEAD)"
if [[ -n "$RELEASE_TAG" ]]; then
    if [[ "$RELEASE_TAG" != v* ]]; then
        echo "error: release tag must start with 'v', got '$RELEASE_TAG'" >&2
        exit 1
    fi
    MAJOR_MINOR="${RELEASE_TAG%.*}"
    TAGS=("latest" "$RELEASE_TAG" "$MAJOR_MINOR" "$SHA")
    echo "==> publishing RELEASE $RELEASE_TAG (sha $SHA)"
else
    TAGS=("dev" "dev-$SHA")
    echo "==> publishing DEV build (sha $SHA)"
fi

build_and_push() {
    local name="$1" ctx="$2" base_tag="$3"
    shift 3
    local image="$REGISTRY/$name"
    echo "==> building $image:$base_tag from $ctx"
    docker build -t "$image:$base_tag" "$ctx"
    local t
    for t in "$@"; do
        docker tag "$image:$base_tag" "$image:$t"
    done
    if [[ "$NO_PUSH" == "1" ]]; then
        echo "==> --no-push: tagged locally as"
        for t in "$base_tag" "$@"; do
            echo "    $image:$t"
        done
        return
    fi
    for t in "$base_tag" "$@"; do
        echo "==> pushing $image:$t"
        docker push "$image:$t"
    done
}

for spec in "${IMAGES[@]}"; do
    name="${spec%%:*}"
    ctx="${spec##*:}"
    build_and_push "$name" "$ctx" "${TAGS[0]}" "${TAGS[@]:1}"
done

echo "==> done. Deploy with EPHORIX_TAG=${TAGS[0]} on the server."
