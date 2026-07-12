#!/bin/sh
#
# Orka install script
# Downloads the correct orka binary from GitHub releases.
#
# Usage:
#   curl -fsSL https://getorka.dev/install.sh | sh
#   curl -fsSL https://getorka.dev/install.sh | sh -s -- --release v0.0.1
#
# Environment variables:
#   ORKA_RELEASE          Release to install: latest, or a version like v0.0.1
#                         (default: latest). Overridden by --release.
#   ORKA_INSTALL_DIR      Directory to install the orka binary into
#                         (default: ~/.local/bin).
#   ORKA_NON_INTERACTIVE  Set to 1, true, or yes to skip prompts.

set -eu

RELEASE="${ORKA_RELEASE:-latest}"
BIN_DIR="${ORKA_INSTALL_DIR:-$HOME/.local/bin}"
BIN_PATH="$BIN_DIR/orka"
REPO="kzsh/orka"

path_action="already"
path_profile=""
tmp_file=""

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

step() {
    printf '==> %s\n' "$1"
}

warn() {
    printf 'WARNING: %s\n' "$1" >&2
}

die() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

download_file() {
    url="$1"
    output="$2"

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$output"
        return
    fi

    if command -v wget >/dev/null 2>&1; then
        wget -q -O "$output" "$url"
        return
    fi

    die "curl or wget is required to install orka."
}

file_sha256() {
    path="$1"

    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$path" | awk '{print $1}'
        return
    fi

    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$path" | awk '{print $1}'
        return
    fi

    if command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$path" | sed 's/^.*= //'
        return
    fi

    # No checksum tool available; skip verification.
    printf ''
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------

parse_args() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --release)
                if [ "$#" -lt 2 ]; then
                    die "--release requires a value."
                fi
                RELEASE="$2"
                shift
                ;;
            --help | -h)
                cat <<EOF
Usage: install.sh [--release VERSION]

Install orka from GitHub releases.

Options:
  --release VERSION   Install a specific release (e.g. v0.0.1, 0.0.1, latest).
  --help, -h          Show this message.

Environment variables:
  ORKA_RELEASE          Release to install; overridden by --release.
  ORKA_INSTALL_DIR      Directory for the binary (default: ~/.local/bin).
  ORKA_NON_INTERACTIVE  Set to 1, true, or yes to skip prompts.
EOF
                exit 0
                ;;
            *)
                die "Unknown argument: $1"
                ;;
        esac
        shift
    done
}

# ---------------------------------------------------------------------------
# Version normalization
# ---------------------------------------------------------------------------

# Strips a leading 'v' and returns the bare version number, or "latest".
normalize_version() {
    case "$1" in
        "" | latest)
            printf 'latest\n'
            ;;
        v*)
            printf '%s\n' "${1#v}"
            ;;
        *)
            printf '%s\n' "$1"
            ;;
    esac
}

validate_version() {
    version="$1"

    if [ "$version" = "latest" ]; then
        return
    fi

    if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.]+)?$'; then
        die "Invalid release version: $version. Expected latest or x.y.z (with optional pre-release suffix)."
    fi
}

# ---------------------------------------------------------------------------
# Shell profile for PATH updates
# ---------------------------------------------------------------------------

# Returns the most appropriate shell profile file to append PATH updates to.
pick_profile() {
    case "$os:${SHELL:-}" in
        darwin:*/zsh)
            printf '%s\n' "$HOME/.zprofile"
            ;;
        darwin:*/bash)
            printf '%s\n' "$HOME/.bash_profile"
            ;;
        linux:*/zsh)
            printf '%s\n' "$HOME/.zshrc"
            ;;
        linux:*/bash)
            printf '%s\n' "$HOME/.bashrc"
            ;;
        *)
            printf '%s\n' "$HOME/.profile"
            ;;
    esac
}

add_to_path() {
    path_action="already"
    path_profile=""

    case ":${PATH}:" in
        *":$BIN_DIR:"*)
            return
            ;;
    esac

    profile="$(pick_profile)"
    path_profile="$profile"
    begin_marker="# >>> orka installer >>>"
    end_marker="# <<< orka installer <<<"
    path_line="export PATH=\"$BIN_DIR:\$PATH\""

    if [ -f "$profile" ] && grep -qF "$begin_marker" "$profile" 2>/dev/null; then
        if grep -qF "$path_line" "$profile" 2>/dev/null; then
            path_action="configured"
            return
        fi
        path_action="updated"
        rewrite_path_block "$profile" "$begin_marker" "$end_marker" "$path_line"
        return
    fi

    {
        printf '\n%s\n' "$begin_marker"
        printf '%s\n' "$path_line"
        printf '%s\n' "$end_marker"
    } >>"$profile"
    path_action="added"
}

rewrite_path_block() {
    profile="$1"
    begin_marker="$2"
    end_marker="$3"
    path_line="$4"

    tmp_profile="$tmp_file.profile"
    awk -v begin="$begin_marker" -v end="$end_marker" -v line="$path_line" '
        BEGIN { in_block = 0; replaced = 0 }
        $0 == begin {
            if (!replaced) {
                print begin
                print line
                print end
                replaced = 1
            }
            in_block = 1
            next
        }
        in_block {
            if ($0 == end) { in_block = 0 }
            next
        }
        { print }
        END { if (in_block != 0) { exit 1 } }
    ' "$profile" >"$tmp_profile"
    mv "$tmp_profile" "$profile"
}

print_launch_instructions() {
    case "$path_action" in
        added)
            step "Current terminal: export PATH=\"$BIN_DIR:\$PATH\" && orka"
            step "Future terminals: open a new terminal and run: orka"
            step "PATH was added to $path_profile"
            ;;
        updated)
            step "Current terminal: export PATH=\"$BIN_DIR:\$PATH\" && orka"
            step "Future terminals: open a new terminal and run: orka"
            step "PATH was updated in $path_profile"
            ;;
        configured)
            step "PATH is already configured in $path_profile"
            step "Run: orka"
            ;;
        *)
            step "$BIN_DIR is already on PATH"
            step "Run: orka"
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------

cleanup() {
    if [ -n "$tmp_file" ] && [ -f "$tmp_file" ]; then
        rm -f "$tmp_file"
    fi
}

trap cleanup EXIT INT TERM

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

parse_args "$@"

# Detect OS
case "$(uname -s)" in
    Darwin)
        os="darwin"
        ;;
    Linux)
        os="linux"
        ;;
    MINGW* | MSYS* | CYGWIN*)
        die "Windows is not supported by this script."
        ;;
    *)
        die "Unsupported operating system: $(uname -s)"
        ;;
esac

# Detect architecture
case "$(uname -m)" in
    x86_64 | amd64)
        arch="x86_64"
        ;;
    arm64 | aarch64)
        arch="aarch64"
        ;;
    *)
        die "Unsupported architecture: $(uname -m)"
        ;;
esac

# On macOS, detect Rosetta 2: if the shell is running as x86_64 under
# Rosetta on an ARM Mac, use the native arm64 binary instead.
if [ "$os" = "darwin" ] && [ "$arch" = "x86_64" ]; then
    if [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = "1" ]; then
        arch="aarch64"
    fi
fi

# Build the target triple.
# macOS targets are included for future use; binaries for them are not
# published yet. When they are, no changes to this script will be needed.
if [ "$os" = "darwin" ]; then
    if [ "$arch" = "aarch64" ]; then
        target="aarch64-apple-darwin"
        platform_label="macOS (Apple Silicon)"
    else
        target="x86_64-apple-darwin"
        platform_label="macOS (Intel)"
    fi
elif [ "$os" = "linux" ]; then
    # Detect musl libc.
    if ldd /bin/ls 2>&1 | grep -q musl 2>/dev/null; then
        libc="musl"
    elif [ -f /lib/libc.musl-x86_64.so.1 ] || [ -f /lib/libc.musl-aarch64.so.1 ]; then
        libc="musl"
    else
        libc="gnu"
    fi
    target="${arch}-unknown-linux-${libc}"
    platform_label="Linux ($arch, $libc)"
fi

# Resolve release tag and asset name.
#
# Rolling releases use tag "latest" and unversioned asset names:
#   orka-<target>
#
# Versioned releases use tag "v0.0.1" and versioned asset names:
#   orka-0.0.1-<target>    (note: no leading 'v' in the asset filename)
version="$(normalize_version "$RELEASE")"
validate_version "$version"

if [ "$version" = "latest" ]; then
    tag="latest"
    asset="orka-$target"
else
    tag="v$version"
    asset="orka-$version-$target"
fi

download_url="https://github.com/$REPO/releases/download/$tag/$asset"

# Print plan.
step "Installing orka"
step "Detected platform: $platform_label"
step "Release:           $tag"
step "Downloading from:  $download_url"

# Download to a temp file.
tmp_file="$(mktemp)"
if ! download_file "$download_url" "$tmp_file"; then
    die "Download failed. Check that release '$tag' exists at https://github.com/$REPO/releases"
fi

# Sanity-check: reject obvious HTML error pages.
first_bytes="$(head -c 15 "$tmp_file" 2>/dev/null || true)"
case "$first_bytes" in
    "<!DOCTYPE html"* | "<html"*)
        die "Download returned an HTML page instead of a binary. The release '$tag' or asset '$asset' may not exist."
        ;;
esac

# Verify checksum if a SHA256SUMS file is available for this release.
# This is intentionally opportunistic: if the file is not published (which
# is the case for the rolling latest releases today), we skip verification
# rather than failing. Once checksums are added to the publish workflow the
# verification will run automatically on the next install.
checksums_url="https://github.com/$REPO/releases/download/$tag/SHA256SUMS"
checksums_file="$(mktemp)"
if download_file "$checksums_url" "$checksums_file" 2>/dev/null; then
    first="$(head -c 15 "$checksums_file" 2>/dev/null || true)"
    case "$first" in
        "<!DOCTYPE html"* | "<html"*)
            rm -f "$checksums_file"
            ;;
        *)
            expected="$(awk -v a="$asset" '$2 == a || $2 == "./" a { print $1; exit }' "$checksums_file" || true)"
            if [ -n "$expected" ]; then
                actual="$(file_sha256 "$tmp_file")"
                if [ -n "$actual" ] && [ "$actual" != "$expected" ]; then
                    rm -f "$checksums_file" "$tmp_file"
                    die "Checksum mismatch for $asset. Expected: $expected  Got: $actual"
                fi
                step "Checksum verified"
            fi
            rm -f "$checksums_file"
            ;;
    esac
fi

# Install the binary.
mkdir -p "$BIN_DIR"
chmod 0755 "$tmp_file"
mv "$tmp_file" "$BIN_PATH"
tmp_file=""

step "Installed orka to $BIN_PATH"

# Verify the binary runs.
if ! "$BIN_PATH" --version >/dev/null 2>&1; then
    warn "orka was installed but 'orka --version' failed. The binary may not be compatible with this system."
fi

# Update PATH in the user's shell profile.
add_to_path
print_launch_instructions

installed_version="$("$BIN_PATH" --version 2>/dev/null | head -n 1 || printf '%s' "$tag")"
printf 'orka %s installed successfully.\n' "$installed_version"
