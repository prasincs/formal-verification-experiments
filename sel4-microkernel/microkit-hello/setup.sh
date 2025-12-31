#!/bin/bash
# Setup script for seL4 Microkit development
#
# Downloads the Microkit SDK and sets up the build environment

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Microkit SDK version and download URL
# Check https://github.com/seL4/microkit/releases for latest
MICROKIT_VERSION="1.4.1"
MICROKIT_SDK_DIR="microkit-sdk"

echo "==================================="
echo "  seL4 Microkit Setup"
echo "==================================="
echo ""

# Function to download Microkit SDK
download_microkit_sdk() {
    local os=$(uname -s | tr '[:upper:]' '[:lower:]')

    echo "Downloading Microkit SDK v${MICROKIT_VERSION}..."

    # The SDK is distributed as a tarball from GitHub releases
    local sdk_url="https://github.com/seL4/microkit/releases/download/${MICROKIT_VERSION}/microkit-sdk-${MICROKIT_VERSION}-${os}-x86_64.tar.gz"

    if command -v wget &> /dev/null; then
        wget -q --show-progress -O microkit-sdk.tar.gz "$sdk_url" || {
            echo "Download failed. Trying alternative method..."
            download_microkit_from_source
            return
        }
    elif command -v curl &> /dev/null; then
        curl -L --progress-bar -o microkit-sdk.tar.gz "$sdk_url" || {
            echo "Download failed. Trying alternative method..."
            download_microkit_from_source
            return
        }
    else
        echo "Neither wget nor curl found. Installing wget..."
        sudo apt-get update && sudo apt-get install -y wget
        wget -q --show-progress -O microkit-sdk.tar.gz "$sdk_url"
    fi

    echo "Extracting SDK..."
    mkdir -p "$MICROKIT_SDK_DIR"
    tar -xzf microkit-sdk.tar.gz -C "$MICROKIT_SDK_DIR" --strip-components=1
    rm microkit-sdk.tar.gz

    echo "SDK extracted to $MICROKIT_SDK_DIR"
}

# Alternative: build from source
download_microkit_from_source() {
    echo ""
    echo "Building Microkit SDK from source..."
    echo "This may take a while..."
    echo ""

    if [ ! -d "microkit-src" ]; then
        git clone https://github.com/seL4/microkit.git microkit-src
    fi

    cd microkit-src
    git checkout "$MICROKIT_VERSION" 2>/dev/null || git checkout main

    # Build for both architectures
    python3 -m pip install --user -r requirements.txt

    # Build SDK
    python3 build_sdk.py --sel4 seL4

    # Copy built SDK
    cp -r release/microkit-sdk-* ../"$MICROKIT_SDK_DIR"
    cd ..

    echo "SDK built and installed to $MICROKIT_SDK_DIR"
}

# Check prerequisites
check_prerequisites() {
    echo "Checking prerequisites..."

    local missing=()

    # Check for Rust
    if ! command -v rustup &> /dev/null; then
        missing+=("rustup (Rust toolchain)")
    fi

    # Check for required Rust targets
    if command -v rustup &> /dev/null; then
        if ! rustup target list --installed | grep -q "aarch64-unknown-none"; then
            echo "Adding aarch64-unknown-none target..."
            rustup target add aarch64-unknown-none --toolchain nightly
        fi
        if ! rustup target list --installed | grep -q "riscv64gc-unknown-none-elf"; then
            echo "Adding riscv64gc-unknown-none-elf target..."
            rustup target add riscv64gc-unknown-none-elf --toolchain nightly
        fi
    fi

    # Check for QEMU
    if ! command -v qemu-system-aarch64 &> /dev/null; then
        missing+=("qemu-system-aarch64")
    fi

    # Check for cross-compilers
    if ! command -v aarch64-linux-gnu-ld &> /dev/null; then
        missing+=("aarch64-linux-gnu-gcc (cross compiler)")
    fi

    if [ ${#missing[@]} -gt 0 ]; then
        echo ""
        echo "Missing prerequisites:"
        for pkg in "${missing[@]}"; do
            echo "  - $pkg"
        done
        echo ""
        echo "Install with:"
        echo "  sudo apt install qemu-system-arm qemu-system-misc gcc-aarch64-linux-gnu gcc-riscv64-linux-gnu"
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        echo ""
    else
        echo "All prerequisites satisfied!"
    fi
}

# Main setup
main() {
    check_prerequisites

    if [ -d "$MICROKIT_SDK_DIR" ]; then
        echo ""
        echo "Microkit SDK already exists at $MICROKIT_SDK_DIR"
        read -p "Re-download? [y/N] " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            echo "Using existing SDK."
            echo ""
            echo "Setup complete! Build with:"
            echo "  make ARCH=aarch64"
            echo "  make ARCH=riscv64"
            return 0
        fi
        rm -rf "$MICROKIT_SDK_DIR"
    fi

    download_microkit_sdk

    echo ""
    echo "==================================="
    echo "  Setup Complete!"
    echo "==================================="
    echo ""
    echo "Build and run:"
    echo "  make ARCH=aarch64       # Build for AArch64"
    echo "  make run ARCH=aarch64   # Run in QEMU"
    echo ""
    echo "  make ARCH=riscv64       # Build for RISC-V"
    echo "  make run ARCH=riscv64   # Run in QEMU"
    echo ""
}

main "$@"
