#!/usr/bin/env bash
#
# Publish dist/ artifacts to GitHub releases on kzsh/pita.
#
# Without --release: uploads unversioned artifacts to a rolling "latest"
# GitHub release.  No versioned release entity is created and no git tag
# is applied.  Useful for pushing an updated binary without bumping the
# version.
#
# With --release: creates (or recreates) a versioned GitHub release with
# both versioned and unversioned artifacts, and tags the current commit in
# the local repo.  If the release already exists on GitHub it is deleted
# and recreated, and the local git tag is force-updated.
#
# Prerequisites:
#   - gh (GitHub CLI): https://cli.github.com
#   - gh auth login
#   - dist/ populated by scripts/build-matrix.sh
#
# Usage:
#   ./scripts/publish.sh             # rolling latest
#   ./scripts/publish.sh --release   # versioned release + git tag

set -euo pipefail

REPO="kzsh/pita"
DIST="${DIST:-dist}"
DO_RELEASE=false

for arg in "$@"; do
    case "$arg" in
        --release) DO_RELEASE=true ;;
        *) echo "error: unknown argument: $arg"; exit 1 ;;
    esac
done

command -v gh &>/dev/null || {
    echo "error: gh not found. Install from https://cli.github.com"
    exit 1
}

VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')"
VERSION_TAG="v$VERSION"

mapfile -t versioned < <(find "$DIST" -maxdepth 1 -name "pita-$VERSION-*" -type f | sort)

if [[ ${#versioned[@]} -eq 0 ]]; then
    echo "error: no artifacts found in $DIST/. Run scripts/build-matrix.sh first."
    exit 1
fi

unversioned=()
for f in "${versioned[@]}"; do
    base="$(basename "$f")"
    stripped="$DIST/${base/$VERSION-/}"  # pita-0.0.1-x86_64-... -> pita-x86_64-...
    cp "$f" "$stripped"
    unversioned+=("$stripped")
done

if [[ "$DO_RELEASE" == true ]]; then
    TAG="$VERSION_TAG"
    artifacts=("${versioned[@]}" "${unversioned[@]}")
else
    TAG="latest"
    artifacts=("${unversioned[@]}")
fi

echo "Tag:        $TAG"
echo "Repo:       $REPO"
echo "Artifacts:"
for f in "${artifacts[@]}"; do
    printf "  %s\n" "$(basename "$f")"
done
echo ""

if gh release view "$TAG" --repo "$REPO" &>/dev/null; then
    if [[ "$DO_RELEASE" == true ]]; then
        echo "$TAG already exists on $REPO — deleting and re-releasing."
    fi
    gh release delete "$TAG" --repo "$REPO" --yes
fi

gh release create "$TAG" \
    --repo "$REPO" \
    --title "$TAG" \
    --notes '' \
    "${artifacts[@]}"

if [[ "$DO_RELEASE" == true ]]; then
    if git rev-parse "$TAG" &>/dev/null; then
        git tag -f "$TAG"
        echo "Re-tagged local commit $(git rev-parse --short HEAD) as $TAG"
    else
        git tag "$TAG"
        echo "Tagged local commit $(git rev-parse --short HEAD) as $TAG"
    fi
fi

echo "Published: https://github.com/$REPO/releases/tag/$TAG"
