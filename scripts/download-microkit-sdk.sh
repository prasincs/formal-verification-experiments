#!/bin/bash
# Download and install Microkit SDK
#
# Usage:
#   ./scripts/download-microkit-sdk.sh [--version VERSION] [--dest PATH] [--sha256 HASH]
#
# Environment:
#   MICROKIT_SDK_VERSION - SDK version (default: 2.1.0)
#
# The download is verified against the SHA-256 pinned in
# sel4-microkernel/build-system/config/versions.mk when the requested
# version/platform matches that pin (or against --sha256 if given).

set -e

# Defaults
VERSION="${MICROKIT_SDK_VERSION:-2.1.0}"
DEST="/opt/microkit-sdk"
SHA256=""

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSIONS_MK="${REPO_ROOT}/sel4-microkernel/build-system/config/versions.mk"

# Parse arguments
for arg in "$@"; do
    case $arg in
        --version=*)
            VERSION="${arg#*=}"
            ;;
        --dest=*)
            DEST="${arg#*=}"
            ;;
        --sha256=*)
            SHA256="${arg#*=}"
            ;;
    esac
done

echo "=== Downloading Microkit SDK ==="
echo "Version: $VERSION"
echo "Destination: $DEST"
echo ""

# Detect platform
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
    Linux)
        PLATFORM="linux-x86-64"
        ;;
    Darwin)
        if [ "$ARCH" = "arm64" ]; then
            PLATFORM="macos-aarch64"
        else
            PLATFORM="macos-x86-64"
        fi
        ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

URL="https://github.com/seL4/microkit/releases/download/${VERSION}/microkit-sdk-${VERSION}-${PLATFORM}.tar.gz"
echo "URL: $URL"

# The pin in versions.mk is for the linux-x86-64 tarball of the pinned version
if [ -z "$SHA256" ] && [ "$PLATFORM" = "linux-x86-64" ] && [ -f "$VERSIONS_MK" ]; then
    PINNED_VERSION="$(sed -n 's/^MICROKIT_VERSION := //p' "$VERSIONS_MK")"
    if [ "$VERSION" = "$PINNED_VERSION" ]; then
        SHA256="$(sed -n 's/^MICROKIT_SDK_SHA256 := //p' "$VERSIONS_MK")"
    fi
fi

# Create destination
if [ -w "$(dirname "$DEST")" ]; then
    mkdir -p "$DEST"
else
    sudo mkdir -p "$DEST"
fi

# Download, verify, extract
echo "Downloading..."
TARBALL="$(mktemp)"
trap 'rm -f "$TARBALL"' EXIT
curl -L -o "$TARBALL" "$URL"

if [ -n "$SHA256" ]; then
    echo "Verifying SHA-256..."
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$SHA256  $TARBALL" | sha256sum -c -
    else
        echo "$SHA256  $TARBALL" | shasum -a 256 -c -
    fi
else
    echo "WARNING: no pinned SHA-256 for microkit-sdk-${VERSION}-${PLATFORM}; skipping verification" >&2
fi

if [ -w "$DEST" ]; then
    tar -xzf "$TARBALL" -C "$DEST" --strip-components=1
else
    sudo tar -xzf "$TARBALL" -C "$DEST" --strip-components=1
fi

echo ""
echo "Microkit SDK installed to: $DEST"
echo ""
echo "Set environment variable:"
echo "  export MICROKIT_SDK=$DEST"
