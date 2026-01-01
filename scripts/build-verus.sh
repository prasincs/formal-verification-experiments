#!/bin/bash
# Build the Verus demo library
#
# Usage:
#   ./scripts/build-verus.sh [--release] [--test]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT/verus"

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

echo "=== Building Verus Demo ==="
cargo build $RELEASE_FLAG --verbose

if [ "$RUN_TESTS" = true ]; then
    echo ""
    echo "=== Running Tests ==="
    cargo test $RELEASE_FLAG --verbose
fi

echo ""
echo "Verus demo build complete!"
