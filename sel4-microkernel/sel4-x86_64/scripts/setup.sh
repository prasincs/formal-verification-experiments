#!/bin/bash
# Setup script for seL4 x86_64 development
#
# This script sets up the seL4 build environment using the official
# seL4 CMake build system with rust-sel4 support.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

SEL4_DIR="sel4-kit"

echo "============================================"
echo "  seL4 x86_64 Development Setup"
echo "============================================"
echo ""

# Check prerequisites
check_prerequisites() {
    echo "Checking prerequisites..."

    local missing=()

    # Build tools
    for cmd in git cmake ninja python3 pip3; do
        if ! command -v $cmd &> /dev/null; then
            missing+=("$cmd")
        fi
    done

    # QEMU for testing
    if ! command -v qemu-system-x86_64 &> /dev/null; then
        missing+=("qemu-system-x86_64")
    fi

    # Rust
    if ! command -v rustup &> /dev/null; then
        missing+=("rustup")
    fi

    if [ ${#missing[@]} -gt 0 ]; then
        echo ""
        echo "Missing prerequisites:"
        for pkg in "${missing[@]}"; do
            echo "  - $pkg"
        done
        echo ""
        echo "Install with:"
        echo "  sudo apt install git cmake ninja-build python3 python3-pip qemu-system-x86"
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        echo ""
        exit 1
    fi

    echo "All prerequisites satisfied!"
}

# Set up seL4 build environment
setup_sel4() {
    if [ -d "$SEL4_DIR" ]; then
        echo "seL4 kit already exists at $SEL4_DIR"
        return
    fi

    echo ""
    echo "Setting up seL4 build environment..."
    echo ""

    mkdir -p "$SEL4_DIR"
    cd "$SEL4_DIR"

    # Initialize repo
    if ! command -v repo &> /dev/null; then
        echo "Installing Google's repo tool..."
        mkdir -p ~/.local/bin
        curl -s https://storage.googleapis.com/git-repo-downloads/repo > ~/.local/bin/repo
        chmod a+x ~/.local/bin/repo
        export PATH="$HOME/.local/bin:$PATH"
    fi

    echo "Fetching seL4 sources (this may take a while)..."

    # Initialize with sel4test manifest (includes everything we need)
    repo init -u https://github.com/seL4/sel4test-manifest.git

    # Sync repositories
    repo sync -j4

    # Set up Python dependencies
    pip3 install --user -r projects/seL4_tools/cmake-tool/requirements.txt 2>/dev/null || \
        pip3 install --user camkes-deps

    cd "$PROJECT_DIR"
    echo "seL4 sources fetched successfully!"
}

# Build seL4 for x86_64
build_sel4() {
    echo ""
    echo "Building seL4 for x86_64..."
    echo ""

    cd "$SEL4_DIR"

    # Create build directory
    mkdir -p build-x86_64
    cd build-x86_64

    # Configure for x86_64 simulation
    ../init-build.sh -DPLATFORM=x86_64 -DSIMULATION=TRUE

    # Build
    ninja

    cd "$PROJECT_DIR"
    echo "seL4 built successfully!"
}

# Set up rust-sel4
setup_rust_sel4() {
    echo ""
    echo "Setting up rust-sel4..."
    echo ""

    # Ensure nightly Rust with required components
    rustup install nightly
    rustup component add rust-src --toolchain nightly

    # The rust-sel4 crates are fetched via Cargo.toml git dependencies
    echo "rust-sel4 will be fetched during cargo build"
}

# Main
main() {
    check_prerequisites
    setup_sel4
    setup_rust_sel4

    echo ""
    echo "============================================"
    echo "  Setup Complete!"
    echo "============================================"
    echo ""
    echo "To build and run:"
    echo "  ./scripts/build.sh    # Build the system"
    echo "  ./scripts/run.sh      # Boot in QEMU"
    echo ""
    echo "Note: Full seL4 x86_64 builds require the complete"
    echo "seL4 build system. For simpler development, consider"
    echo "using Microkit with AArch64 or RISC-V instead."
    echo ""
}

main "$@"
