#!/usr/bin/env bash
#
# Run cargo on a remote Mac against a synced copy of the working tree.
#
# The source is rsynced to a stable directory on the remote host, so unchanged
# files are skipped and the cargo target directory survives between runs for
# incremental builds. All arguments are passed to cargo verbatim.
#
# Requirements:
#   - rsync on both hosts
#   - ssh access to the remote host (key-based)
#   - a Rust toolchain on the remote host, installed via rustup
#
# The remote host comes from $ORKA_BUILD_HOST or --host; there is no default.
#
# Usage:
#   ORKA_BUILD_HOST=mini.local ./scripts/mac-cargo.sh test
#   ./scripts/mac-cargo.sh build --release --target aarch64-apple-darwin
#   ./scripts/mac-cargo.sh run -- --engine container --dry-run
#   ./scripts/mac-cargo.sh --host mini.local clippy --all-targets
#
# The build directory is never removed; delete it by hand when you want a cold
# build:
#   ssh "$ORKA_BUILD_HOST" 'rm -rf /tmp/orka-build'

set -euo pipefail

cd "$(dirname "$0")/.."

HOST="${ORKA_BUILD_HOST:-}"
REMOTE_DIR="${ORKA_BUILD_DIR:-/tmp/orka-build}"
CONTAINER_START=1

usage() {
    cat <<'EOF'
Usage: mac-cargo.sh [OPTIONS] <cargo args...>

  --host HOST   Remote host to build on (required, or set
                $ORKA_BUILD_HOST)
  --dir PATH    Remote build directory (default: /tmp/orka-build,
                override with $ORKA_BUILD_DIR)
  --no-container-start
                Do not start Apple's container services when they are down
                (the container backend is alpha)
  -h, --help    Show this message

Everything after the options is passed to cargo unchanged.
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
        --no-container-start)
            CONTAINER_START=0
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            break
            ;;
    esac
done

[[ -n "$HOST" ]] || {
    echo "error: no remote host; set \$ORKA_BUILD_HOST or pass --host" >&2
    usage >&2
    exit 2
}

[[ $# -gt 0 ]] || {
    echo "error: no cargo arguments given" >&2
    usage >&2
    exit 2
}

command -v rsync >/dev/null || {
    echo "error: rsync not found on this host" >&2
    exit 1
}

# Excludes keep the transfer to source only: no history, no build output, and
# none of the sibling checkouts that live inside the working tree.  target/ is
# excluded but not deleted remotely, which is what makes incremental builds
# work; --delete removes source files that have gone away locally.
echo "==> syncing to $HOST:$REMOTE_DIR"
rsync -az --delete \
    --exclude=.git \
    --exclude=.envrc \
    --exclude=target \
    --exclude=dist \
    --exclude=public-repo \
    --exclude=local-reference \
    ./ "$HOST:$REMOTE_DIR/"

# Quote each argument so the remote bash sees exactly what was typed here.
remote_cmd="export PATH=\"\$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin\"
command -v cargo >/dev/null || { echo 'error: cargo not found on $HOST; install rustup' >&2; exit 1; }"

# Anything exercising the alpha --engine container backend needs the launchd
# services up.  Skip silently when the CLI is absent, since most cargo
# subcommands never touch it.
if [[ "$CONTAINER_START" -eq 1 ]]; then
    remote_cmd+="
if command -v container >/dev/null 2>&1 && ! container system status >/dev/null 2>&1; then
    echo '==> starting container services'
    container system start
fi"
fi

remote_cmd+="
cd $(printf '%q' "$REMOTE_DIR")
exec cargo"
for arg in "$@"; do
    remote_cmd+=" $(printf '%q' "$arg")"
done

# A TTY is only requested when this script has one, so piped output stays clean
# while `cargo run` remains interactive.
ssh_opts=()
[[ -t 0 ]] && ssh_opts+=(-t)

echo "==> cargo $*"
ssh "${ssh_opts[@]}" "$HOST" "bash -c $(printf '%q' "$remote_cmd")"
