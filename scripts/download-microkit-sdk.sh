#!/bin/bash
# Download and install Microkit SDK
#
# Usage:
#   ./scripts/download-microkit-sdk.sh [--version VERSION] [--dest PATH]
#
# Environment:
#   MICROKIT_SDK_VERSION - SDK version (default: 2.1.0)

set -e

# Defaults
VERSION="${MICROKIT_SDK_VERSION:-2.1.0}"
DEST="/opt/microkit-sdk"

# Parse arguments
for arg in "$@"; do
    case $arg in
        --version=*)
            VERSION="${arg#*=}"
            ;;
        --dest=*)
            DEST="${arg#*=}"
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

# Create destination
if [ -w "$(dirname "$DEST")" ]; then
    mkdir -p "$DEST"
else
    sudo mkdir -p "$DEST"
fi

# Download and extract
echo "Downloading..."
if [ -w "$DEST" ]; then
    curl -L "$URL" | tar -xzf - -C "$DEST" --strip-components=1
else
    curl -L "$URL" | sudo tar -xzf - -C "$DEST" --strip-components=1
fi

echo ""
echo "Microkit SDK installed to: $DEST"
echo ""
echo "Set environment variable:"
echo "  export MICROKIT_SDK=$DEST"
