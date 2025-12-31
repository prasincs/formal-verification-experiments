#!/bin/bash
# Run seL4 x86_64 in QEMU

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

SEL4_DIR="sel4-kit"

# Check if seL4 is built
if [ ! -f "$SEL4_DIR/build-x86_64/images/sel4test-driver-image-x86_64-pc99" ]; then
    echo "seL4 image not found. Building..."
    cd "$SEL4_DIR/build-x86_64"
    ninja
    cd "$PROJECT_DIR"
fi

IMAGE="$SEL4_DIR/build-x86_64/images/sel4test-driver-image-x86_64-pc99"

echo "Booting seL4 x86_64 in QEMU..."
echo "Press Ctrl-A X to exit"
echo ""

qemu-system-x86_64 \
    -cpu Nehalem,-vme,+pdpe1gb,-xsave,-xsaveopt,-xsavec,-fsgsbase,-invpcid,enforce \
    -m 512M \
    -nographic \
    -serial mon:stdio \
    -kernel "$IMAGE"
