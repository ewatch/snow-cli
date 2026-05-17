#!/usr/bin/env bash
# snow-cli install script
# Detects platform/architecture, downloads the latest release from GitHub,
# and installs both snow-cli and snow-cli-ro to a local bin directory.
#
# Usage:
#   curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.sh | bash
#   
# Or with explicit install directory:
#   curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.sh | bash -s -- --install-dir /usr/local/bin

set -euo pipefail

REPO="ewatch/snow-cli"
INSTALL_DIR="${INSTALL_DIR:-}"
FORCE="${FORCE:-false}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

error() {
    echo -e "${RED}error:${NC} $1" >&2
    exit 1
}

info() {
    echo -e "${GREEN}info:${NC} $1"
}

warn() {
    echo -e "${YELLOW}warn:${NC} $1"
}

detect_platform() {
    local os arch
    os=$(uname -s)
    arch=$(uname -m)

    case "$os" in
        Linux)
            os="unknown-linux-gnu"
            ;;
        Darwin)
            os="apple-darwin"
            ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT)
            error "Windows is not supported by this installer. Please download the Windows archive from the GitHub release page manually."
            ;;
        *)
            error "Unsupported operating system: $os"
            ;;
    esac

    case "$arch" in
        x86_64)
            arch="x86_64"
            ;;
        arm64 | aarch64)
            arch="aarch64"
            ;;
        *)
            error "Unsupported architecture: $arch"
            ;;
    esac

    echo "${arch}-${os}"
}

get_latest_release_tag() {
    local tag
    tag=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$tag" ]; then
        error "Could not determine latest release tag from GitHub API."
    fi
    echo "$tag"
}

find_install_dir() {
    if [ -n "$INSTALL_DIR" ]; then
        echo "$INSTALL_DIR"
        return
    fi

    # Prefer ~/.local/bin if it exists or is creatable
    if [ -d "$HOME/.local/bin" ] || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
        echo "$HOME/.local/bin"
        return
    fi

    # Fallback to /usr/local/bin if writable
    if [ -w "/usr/local/bin" ]; then
        echo "/usr/local/bin"
        return
    fi

    # Last resort: install to ~/.snow-cli/bin
    mkdir -p "$HOME/.snow-cli/bin"
    echo "$HOME/.snow-cli/bin"
}

download_and_install() {
    local platform="$1"
    local tag="$2"
    local install_dir="$3"
    local archive_name="snow-cli-${platform}.tar.gz"
    local url="https://github.com/${REPO}/releases/download/${tag}/${archive_name}"
    local tmpdir
    tmpdir=$(mktemp -d)

    info "Downloading ${url} ..."
    curl -fsSL "$url" -o "${tmpdir}/${archive_name}" || error "Failed to download release archive."

    info "Extracting archive ..."
    tar -xzf "${tmpdir}/${archive_name}" -C "$tmpdir"

    # Find the extracted directory (cargo-dist creates a folder like snow-cli-*/)
    local extracted_dir
    extracted_dir=$(find "$tmpdir" -maxdepth 1 -type d -name 'snow-cli*' | head -n 1)
    if [ -z "$extracted_dir" ]; then
        # Some archives may place binaries directly at the root
        extracted_dir="$tmpdir"
    fi

    local binaries=("snow-cli" "snow-cli-ro")
    for binary in "${binaries[@]}"; do
        local src="${extracted_dir}/${binary}"
        if [ ! -f "$src" ]; then
            # Try nested directory structure
            src=$(find "$extracted_dir" -name "$binary" -type f | head -n 1)
        fi

        if [ -z "$src" ] || [ ! -f "$src" ]; then
            warn "Binary '${binary}' not found in archive. Skipping."
            continue
        fi

        local dest="${install_dir}/${binary}"

        if [ -f "$dest" ] && [ "$FORCE" != "true" ]; then
            warn "${binary} already exists at ${dest}. Use --force to overwrite."
            continue
        fi

        cp "$src" "$dest"
        chmod +x "$dest"
        info "Installed ${binary} -> ${dest}"
    done

    rm -rf "$tmpdir"
}

print_post_install() {
    local install_dir="$1"
    echo
    info "Installation complete!"
    echo
    if ! echo "$PATH" | grep -q "${install_dir}"; then
        warn "${install_dir} is not in your PATH."
        echo "  Add it by running one of the following:"
        echo
        echo "    echo 'export PATH=\"${install_dir}:\$PATH\"' >> ~/.bashrc"
        echo "    echo 'export PATH=\"${install_dir}:\$PATH\"' >> ~/.zshrc"
        echo
        echo "  Then reload your shell:"
        echo
        echo "    source ~/.bashrc   # or source ~/.zshrc"
        echo
    fi
    echo "  Verify the installation:"
    echo
    echo "    ${install_dir}/snow-cli --version"
    echo
}

# --- Parse args ---
while [ $# -gt 0 ]; do
    case "$1" in
        --install-dir)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --force)
            FORCE="true"
            shift
            ;;
        --help | -h)
            cat <<'EOF'
Usage: install.sh [OPTIONS]

Options:
  --install-dir <DIR>  Directory to install binaries into
  --force              Overwrite existing binaries
  --help, -h           Show this help message

Environment variables:
  INSTALL_DIR          Same as --install-dir
  FORCE                Same as --force
EOF
            exit 0
            ;;
        *)
            error "Unknown option: $1"
            ;;
    esac
done

# --- Main ---
echo "snow-cli installer"
echo

PLATFORM=$(detect_platform)
TAG=$(get_latest_release_tag)
INSTALL_DIR=$(find_install_dir)

info "Platform: ${PLATFORM}"
info "Release: ${TAG}"
info "Install directory: ${INSTALL_DIR}"

# Ensure install directory exists
mkdir -p "$INSTALL_DIR"

download_and_install "$PLATFORM" "$TAG" "$INSTALL_DIR"
print_post_install "$INSTALL_DIR"
