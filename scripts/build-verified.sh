#!/bin/bash
# Build the seL4 verified components library
#
# Usage:
#   ./scripts/build-verified.sh [--release] [--test]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT/sel4-microkernel/verified"

# Parse arguments
RELEASE_FLAG=""
RUN_TESTS=false

for arg in "$@"; do
    case $arg in
        --release)
            RELEASE_FLAG="--release"
            ;;
        --test)
            RUN_TESTS=true
            ;;
    esac
done

echo "=== Building Verified Components ==="
cargo build $RELEASE_FLAG --verbose

if [ "$RUN_TESTS" = true ]; then
    echo ""
    echo "=== Running Tests ==="
    cargo test $RELEASE_FLAG --verbose
fi

echo ""
echo "Verified components build complete!"
