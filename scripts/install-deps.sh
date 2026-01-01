#!/bin/bash
# Install system dependencies for CI/development
#
# Usage:
#   ./scripts/install-deps.sh [--microkit]

set -e

INSTALL_MICROKIT=false

for arg in "$@"; do
    case $arg in
        --microkit)
            INSTALL_MICROKIT=true
            ;;
    esac
done

echo "=== Installing System Dependencies ==="

# Detect OS
if [ -f /etc/os-release ]; then
    . /etc/os-release
    OS=$ID
else
    OS=$(uname -s)
fi

case $OS in
    ubuntu|debian)
        sudo apt-get update
        sudo apt-get install -y \
            build-essential \
            curl

        if [ "$INSTALL_MICROKIT" = true ]; then
            sudo apt-get install -y \
                gcc-aarch64-linux-gnu \
                libclang-dev \
                qemu-system-arm
        fi
        ;;
    Darwin)
        if [ "$INSTALL_MICROKIT" = true ]; then
            brew install qemu aarch64-elf-gcc || true
        fi
        ;;
    *)
        echo "Unsupported OS: $OS"
        echo "Please install dependencies manually"
        exit 1
        ;;
esac

echo ""
echo "Dependencies installed!"
