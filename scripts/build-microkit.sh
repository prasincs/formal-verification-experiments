#!/bin/bash
# Build seL4 Microkit system for AArch64
#
# Usage:
#   ./scripts/build-microkit.sh [--sdk-path PATH]
#
# Environment:
#   MICROKIT_SDK - Path to Microkit SDK (default: /opt/microkit-sdk or local)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Parse arguments
for arg in "$@"; do
    case $arg in
        --sdk-path=*)
            MICROKIT_SDK="${arg#*=}"
            ;;
        --sdk-path)
            shift
            MICROKIT_SDK="$1"
            ;;
    esac
done

# Find Microkit SDK
if [ -z "$MICROKIT_SDK" ]; then
    if [ -d "/opt/microkit-sdk" ]; then
        MICROKIT_SDK="/opt/microkit-sdk"
    elif [ -d "$REPO_ROOT/sel4-microkernel/microkit-hello/microkit-sdk" ]; then
        MICROKIT_SDK="$REPO_ROOT/sel4-microkernel/microkit-hello/microkit-sdk"
    else
        echo "Error: Microkit SDK not found"
        echo "Set MICROKIT_SDK environment variable or use --sdk-path"
        exit 1
    fi
fi

export MICROKIT_SDK
export SEL4_INCLUDE_DIRS="$MICROKIT_SDK/board/qemu_virt_aarch64/debug/include"

echo "=== Building Microkit System ==="
echo "SDK: $MICROKIT_SDK"
echo ""

cd "$REPO_ROOT/sel4-microkernel/microkit-hello"

# Build the Rust protection domain
echo "Building Rust protection domain..."
cargo +nightly build \
    --release \
    --target aarch64-sel4-microkit \
    -Z build-std=core,alloc \
    -Z build-std-features=compiler-builtins-mem

# Create output directory and copy ELF
mkdir -p build/aarch64
cp target/aarch64-sel4-microkit/release/hello.elf build/aarch64/

# Build system image with Microkit tool
echo ""
echo "Building system image..."
"$MICROKIT_SDK/bin/microkit" \
    hello.system \
    --search-path build/aarch64 \
    --board qemu_virt_aarch64 \
    --config debug \
    -o build/aarch64/loader.img \
    -r build/aarch64/report.txt

echo ""
echo "Build complete!"
echo "  System image: build/aarch64/loader.img"
echo "  ELF: build/aarch64/hello.elf"
echo "  Report: build/aarch64/report.txt"
