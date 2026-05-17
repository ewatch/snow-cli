#!/usr/bin/env bash
# snow-cli install script (macOS & Linux)
# Usage: curl -fsSL https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.sh | bash

set -euo pipefail

REPO="ewatch/snow-cli"

# --- Detect platform ---
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Linux)  PLATFORM="${ARCH}-unknown-linux-gnu" ;;
  Darwin) PLATFORM="${ARCH}-apple-darwin" ;;
  *)      echo "This script supports macOS and Linux. For Windows, see:"; echo "  https://github.com/${REPO}/releases"; exit 1 ;;
esac

# --- Resolve install dir ---
INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
mkdir -p "$INSTALL_DIR" 2>/dev/null || {
  echo "Cannot write to ${INSTALL_DIR}. Trying ${HOME}/.snow-cli/bin instead."
  INSTALL_DIR="${HOME}/.snow-cli/bin"
  mkdir -p "$INSTALL_DIR"
}

# --- Discover latest release ---
echo "Checking latest release..."
TAG=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | head -1 | sed -E 's/.*"([^"]+)".*/\1/')
[ -z "$TAG" ] && { echo "Could not find latest release."; exit 1; }

ARCHIVE="snow-cli-${PLATFORM}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"

# --- Show plan ---
echo
echo "Plan:"
echo "  Download: ${URL}"
echo "  Release:  ${TAG}"
echo "  Install to: ${INSTALL_DIR}"
echo "  Binaries: snow-cli, snow-cli-ro"
echo

# --- Confirm ---
if [ "${FORCE:-}" != "1" ]; then
  printf "Proceed? [Y/n] "
  read -r REPLY
  case "$REPLY" in
    [Nn]*) echo "Aborted."; exit 0 ;;
  esac
fi

# --- Download & extract ---
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

echo "Downloading..."
curl -fsSL "$URL" -o "${TMP}/${ARCHIVE}"

echo "Extracting..."
tar -xzf "${TMP}/${ARCHIVE}" -C "$TMP"

# --- Install binaries ---
for BIN in snow-cli snow-cli-ro; do
  SRC=$(find "$TMP" -name "$BIN" -type f | head -1)
  if [ -z "$SRC" ]; then
    echo "  Warning: ${BIN} not found in archive."
    continue
  fi
  cp "$SRC" "${INSTALL_DIR}/${BIN}"
  chmod +x "${INSTALL_DIR}/${BIN}"
  echo "  Installed ${BIN}"
done

# --- Post-install ---
echo
echo "Done."

if ! echo "$PATH" | grep -q "${INSTALL_DIR}"; then
  echo
echo "${INSTALL_DIR} is not on your PATH. Add it with one of:"
  echo "  export PATH=\"${INSTALL_DIR}:\$PATH\"  # add to ~/.bashrc, ~/.zshrc, or ~/.bash_profile"
fi

echo
echo "Verify: ${INSTALL_DIR}/snow-cli --version"
