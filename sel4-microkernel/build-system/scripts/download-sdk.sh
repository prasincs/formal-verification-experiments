#!/bin/bash
# Download and set up Microkit SDK
#
# Usage:
#   ./download-sdk.sh --version VER --sha256 SHA --output DIR

set -e

VERSION=""
SHA256=""
OUTPUT=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --version)
            VERSION="$2"
            shift 2
            ;;
        --sha256)
            SHA256="$2"
            shift 2
            ;;
        --output)
            OUTPUT="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 --version VER --sha256 SHA --output DIR"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [[ -z "$VERSION" ]] || [[ -z "$OUTPUT" ]]; then
    echo "Error: --version and --output are required"
    exit 1
fi

# Detect OS
UNAME_S=$(uname -s)
if [[ "$UNAME_S" == "Darwin" ]]; then
    PLATFORM="macos-x86-64"
else
    PLATFORM="linux-x86-64"
fi

URL="https://github.com/seL4/microkit/releases/download/${VERSION}/microkit-sdk-${VERSION}-${PLATFORM}.tar.gz"
TMP_FILE="/tmp/microkit-sdk-${VERSION}.tar.gz"

echo "=== Downloading Microkit SDK ${VERSION} ==="
echo "Platform: ${PLATFORM}"
echo "URL: ${URL}"
echo ""

curl -L -o "$TMP_FILE" "$URL"

# Verify checksum if provided
if [[ -n "$SHA256" ]]; then
    echo "Verifying SHA256 checksum..."
    if [[ "$UNAME_S" == "Darwin" ]]; then
        ACTUAL_SHA=$(shasum -a 256 "$TMP_FILE" | awk '{print $1}')
    else
        ACTUAL_SHA=$(sha256sum "$TMP_FILE" | awk '{print $1}')
    fi

    if [[ "$ACTUAL_SHA" != "$SHA256" ]]; then
        echo "Error: Checksum mismatch!"
        echo "Expected: $SHA256"
        echo "Actual:   $ACTUAL_SHA"
        rm -f "$TMP_FILE"
        exit 1
    fi
    echo "Checksum verified."
fi

# Extract
echo "Extracting to $OUTPUT..."
mkdir -p "$OUTPUT"
tar -xzf "$TMP_FILE" -C "$OUTPUT" --strip-components=1

rm "$TMP_FILE"

echo ""
echo "SDK installed at: $OUTPUT"
