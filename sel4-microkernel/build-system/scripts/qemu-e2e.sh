#!/usr/bin/env bash
# Build and boot the QEMU Microkit acceptance targets.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SEL4_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

MICROKIT_SDK="${MICROKIT_SDK:-/opt/microkit-sdk}"
BOOT_TIMEOUT="${BOOT_TIMEOUT:-30}"
IPDEMO_TIMEOUT="${IPDEMO_TIMEOUT:-40}"
RUN_HELLO="${RUN_HELLO:-1}"
RUN_NETDEMO="${RUN_NETDEMO:-1}"
RUN_IPDEMO="${RUN_IPDEMO:-1}"
QEMU_E2E_LOG_DIR="${QEMU_E2E_LOG_DIR:-}"

if [ -z "$QEMU_E2E_LOG_DIR" ]; then
    QEMU_E2E_LOG_DIR="$(mktemp -d "${TMPDIR:-/tmp}/qemu-e2e.XXXXXX")"
else
    mkdir -p "$QEMU_E2E_LOG_DIR"
fi

HELLO_LOG="${QEMU_E2E_LOG_DIR}/microkit-hello.log"
NETDEMO_LOG="${QEMU_E2E_LOG_DIR}/netdemo.log"
IPDEMO_LOG="${QEMU_E2E_LOG_DIR}/ipdemo.log"

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "missing required command: $1" >&2
        exit 1
    fi
}

assert_log() {
    local log_file="$1"
    local pattern="$2"
    local label="$3"

    if grep -Eq "$pattern" "$log_file"; then
        echo "ok: $label"
        return
    fi

    echo "missing marker: $label" >&2
    echo "pattern: $pattern" >&2
    echo "log: $log_file" >&2
    tail -n 120 "$log_file" >&2 || true
    exit 1
}

run_qemu_capture() {
    local timeout_s="$1"
    local log_file="$2"
    shift 2

    rm -f "$log_file"
    set +e
    timeout "${timeout_s}s" "$@" 2>&1 | tee "$log_file"
    local qemu_status="${PIPESTATUS[0]}"
    set -e

    # QEMU demos keep running after success; timeout(1)'s 124 is expected.
    if [ "$qemu_status" -ne 0 ] && [ "$qemu_status" -ne 124 ]; then
        echo "QEMU exited with status $qemu_status" >&2
        tail -n 120 "$log_file" >&2 || true
        exit "$qemu_status"
    fi
}

require make
require qemu-system-aarch64
require timeout

echo "QEMU e2e logs: ${QEMU_E2E_LOG_DIR}"

if [ ! -x "${MICROKIT_SDK}/bin/microkit" ]; then
    echo "Microkit SDK not found at ${MICROKIT_SDK}" >&2
    exit 1
fi

if [ "$RUN_HELLO" = "1" ]; then
    echo "==> microkit-hello: build"
    make -C "${SEL4_ROOT}/microkit-hello" ARCH=aarch64 MICROKIT_SDK="${MICROKIT_SDK}"

    echo "==> microkit-hello: QEMU boot"
    run_qemu_capture "${BOOT_TIMEOUT}" "$HELLO_LOG" \
        qemu-system-aarch64 \
            -machine virt,virtualization=on \
            -cpu cortex-a53 \
            -m 2G \
            -nographic \
            -device loader,file="${SEL4_ROOT}/microkit-hello/build/aarch64/loader.img",addr=0x70000000,cpu-num=0

    assert_log "$HELLO_LOG" "seL4 Microkit" "hello banner"
    assert_log "$HELLO_LOG" "Protection Domain" "hello protection domain"
    assert_log "$HELLO_LOG" "System ready" "hello ready marker"
fi

if [ "$RUN_NETDEMO" = "1" ]; then
    echo "==> netdemo: build"
    make -C "${SEL4_ROOT}/build-system" PRODUCT=netdemo PLATFORM=qemu-aarch64 MICROKIT_SDK="${MICROKIT_SDK}" all

    echo "==> netdemo: QEMU virtio-net boot"
    run_qemu_capture "${BOOT_TIMEOUT}" "$NETDEMO_LOG" \
        qemu-system-aarch64 \
            -machine virt,virtualization=on \
            -cpu cortex-a53 \
            -m 2G \
            -nographic \
            -device virtio-net-device,netdev=net0 \
            -netdev user,id=net0 \
            -device loader,file="${SEL4_ROOT}/build/qemu-aarch64/netdemo/loader.img",addr=0x70000000,cpu-num=0

    assert_log "$NETDEMO_LOG" "Network PD: interface initialized" "netdemo virtio-net initialized"
    assert_log "$NETDEMO_LOG" "netclient: ARP probe sent" "netdemo ARP probe sent"
    assert_log "$NETDEMO_LOG" "netclient: received frame" "netdemo frame received"
    assert_log "$NETDEMO_LOG" "netclient: ARP reply from 10\.0\.2\.2" "netdemo ARP reply decoded"
fi

if [ "$RUN_IPDEMO" = "1" ]; then
    if [ ! -f "${SEL4_ROOT}/build-system/config/products/ipdemo.mk" ]; then
        echo "==> ipdemo: skipped; product config not present"
    else
        echo "==> ipdemo: build"
        make -C "${SEL4_ROOT}/build-system" PRODUCT=ipdemo PLATFORM=qemu-aarch64 MICROKIT_SDK="${MICROKIT_SDK}" all

        echo "==> ipdemo: QEMU DHCP and ping"
        run_qemu_capture "${IPDEMO_TIMEOUT}" "$IPDEMO_LOG" \
            qemu-system-aarch64 \
                -machine virt,virtualization=on \
                -cpu cortex-a53 \
                -m 2G \
                -nographic \
                -device virtio-net-device,netdev=net0 \
                -netdev user,id=net0 \
                -device loader,file="${SEL4_ROOT}/build/qemu-aarch64/ipdemo/loader.img",addr=0x70000000,cpu-num=0

        assert_log "$IPDEMO_LOG" "IPDEMO START" "ipdemo started"
        assert_log "$IPDEMO_LOG" "DHCP OK 10\.0\.2\.[0-9]+/24" "ipdemo DHCP lease"
        assert_log "$IPDEMO_LOG" "PING OK" "ipdemo ping"
    fi
fi

echo "QEMU e2e validation passed"
