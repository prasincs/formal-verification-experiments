#!/bin/bash
# Build U-Boot for Raspberry Pi 4
#
# Usage:
#   ./build-uboot.sh --source DIR --output FILE --cross-compile PREFIX --version VER

set -e

SOURCE=""
OUTPUT=""
CROSS_COMPILE=""
VERSION=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --source)
            SOURCE="$2"
            shift 2
            ;;
        --output)
            OUTPUT="$2"
            shift 2
            ;;
        --cross-compile)
            CROSS_COMPILE="$2"
            shift 2
            ;;
        --version)
            VERSION="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 --source DIR --output FILE --cross-compile PREFIX --version VER"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [[ -z "$SOURCE" ]] || [[ -z "$OUTPUT" ]] || [[ -z "$CROSS_COMPILE" ]]; then
    echo "Error: --source, --output, and --cross-compile are required"
    exit 1
fi

# Check if submodule is initialized
if [[ ! -d "$SOURCE/.git" ]]; then
    echo "Initializing U-Boot submodule..."
    git submodule update --init "$SOURCE"
fi

# Checkout specific version if provided
if [[ -n "$VERSION" ]]; then
    echo "Checking out $VERSION..."
    cd "$SOURCE" && git checkout "$VERSION"
fi

echo "=== Building U-Boot for Raspberry Pi 4 ==="
echo "Source: $SOURCE"
echo "Cross-compile: $CROSS_COMPILE"
echo ""

# Detect number of CPUs
if [[ "$(uname -s)" == "Darwin" ]]; then
    NPROC=$(sysctl -n hw.ncpu)
else
    NPROC=$(nproc)
fi

# Configure and build
make -C "$SOURCE" CROSS_COMPILE="$CROSS_COMPILE" rpi_4_defconfig
make -C "$SOURCE" CROSS_COMPILE="$CROSS_COMPILE" -j"$NPROC"

# Copy output
mkdir -p "$(dirname "$OUTPUT")"
cp "$SOURCE/u-boot.bin" "$OUTPUT"

echo ""
echo "U-Boot built: $OUTPUT"
