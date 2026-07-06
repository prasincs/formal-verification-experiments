#!/system/bin/sh
# On-device launcher: boot the staged seL4 image under AVF's crosvm.
#
# Runs crosvm directly (bypassing VirtualizationService/Microdroid), so
# it needs a root shell — rooted device or userdebug build. The default
# Android seccomp policy blocks crosvm's worker threads when launched
# from an interactive shell, hence --disable-sandbox; the guest still
# sits behind KVM/pKVM stage-2 isolation.
#
# Serial: crosvm routes the guest UART to stdout, i.e. straight into
# the adb shell session that launched this script.
#
# BRING-UP STATUS: the image staged next to this script is built for
# the Microkit qemu_virt_aarch64 board. crosvm's aarch64 machine
# differs (RAM base, UART, GIC), so until a crosvm Microkit board port
# exists this launch is expected to produce no guest output. See
# sel4-microkernel/docs/android-agent-os.md.
#
# Environment overrides: CROSVM, IMAGE, MEM_MB, CPUS.

DIR=$(dirname "$0")
CROSVM="${CROSVM:-/apex/com.android.virt/bin/crosvm}"
IMAGE="${IMAGE:-$DIR/loader.img}"
MEM_MB="${MEM_MB:-1024}"
CPUS="${CPUS:-1}"

if [ ! -f "$IMAGE" ]; then
    echo "error: image not found: $IMAGE (run deploy-avf first)" >&2
    exit 1
fi

if [ ! -x "$CROSVM" ]; then
    echo "error: crosvm not found at $CROSVM" >&2
    echo "       This device does not expose the AVF virtualization APEX." >&2
    exit 1
fi

run_cmd() {
    # Raw crosvm needs root; re-exec through su when the shell isn't.
    if [ "$(id -u)" = "0" ]; then
        "$@"
    elif command -v su >/dev/null 2>&1; then
        echo "(not root; re-launching via su)"
        su -c "$*"
    else
        echo "error: root required to run crosvm outside VirtualizationService." >&2
        echo "       Use a rooted/userdebug device, or the Termux QEMU path." >&2
        exit 1
    fi
}

echo "=== crosvm: booting $IMAGE (mem=${MEM_MB}MiB cpus=$CPUS) ==="
run_cmd "$CROSVM" run \
    --disable-sandbox \
    --mem "$MEM_MB" \
    --cpus "$CPUS" \
    --serial type=stdout,hardware=serial,num=1,console=true \
    --bios "$IMAGE"
