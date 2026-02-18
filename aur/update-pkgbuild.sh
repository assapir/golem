#!/usr/bin/env bash
# Updates PKGBUILD with a new version and checksums from GitHub release.
# Usage: ./update-pkgbuild.sh <version>
# Example: ./update-pkgbuild.sh 0.2.0
set -euo pipefail

VERSION="${1:?Usage: $0 <version>}"
REPO="assapir/golem"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PKGBUILD="${SCRIPT_DIR}/PKGBUILD"

echo "Updating PKGBUILD to version ${VERSION}..."

# Download checksums from the release
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

for arch in x86_64 aarch64; do
    url="https://github.com/${REPO}/releases/download/v${VERSION}/golem-${arch}-linux.sha256"
    if ! curl -sfL "$url" -o "${TMPDIR}/${arch}.sha256"; then
        echo "ERROR: Failed to download checksum for ${arch}" >&2
        exit 1
    fi
done

SHA_X86=$(awk '{print $1}' "${TMPDIR}/x86_64.sha256")
SHA_AARCH64=$(awk '{print $1}' "${TMPDIR}/aarch64.sha256")

echo "  x86_64:  ${SHA_X86}"
echo "  aarch64: ${SHA_AARCH64}"

# Update PKGBUILD
sed -i "s/^pkgver=.*/pkgver=${VERSION}/" "$PKGBUILD"
sed -i "s/^pkgrel=.*/pkgrel=1/" "$PKGBUILD"
sed -i "s/^sha256sums_x86_64=.*/sha256sums_x86_64=('${SHA_X86}')/" "$PKGBUILD"
sed -i "s/^sha256sums_aarch64=.*/sha256sums_aarch64=('${SHA_AARCH64}')/" "$PKGBUILD"

echo "PKGBUILD updated to ${VERSION}"
