#!/usr/bin/env bash
#
# Build a .deb package from a pre-built sshm binary.
#
# Usage:
#   ./scripts/build-deb.sh <binary-path> <architecture> <version>
#
# Example:
#   ./scripts/build-deb.sh target/x86_64-unknown-linux-gnu/release/sshm amd64 1.0.4
#   ./scripts/build-deb.sh target/aarch64-unknown-linux-gnu/release/sshm arm64 1.0.4
#
set -euo pipefail

BINARY="$1"
ARCH="$2"
VERSION="$3"

PKG_NAME="sshm"
DEB_NAME="${PKG_NAME}_${VERSION}_${ARCH}"
STAGING="/tmp/${DEB_NAME}"

echo "==> Building ${DEB_NAME}.deb"

# Clean previous build
rm -rf "${STAGING}"

# Create directory structure
mkdir -p "${STAGING}/DEBIAN"
mkdir -p "${STAGING}/usr/bin"
mkdir -p "${STAGING}/usr/share/doc/${PKG_NAME}"

# Install binary
install -m 755 "${BINARY}" "${STAGING}/usr/bin/${PKG_NAME}"

# Generate DEBIAN/control from template
sed \
  -e "s/\${VERSION}/${VERSION}/g" \
  -e "s/\${ARCH}/${ARCH}/g" \
  debian/control > "${STAGING}/DEBIAN/control"

# Compute installed size (in KB)
SIZE=$(du -sk "${STAGING}/usr" | cut -f1)
echo "Installed-Size: ${SIZE}" >> "${STAGING}/DEBIAN/control"

# Copy documentation
cp debian/copyright "${STAGING}/usr/share/doc/${PKG_NAME}/copyright"
cp debian/changelog "${STAGING}/usr/share/doc/${PKG_NAME}/changelog.Debian"
gzip -9 -n "${STAGING}/usr/share/doc/${PKG_NAME}/changelog.Debian"

# Set correct ownership (everything root:root)
# Note: in CI this runs as root, locally fakeroot may be needed
if [ "$(id -u)" = "0" ]; then
  chown -R root:root "${STAGING}"
fi

# Build .deb
dpkg-deb --root-owner-group --build "${STAGING}" "${DEB_NAME}.deb"

echo "==> Created ${DEB_NAME}.deb ($(du -h "${DEB_NAME}.deb" | cut -f1))"

# Cleanup
rm -rf "${STAGING}"
