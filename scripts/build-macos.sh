#!/usr/bin/env bash
#
# Build the macOS (Apple silicon) release binary on a remote Mac and copy the
# result into dist/.
#
# A thin wrapper around mac-cargo.sh: it syncs the working tree to a stable
# remote directory and builds there, so repeat runs are incremental.
#
# Requirements: see mac-cargo.sh.
#
# Usage:
#   ./scripts/build-macos.sh
#   ./scripts/build-macos.sh --host mini.local
#   ./scripts/build-macos.sh --clean          # discard the remote build dir first
#   DIST=out ./scripts/build-macos.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/.."

HOST="${ORKA_BUILD_HOST:-mini.local}"
REMOTE_DIR="${ORKA_BUILD_DIR:-/tmp/orka-build}"
DIST="${DIST:-dist}"
TARGET="aarch64-apple-darwin"
CLEAN=0

usage() {
    cat <<'EOF'
Usage: build-macos.sh [OPTIONS]

  --host HOST      Remote host to build on (default: mini.local,
                   override with $ORKA_BUILD_HOST)
  --dir PATH       Remote build directory (default: /tmp/orka-build,
                   override with $ORKA_BUILD_DIR)
  --target TRIPLE  Rust target triple (default: aarch64-apple-darwin)
  --clean          Remove the remote build directory before building
  -h, --help       Show this message
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host)
            HOST="${2:?--host requires a value}"
            shift 2
            ;;
        --dir)
            REMOTE_DIR="${2:?--dir requires a value}"
            shift 2
            ;;
        --target)
            TARGET="${2:?--target requires a value}"
            shift 2
            ;;
        --clean)
            CLEAN=1
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ "$CLEAN" -eq 1 ]]; then
    echo "==> removing $HOST:$REMOTE_DIR"
    ssh "$HOST" "rm -rf -- $(printf '%q' "$REMOTE_DIR")"
fi

"$SCRIPT_DIR/mac-cargo.sh" \
    --host "$HOST" \
    --dir "$REMOTE_DIR" \
    build --release --target "$TARGET"

mkdir -p "$DIST"
OUT="$DIST/orka-$TARGET"
echo "==> retrieving binary"
scp -q "$HOST:$REMOTE_DIR/target/$TARGET/release/orka" "$OUT"
chmod +x "$OUT"

echo ""
echo "Artifact:"
ls -lh "$OUT"
file "$OUT"
