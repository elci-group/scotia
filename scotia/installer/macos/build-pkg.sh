#!/usr/bin/env bash
# Build a macOS installer package (.pkg) and wrap it in a DMG for Scotia.
# Run from the repository root:
#   ./installer/macos/build-pkg.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${SCRIPT_DIR}/build"
VERSION=$(cat "${SRC_DIR}/VERSION")

echo "Building Scotia macOS installer v${VERSION}..."

rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}/payload/usr/local/scotia/bin"

# Copy release binaries from the Rust build.
cp "${SRC_DIR}/target/release/scotia"      "${BUILD_DIR}/payload/usr/local/scotia/bin/scotia"
cp "${SRC_DIR}/target/release/scotiad"     "${BUILD_DIR}/payload/usr/local/scotia/bin/scotiad"
cp "${SRC_DIR}/target/release/scotia-shim" "${BUILD_DIR}/payload/usr/local/scotia/bin/scotia-shim"
chmod +x "${BUILD_DIR}/payload/usr/local/scotia/bin/"*

# Build the component package.
pkgbuild \
    --identifier com.scotia.scotia \
    --version "${VERSION}" \
    --root "${BUILD_DIR}/payload" \
    --install-location / \
    --scripts "${SCRIPT_DIR}/scripts" \
    "${BUILD_DIR}/scotia.pkg"

# Assemble the product archive.
cp "${SRC_DIR}/LICENSE" "${BUILD_DIR}/LICENSE"
cp "${SCRIPT_DIR}/../linux/calamares/branding/scotia/logo.png" "${BUILD_DIR}/logo.png"

productbuild \
    --distribution "${SCRIPT_DIR}/Distribution.xml" \
    --package-path "${BUILD_DIR}" \
    --resources "${BUILD_DIR}" \
    "${BUILD_DIR}/Scotia-${VERSION}.pkg"

# Create a DMG containing the installer package.
mkdir -p "${BUILD_DIR}/dmg"
cp "${BUILD_DIR}/Scotia-${VERSION}.pkg" "${BUILD_DIR}/dmg/"
cp "${SRC_DIR}/LICENSE" "${BUILD_DIR}/dmg/LICENSE.txt"

hdiutil create \
    -volname "Scotia ${VERSION}" \
    -srcfolder "${BUILD_DIR}/dmg" \
    -ov \
    -format UDZO \
    "${BUILD_DIR}/Scotia-${VERSION}.dmg"

echo "Installer ready:"
echo "  ${BUILD_DIR}/Scotia-${VERSION}.pkg"
echo "  ${BUILD_DIR}/Scotia-${VERSION}.dmg"
