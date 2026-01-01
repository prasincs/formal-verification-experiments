#!/bin/bash
# Build script for seL4 x86_64 with Rust root server

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

SEL4_DIR="sel4-kit"
BUILD_DIR="build"

echo "Building seL4 x86_64 system..."

# Check if seL4 is set up
if [ ! -d "$SEL4_DIR" ]; then
    echo "seL4 not set up. Run ./scripts/setup.sh first"
    exit 1
fi

# Build the Rust root server
echo "Building Rust root server..."
mkdir -p "$BUILD_DIR"

# For now, we'll build a standalone ELF that can be loaded by seL4
# In a full setup, this would integrate with the seL4 build system

cargo +nightly build \
    --release \
    --target x86_64-unknown-none \
    -Z build-std=core,alloc \
    -Z build-std-features=compiler-builtins-mem \
    2>&1 || {
        echo ""
        echo "Note: Building against real seL4 requires the full seL4 build system."
        echo "For a simpler development experience, use the Microkit project instead:"
        echo "  cd ../microkit-hello"
        echo "  make ARCH=aarch64"
        echo ""
        exit 1
    }

echo ""
echo "Build complete!"
echo ""
echo "To run in QEMU:"
echo "  ./scripts/run.sh"
