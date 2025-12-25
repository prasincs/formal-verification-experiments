#!/bin/sh
# Run the Verus demo container
# Works with Docker, Podman, or any OCI-compliant runtime

set -e

IMAGE_NAME="verus-demo"

# Check if a runtime is actually working (not just installed)
runtime_works() {
    "$1" info >/dev/null 2>&1
}

# Auto-detect container runtime (prefer working runtime over installed one)
detect_runtime() {
    # Check Docker first (most common, usually "just works")
    if command -v docker >/dev/null 2>&1 && runtime_works docker; then
        echo "docker"
        return
    fi

    # Check Podman
    if command -v podman >/dev/null 2>&1; then
        if runtime_works podman; then
            echo "podman"
            return
        else
            echo "podman-not-running"
            return
        fi
    fi

    # Check nerdctl
    if command -v nerdctl >/dev/null 2>&1 && runtime_works nerdctl; then
        echo "nerdctl"
        return
    fi

    # Check Apple container CLI (macOS, requires Xcode 16+)
    if command -v container >/dev/null 2>&1; then
        echo "container"
        return
    fi

    echo ""
}

RUNTIME=$(detect_runtime)

# Handle no working runtime
if [ -z "$RUNTIME" ] || [ "$RUNTIME" = "podman-not-running" ]; then
    echo "No running container runtime detected."
    echo

    # Check what's installed
    HAS_DOCKER=$(command -v docker >/dev/null 2>&1 && echo "yes" || echo "no")
    HAS_PODMAN=$(command -v podman >/dev/null 2>&1 && echo "yes" || echo "no")

    if [ "$HAS_DOCKER" = "yes" ] || [ "$HAS_PODMAN" = "yes" ]; then
        echo "Installed but not running:"
        [ "$HAS_DOCKER" = "yes" ] && echo "  - Docker: Start Docker Desktop app"
        [ "$HAS_PODMAN" = "yes" ] && echo "  - Podman: podman machine start"
        echo
    fi

    echo "Quick start options:"
    echo
    echo "  # Option 1: Start Docker Desktop (if installed)"
    echo "  open -a Docker"
    echo
    echo "  # Option 2: Start Podman VM (if installed)"
    echo "  podman machine start"
    echo
    echo "  # Option 3: Install Docker Desktop"
    echo "  brew install --cask docker"
    echo
    exit 1
fi

echo "Using container runtime: $RUNTIME"
echo

# Check if image exists, if not build it
image_exists() {
    $RUNTIME images --format '{{.Repository}}' 2>/dev/null | grep -q "^${IMAGE_NAME}$"
}

if ! image_exists; then
    echo "Building image (this may take 5-10 minutes on first run)..."
    echo
    $RUNTIME build -t "$IMAGE_NAME" -f Containerfile .
    echo
fi

# Apple container CLI has different syntax
if [ "$RUNTIME" = "container" ]; then
    echo "Using Apple container CLI (experimental)"
    case "$1" in
        shell)
            echo "Starting interactive shell..."
            echo "  - Run 'verus --crate-type lib verified/src/lib.rs' to verify"
            echo "  - Run 'verus examples/01_division_by_zero.rs' to see a failure"
            echo
            container run --interactive --tty --rm "$IMAGE_NAME" /bin/bash
            ;;
        build)
            echo "Building image..."
            container build --tag "$IMAGE_NAME" --file Containerfile .
            ;;
        *)
            if ! container images 2>/dev/null | grep -q "$IMAGE_NAME"; then
                echo "Building image (this may take ~30 minutes on first run)..."
                container build --tag "$IMAGE_NAME" --file Containerfile .
            fi
            echo "=== Verus Formal Verification Demo ==="
            echo
            echo "Project structure:"
            echo "  verified/src/lib.rs  - Verified library (cargo + verus compatible)"
            echo "  app/src/main.rs      - Example application"
            echo "  examples/            - 8 failure examples"
            echo
            container run --rm "$IMAGE_NAME"
            ;;
    esac
    exit 0
fi

# Run the container (Docker/Podman/nerdctl)
case "$1" in
    shell)
        echo "Starting interactive shell..."
        echo "  - Run 'verus --crate-type lib verified/src/lib.rs' to verify"
        echo "  - Run 'verus examples/01_division_by_zero.rs' to see a failure"
        echo
        $RUNTIME run -it --rm -v "$(pwd)":/home/verus/demo:ro "$IMAGE_NAME" /bin/bash
        ;;
    build)
        echo "Rebuilding image..."
        $RUNTIME build -t "$IMAGE_NAME" -f Containerfile .
        ;;
    examples)
        echo "=== Running all failure examples ==="
        echo
        for f in examples/*.rs; do
            name=$(basename "$f")
            echo "--- $name ---"
            $RUNTIME run --rm -v "$(pwd)":/home/verus/demo:ro "$IMAGE_NAME" \
                verus "/home/verus/demo/$f" 2>&1 | head -20
            echo
        done
        ;;
    *)
        # Default: run verification on the library
        echo "=== Verus Formal Verification Demo ==="
        echo
        echo "Project structure:"
        echo "  verified/src/lib.rs  - Verified library (cargo + verus compatible)"
        echo "  app/src/main.rs      - Example application"
        echo "  examples/            - 8 failure examples"
        echo
        echo "Running: verus --crate-type lib verified/src/lib.rs"
        echo
        $RUNTIME run --rm -v "$(pwd)":/home/verus/demo:ro "$IMAGE_NAME" \
            verus --crate-type lib /home/verus/demo/verified/src/lib.rs
        echo
        echo "---"
        echo "Try also:"
        echo "  ./run.sh shell     - Interactive shell"
        echo "  ./run.sh examples  - Run all failure examples"
        ;;
esac
