#!/usr/bin/env bash
#
# Regenerate THIRD_PARTY_LICENSES from dependency metadata.
# Commit the result; the binary embeds it via include_str! at build time.
#
# Requirements:
#   cargo install cargo-about
#
# Usage:
#   ./scripts/gen-licenses.sh

set -euo pipefail

cd "$(dirname "$0")/.."

command -v cargo-about &>/dev/null || {
    echo "error: cargo-about not found. Install with: cargo install cargo-about --features=cli"
    exit 1
}

cargo about generate --manifest-path Cargo.toml about.hbs > THIRD_PARTY_LICENSES

echo "Generated THIRD_PARTY_LICENSES ($(wc -l < THIRD_PARTY_LICENSES) lines)"
echo "Commit this file so the next build picks it up."
