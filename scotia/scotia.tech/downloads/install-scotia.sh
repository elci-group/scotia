#!/usr/bin/env bash
set -euo pipefail

# Scotia installer for Debian/Ubuntu and compatible Linux distributions.
# Downloads the latest Linux x64 binary and installs it to /usr/local/bin.

INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BINARY_NAME="scotia"
DOWNLOAD_URL="${DOWNLOAD_URL:-https://scotia.tech/downloads/scotia-linux-x64}"

REPO_URL="https://github.com/scotia/scotia"

echo "=== Scotia installer ==="
echo "Repository: $REPO_URL"
echo "Install dir: $INSTALL_DIR"
echo

# Detect platform
if [[ "$(uname -s)" != "Linux" ]]; then
    echo "Error: This installer only supports Linux." >&2
    exit 1
fi

if [[ "$(uname -m)" != "x86_64" ]]; then
    echo "Error: This installer only supports x86_64." >&2
    exit 1
fi

# Ensure install directory exists and is writable
if [[ ! -d "$INSTALL_DIR" ]]; then
    echo "Creating $INSTALL_DIR..."
    mkdir -p "$INSTALL_DIR"
fi

if [[ ! -w "$INSTALL_DIR" ]]; then
    echo "Error: $INSTALL_DIR is not writable. Try running with sudo." >&2
    exit 1
fi

# Download binary
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading Scotia binary..."
if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$DOWNLOAD_URL" -o "$TMP_DIR/$BINARY_NAME"
elif command -v wget >/dev/null 2>&1; then
    wget -q "$DOWNLOAD_URL" -O "$TMP_DIR/$BINARY_NAME"
else
    echo "Error: curl or wget is required." >&2
    exit 1
fi

chmod +x "$TMP_DIR/$BINARY_NAME"

# Verify binary executes
echo "Verifying binary..."
if ! "$TMP_DIR/$BINARY_NAME" --version >/dev/null 2>&1; then
    echo "Error: Downloaded binary does not execute." >&2
    exit 1
fi

# Install
echo "Installing $BINARY_NAME to $INSTALL_DIR..."
mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"

echo
echo "Scotia installed successfully!"
echo "Run 'scotia --help' to get started."
echo "Example: scotia run --agent claude-code -- claude"
