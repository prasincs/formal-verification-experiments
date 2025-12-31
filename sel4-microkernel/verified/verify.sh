#!/bin/bash
# Verify the library with Verus

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Check if we're in a container with Verus
if command -v verus &> /dev/null; then
    echo "Running Verus verification..."
    verus --crate-type lib src/lib.rs
elif [ -f "../../../verus/run.sh" ]; then
    echo "Using Verus container from verus/ directory..."
    cd ../../../verus
    ./run.sh shell -c "cd /work/sel4-microkernel/verified && verus --crate-type lib src/lib.rs"
else
    echo "Verus not found. Options:"
    echo "  1. Run from verus container: cd ../../../verus && ./run.sh shell"
    echo "  2. Install Verus locally: https://github.com/verus-lang/verus"
    echo ""
    echo "Running cargo test instead..."
    cargo test
fi
