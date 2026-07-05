#!/usr/bin/env bash
#
# Syncs config/environments.yaml into public-repo/, commits, and pushes.
# Called by scripts/publish-release.sh as part of the release flow.
#
# Usage:
#   ./scripts/update-public-repo.sh

set -euo pipefail

VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')"
TAG="v$VERSION"

cd public-repo
git add -A
if git diff --cached --quiet; then
    echo "public-repo: nothing changed, skipping commit."
else
    git commit -m "Release $TAG"
fi
git push
