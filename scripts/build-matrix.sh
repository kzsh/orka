#!/usr/bin/env bash
#
# Build all release targets and collect binaries into dist/.
# All targets are built inside Docker via cross, so the host only needs
# Docker and cross itself.
#
# Requirements:
#   - Docker (running)
#   - cross  (cargo install cross)
#
# Usage:
#   ./scripts/build-matrix.sh
#   DIST=out ./scripts/build-matrix.sh   # override output directory

set -euo pipefail

DIST="${DIST:-dist}"
mkdir -p "$DIST"

command -v cross &>/dev/null || {
    echo "error: cross not found. Install with: cargo install cross"
    exit 1
}

VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')"

TARGETS=(
    x86_64-unknown-linux-gnu
    x86_64-unknown-linux-musl
    aarch64-unknown-linux-gnu
    aarch64-unknown-linux-musl
)

for target in "${TARGETS[@]}"; do
    echo "==> $target"
    cross build --release --target "$target"
    cp "target/$target/release/pita" "$DIST/pita-$VERSION-$target"
    echo "   -> $DIST/pita-$VERSION-$target"
done

echo ""
echo "Artifacts in $DIST/:"
ls -lh "$DIST/"
