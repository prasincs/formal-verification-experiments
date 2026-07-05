#!/usr/bin/env bash
# Build and run the seL4 Microkit QEMU e2e container.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CONTAINERFILE="${REPO_ROOT}/sel4-microkernel/qemu-e2e.Containerfile"
IMAGE="${IMAGE:-formal-verification-qemu-e2e:local}"
CACHE_ROOT="${CACHE_ROOT:-${XDG_CACHE_HOME:-${HOME}/.cache}/formal-verification-qemu-e2e}"

pick_runtime() {
    if [ -n "${CONTAINER_RUNTIME:-}" ]; then
        command -v "${CONTAINER_RUNTIME}" >/dev/null 2>&1 && {
            echo "${CONTAINER_RUNTIME}"
            return
        }
        echo "CONTAINER_RUNTIME=${CONTAINER_RUNTIME} not found" >&2
        exit 1
    fi

    if command -v docker >/dev/null 2>&1; then
        echo docker
        return
    fi

    if command -v podman >/dev/null 2>&1; then
        echo podman
        return
    fi

    echo "Install Docker or Podman to run the QEMU e2e container." >&2
    exit 1
}

RUNTIME="$(pick_runtime)"
if ! mkdir -p "${CACHE_ROOT}/cargo" 2>/dev/null; then
    CACHE_ROOT="${TMPDIR:-/tmp}/formal-verification-qemu-e2e-${USER:-user}"
    mkdir -p "${CACHE_ROOT}/cargo"
fi

echo "==> Building ${IMAGE} with ${RUNTIME}"
"${RUNTIME}" build -t "${IMAGE}" -f "${CONTAINERFILE}" "${REPO_ROOT}"

RUN_ARGS=(
    run
    --rm
    --user "$(id -u):$(id -g)"
    -e CARGO_HOME=/cargo
    -e HOME=/tmp
    -e MICROKIT_SDK=/opt/microkit-sdk
    -e RUSTUP_HOME=/opt/rust/rustup
    -v "${REPO_ROOT}:/work"
    -v "${CACHE_ROOT}/cargo:/cargo"
    -w /work
)

if [ "${RUNTIME##*/}" = "podman" ]; then
    RUN_ARGS+=(--security-opt label=disable)
fi

if [ "$#" -eq 0 ]; then
    set -- ./sel4-microkernel/build-system/scripts/qemu-e2e.sh
fi

echo "==> Running ${IMAGE}"
exec "${RUNTIME}" "${RUN_ARGS[@]}" "${IMAGE}" "$@"
