#!/bin/bash
# Detect cross-compilation toolchain
#
# Usage:
#   ./detect-toolchain.sh aarch64   # Output: aarch64-linux-gnu- or aarch64-elf-
#   ./detect-toolchain.sh riscv64   # Output: riscv64-linux-gnu- or riscv64-elf-

ARCH="$1"

if [[ -z "$ARCH" ]]; then
    echo "Usage: $0 <aarch64|riscv64>"
    exit 1
fi

UNAME_S=$(uname -s)

case "$ARCH" in
    aarch64)
        if [[ "$UNAME_S" == "Darwin" ]]; then
            PREFIX="aarch64-elf-"
        else
            PREFIX="aarch64-linux-gnu-"
        fi
        ;;
    riscv64)
        if [[ "$UNAME_S" == "Darwin" ]]; then
            PREFIX="riscv64-elf-"
        else
            PREFIX="riscv64-linux-gnu-"
        fi
        ;;
    *)
        echo "Unknown architecture: $ARCH"
        exit 1
        ;;
esac

# Verify toolchain exists
if command -v "${PREFIX}gcc" &>/dev/null; then
    echo "$PREFIX"
else
    echo "Warning: ${PREFIX}gcc not found" >&2
    echo "$PREFIX"
fi
