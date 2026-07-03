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

echo "=== Building Microkit System ==="
echo "SDK: $MICROKIT_SDK"
echo ""

cd "$REPO_ROOT/sel4-microkernel/microkit-hello"

# Delegate to the Makefile so there is exactly one build path. It handles
# SEL4_INCLUDE_DIRS, the .json target spec, and the nightly target-spec
# opt-in flags (which change across nightlies), then runs the Microkit tool.
make ARCH=aarch64 MICROKIT_SDK="$MICROKIT_SDK"
