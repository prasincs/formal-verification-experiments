#!/bin/bash
# Run the seL4 Microkit system in QEMU
#
# Usage:
#   ./run-qemu.sh aarch64    # Run AArch64 build
#   ./run-qemu.sh riscv64    # Run RISC-V build

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

ARCH=${1:-aarch64}

case $ARCH in
    aarch64)
        IMAGE="build/aarch64/loader.img"
        if [ ! -f "$IMAGE" ]; then
            echo "Build not found. Running: make ARCH=aarch64"
            make ARCH=aarch64
        fi
        echo ""
        echo "Booting seL4 Microkit (AArch64) in QEMU..."
        echo "Press Ctrl-A X to exit"
        echo ""
        qemu-system-aarch64 \
            -machine virt,virtualization=on \
            -cpu cortex-a53 \
            -m 2G \
            -nographic \
            -device loader,file=$IMAGE,addr=0x70000000,cpu-num=0
        ;;
    riscv64)
        IMAGE="build/riscv64/loader.img"
        if [ ! -f "$IMAGE" ]; then
            echo "Build not found. Running: make ARCH=riscv64"
            make ARCH=riscv64
        fi
        echo ""
        echo "Booting seL4 Microkit (RISC-V 64) in QEMU..."
        echo "Press Ctrl-A X to exit"
        echo ""
        qemu-system-riscv64 \
            -machine virt \
            -cpu rv64 \
            -m 2G \
            -nographic \
            -bios default \
            -kernel $IMAGE
        ;;
    *)
        echo "Usage: $0 [aarch64|riscv64]"
        exit 1
        ;;
esac
