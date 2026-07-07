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
#   - dist/ populated by scripts/build.sh
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

mapfile -t unversioned < <(find "$DIST" -maxdepth 1 -name "pita-*" -not -name "pita-$VERSION-*" -type f | sort)

if [[ ${#unversioned[@]} -eq 0 ]]; then
    echo "error: no artifacts found in $DIST/. Run scripts/build.sh first."
    exit 1
fi

if [[ "$DO_RELEASE" == true ]]; then
    TAG="$VERSION_TAG"
    versioned=()
    for f in "${unversioned[@]}"; do
        base="$(basename "$f")"
        dest="$DIST/pita-$VERSION-${base#pita-}"  # pita-x86_64-... -> pita-0.0.1-x86_64-...
        cp "$f" "$dest"
        versioned+=("$dest")
    done
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
    gh release delete "$TAG" --repo "$REPO" --cleanup-tag --yes
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

DATE="$(date -u +%Y-%m-%d)"
README="public-repo/README.md"
sed -i "s|^\*\*Latest:\*\*.*|**Latest:** $DATE|" "$README"

if [[ "$DO_RELEASE" == true ]]; then
    COMMIT_MSG="Release $TAG"
else
    COMMIT_MSG="Update latest ($DATE)"
fi

(cd public-repo && git add README.md && git diff --cached --quiet || git commit -m "$COMMIT_MSG" && git push)

echo "Published: https://github.com/$REPO/releases/tag/$TAG"
