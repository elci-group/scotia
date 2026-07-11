#!/usr/bin/env bash
# Scotia installer for Linux x86_64.
#
# Downloads the Scotia binary and a SHA-256 manifest, verifies the binary
# against the manifest BEFORE executing it, and installs it into a
# user-writable directory (default: ~/.local/bin). Never requires sudo.
#
# Environment overrides:
#   SCOTIA_VERSION   release tag to install (default: latest)
#   SCOTIA_BASE_URL  download base URL  (default: https://scotia.tech/downloads)
#   INSTALL_DIR      install directory  (default: ~/.local/bin)
set -euo pipefail

SCOTIA_VERSION="${SCOTIA_VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BASE_URL="${SCOTIA_BASE_URL:-https://scotia.tech/downloads}"
BINARY_NAME="scotia"
ASSET="scotia-linux-x64"
CHECKSUM_FILE="SHA256SUMS"

REPO_URL="https://github.com/elci-group/scotia"

# Pinned minisign public key used to authenticate SHA256SUMS. Release builds
# MUST replace this placeholder with the real release signing public key (the
# matching secret key signs SHA256SUMS in the release-sign workflow). Keeping
# the key inline avoids any trust-on-first-use keyring step.
MINISIGN_PUBKEY="REPLACE_ME_WITH_RELEASE_MINISIGN_PUBLIC_KEY"

# --- Argument parsing ------------------------------------------------------
ALLOW_UNSIGNED=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --insecure-allow-unsigned)
            ALLOW_UNSIGNED=1
            shift
            ;;
        -h|--help)
            cat <<'EOF'
Usage: install-scotia.sh [OPTIONS]

Options:
  --insecure-allow-unsigned   Proceed on checksum only if no valid signature is
                              available. NOT RECOMMENDED — disables authenticity
                              verification and leaves you exposed to tampered
                              downloads. Intended only for local development.
  -h, --help                  Show this help and exit.

Environment overrides:
  SCOTIA_VERSION   release tag to install (default: latest)
  SCOTIA_BASE_URL  download base URL  (default: https://scotia.tech/downloads)
  INSTALL_DIR      install directory  (default: ~/.local/bin)
EOF
            exit 0
            ;;
        *)
            echo "Error: unknown option: $1" >&2
            exit 2
            ;;
    esac
done

echo "=== Scotia installer ==="
echo "Repository:  $REPO_URL"
echo "Version:     $SCOTIA_VERSION"
echo "Install dir: $INSTALL_DIR"
echo

# --- Platform checks -------------------------------------------------------
if [[ "$(uname -s)" != "Linux" ]]; then
    echo "Error: this installer only supports Linux." >&2
    exit 1
fi
if [[ "$(uname -m)" != "x86_64" ]]; then
    echo "Error: this installer only supports x86_64." >&2
    exit 1
fi

# --- Install dir -----------------------------------------------------------
mkdir -p "$INSTALL_DIR"
if [[ ! -w "$INSTALL_DIR" ]]; then
    echo "Error: $INSTALL_DIR is not writable." >&2
    echo "Choose a user-writable INSTALL_DIR (e.g. ~/.local/bin). This installer never needs sudo." >&2
    exit 1
fi

# --- Download helpers (HTTPS only) ----------------------------------------
download() {
    local url="$1" out="$2"
    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -fsSL "$url" -o "$out"
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only -q "$url" -O "$out"
    else
        echo "Error: curl or wget is required." >&2
        exit 1
    fi
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        echo "Error: sha256sum or shasum is required to verify the download." >&2
        exit 1
    fi
}

asset_url() {
    if [[ "$SCOTIA_VERSION" == "latest" ]]; then
        printf '%s/%s' "$BASE_URL" "$1"
    else
        printf '%s/%s/%s' "$BASE_URL" "$SCOTIA_VERSION" "$1"
    fi
}

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# --- Download binary + checksum manifest ----------------------------------
echo "Downloading $ASSET ..."
download "$(asset_url "$ASSET")"        "$TMP_DIR/$ASSET"
download "$(asset_url "$CHECKSUM_FILE")" "$TMP_DIR/$CHECKSUM_FILE"

# --- Verify SHA-256 BEFORE executing anything ------------------------------
expected="$(grep -E "[[:space:]]${ASSET}$" "$TMP_DIR/$CHECKSUM_FILE" | awk '{print $1}' || true)"
if [[ -z "$expected" ]]; then
    echo "Error: no checksum for $ASSET found in $CHECKSUM_FILE; refusing to install." >&2
    exit 1
fi
actual="$(sha256 "$TMP_DIR/$ASSET")"
if [[ "${expected,,}" != "${actual,,}" ]]; then
    echo "Error: checksum mismatch for $ASSET." >&2
    echo "  expected: $expected" >&2
    echo "    actual: $actual" >&2
    echo "The download may be corrupt or tampered with; aborting." >&2
    exit 1
fi
echo "Checksum verified ($actual)."

# --- Authenticate the manifest (REQUIRED by default) -----------------------
# The checksum above proves integrity; a detached signature over the manifest
# proves authenticity. We prefer minisign (verified against the pinned
# MINISIGN_PUBKEY, so no keyring setup or trust-on-first-use is needed) and fall
# back to a gpg detached signature verified against the local keyring. Refusing
# to install an unauthenticated binary is the safe default; local development
# can opt out with --insecure-allow-unsigned.
verify_signature() {
    # 1) minisign against the pinned public key.
    if command -v minisign >/dev/null 2>&1 \
        && [[ "$MINISIGN_PUBKEY" != REPLACE_ME_* ]] \
        && download "$(asset_url "${CHECKSUM_FILE}.minisig")" "$TMP_DIR/${CHECKSUM_FILE}.minisig" 2>/dev/null; then
        if minisign -Vm "$TMP_DIR/$CHECKSUM_FILE" -x "$TMP_DIR/${CHECKSUM_FILE}.minisig" -P "$MINISIGN_PUBKEY" >/dev/null 2>&1; then
            echo "minisign signature over $CHECKSUM_FILE verified (pinned key)."
            return 0
        fi
        echo "Error: ${CHECKSUM_FILE}.minisig present but minisign verification failed; aborting." >&2
        return 1
    fi

    # 2) gpg detached signature against the local keyring.
    if command -v gpg >/dev/null 2>&1 \
        && download "$(asset_url "${CHECKSUM_FILE}.sig")" "$TMP_DIR/${CHECKSUM_FILE}.sig" 2>/dev/null; then
        if gpg --verify "$TMP_DIR/${CHECKSUM_FILE}.sig" "$TMP_DIR/$CHECKSUM_FILE" 2>/dev/null; then
            echo "gpg signature over $CHECKSUM_FILE verified."
            return 0
        fi
        echo "Error: ${CHECKSUM_FILE}.sig present but gpg verification failed; aborting." >&2
        return 1
    fi

    return 2  # no usable signature path is available
}

if verify_signature; then
    :
else
    if [[ "$ALLOW_UNSIGNED" == "1" ]]; then
        echo "WARNING: no valid signature for $CHECKSUM_FILE; proceeding on checksum only" >&2
        echo "         because --insecure-allow-unsigned was passed. This is unsafe." >&2
    else
        echo "Error: could not authenticate $CHECKSUM_FILE (no usable minisign/gpg signature)." >&2
        echo "Refusing to install an unauthenticated binary." >&2
        echo "To bypass for local development (NOT RECOMMENDED), re-run with --insecure-allow-unsigned." >&2
        exit 1
    fi
fi

# --- Install (now that the binary is authenticated) ------------------------
chmod +x "$TMP_DIR/$ASSET"
echo "Verifying binary..."
if ! "$TMP_DIR/$ASSET" --version >/dev/null 2>&1; then
    echo "Error: downloaded binary does not execute." >&2
    exit 1
fi

echo "Installing $BINARY_NAME to $INSTALL_DIR..."
mv "$TMP_DIR/$ASSET" "$INSTALL_DIR/$BINARY_NAME"

echo
echo "Scotia installed successfully!"
echo "Ensure $INSTALL_DIR is on your PATH, then run 'scotia --help'."
echo "Example: scotia run --agent claude-code -- claude"
