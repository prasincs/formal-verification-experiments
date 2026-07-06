#!/data/data/com.termux/files/usr/bin/sh
# Boot the seL4 image in QEMU inside Termux — on-device software
# emulation, no root required.
#
# This is the exact machine the image is built for (Microkit board
# qemu_virt_aarch64), so behavior matches `make ... PLATFORM=android-avf
# run` on the host. Keep the machine/CPU/loader-address arguments in
# sync with build-system/config/platforms/android-avf.mk.
#
# Setup (once, in Termux):
#   pkg install qemu-system-aarch64-headless
#
# Usage: unpack the termux bundle next to this script and run:
#   sh run-termux-qemu.sh
#
# Exit QEMU with Ctrl-A X.
#
# Environment overrides: IMAGE, QEMU_MEMORY.

DIR=$(dirname "$0")
IMAGE="${IMAGE:-$DIR/loader.img}"
QEMU_MEMORY="${QEMU_MEMORY:-1024}"
LOADER_ADDR=0x70000000

if [ ! -f "$IMAGE" ]; then
    echo "error: image not found: $IMAGE" >&2
    exit 1
fi

if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
    echo "error: qemu-system-aarch64 not found." >&2
    echo "       In Termux: pkg install qemu-system-aarch64-headless" >&2
    exit 1
fi

echo "=== Booting seL4 Microkit in Termux QEMU ==="
echo "Press Ctrl-A X to exit"
echo ""
exec qemu-system-aarch64 \
    -machine virt,virtualization=on \
    -cpu cortex-a53 \
    -m "$QEMU_MEMORY" \
    -nographic \
    -device loader,file="$IMAGE",addr=$LOADER_ADDR,cpu-num=0
