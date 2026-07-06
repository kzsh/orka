#!/usr/bin/env bash
#
# Push dist/ artifacts to a GitHub release on kzsh/pita, then update public-repo.
#
# The release tag is derived from the version in Cargo.toml (prefixed with v).
# Exits cleanly if that release already exists.
#
# Prerequisites:
#   - gh (GitHub CLI): https://cli.github.com
#   - gh auth login
#   - dist/ populated by scripts/build-matrix.sh
#
# Usage:
#   ./scripts/publish-release.sh

set -euo pipefail

REPO="kzsh/pita"
DIST="${DIST:-dist}"

command -v gh &>/dev/null || {
    echo "error: gh not found. Install from https://cli.github.com"
    exit 1
}

VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')"
TAG="v$VERSION"

if gh release view "$TAG" --repo "$REPO" &>/dev/null; then
    echo "$TAG already exists on $REPO — nothing to do."
    exit 0
fi

mapfile -t versioned < <(find "$DIST" -maxdepth 1 -name "pita-$VERSION-*" -type f | sort)

if [[ ${#versioned[@]} -eq 0 ]]; then
    echo "error: no artifacts found in $DIST/. Run scripts/build-matrix.sh first."
    exit 1
fi

# Create unversioned copies alongside the versioned ones for the stable
# /releases/latest/download/<name> URLs.
unversioned=()
for f in "${versioned[@]}"; do
    base="$(basename "$f")"
    stripped="$DIST/${base/$VERSION-/}"  # pita-0.0.1-x86_64-... -> pita-x86_64-...
    cp "$f" "$stripped"
    unversioned+=("$stripped")
done

artifacts=("${versioned[@]}" "${unversioned[@]}")

echo "Tag:        $TAG"
echo "Repo:       $REPO"
echo "Artifacts:"
for f in "${artifacts[@]}"; do
    printf "  %s\n" "$(basename "$f")"
done
echo ""

gh release create "$TAG" \
    --repo "$REPO" \
    --title "$TAG" \
    --notes '' \
    "${artifacts[@]}"

echo "Released: https://github.com/$REPO/releases/tag/$TAG"
