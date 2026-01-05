#!/bin/bash
# Create SD card that boots seL4 directly (no U-Boot)
# This is the method described in seL4 RPi4 docs
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build"
CACHE_DIR="$BUILD_DIR/firmware-cache"

OUTPUT_IMG="$BUILD_DIR/rpi4-sel4-direct.img"
IMG_SIZE_MB=64
PART_OFFSET=$((2048 * 512))

# Select image: seL4Test or Microkit
IMAGE_TYPE="${1:-sel4test}"

echo "=== Creating Direct Boot SD Card ==="
echo "Type: $IMAGE_TYPE (no U-Boot)"

if [[ "$IMAGE_TYPE" == "sel4test" ]]; then
    SEL4_IMG="$BUILD_DIR/sel4test-2gb.img"
    KERNEL_NAME="sel4test.img"
elif [[ "$IMAGE_TYPE" == "microkit" ]]; then
    SEL4_IMG="$BUILD_DIR/loader.img"
    KERNEL_NAME="loader.img"
else
    echo "Usage: $0 [sel4test|microkit]"
    exit 1
fi

if [[ ! -f "$SEL4_IMG" ]]; then
    echo "Error: Image not found: $SEL4_IMG"
    exit 1
fi

echo ""
echo "Creating ${IMG_SIZE_MB}MB image..."
dd if=/dev/zero of="$OUTPUT_IMG" bs=1M count=$IMG_SIZE_MB status=none
echo 'type=c, bootable' | sfdisk "$OUTPUT_IMG" >/dev/null 2>&1

# Setup mtools
MTOOLSRC=$(mktemp)
cat > "$MTOOLSRC" << EOF
drive x:
    file="$OUTPUT_IMG"
    partition=1
EOF
export MTOOLSRC
mformat -F x:

# Copy firmware
echo "Copying firmware..."
for file in start4.elf fixup4.dat bcm2711-rpi-4-b.dtb; do
    if [[ -f "$CACHE_DIR/$file" ]]; then
        mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$CACHE_DIR/$file" ::
    fi
done

# Create config.txt for DIRECT boot (no U-Boot)
# This loads the seL4 image directly via GPU firmware
echo "Creating config.txt for direct boot..."
TMP_CONFIG=$(mktemp)
cat > "$TMP_CONFIG" << EOF
# seL4 Direct Boot on Raspberry Pi 4 (2GB)
# No U-Boot - GPU firmware loads kernel directly

arm_64bit=1
kernel=$KERNEL_NAME
kernel_address=0x10000000

# Display
hdmi_force_hotplug=1
hdmi_group=1
hdmi_mode=4
disable_overscan=1

# GPU memory
gpu_mem=64

# UART for debug (active LED will blink on serial activity)
enable_uart=1

# Disable Bluetooth to free up UART
dtoverlay=disable-bt
EOF
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$TMP_CONFIG" ::config.txt
rm "$TMP_CONFIG"

# Copy seL4 image
echo "Copying seL4 image as $KERNEL_NAME..."
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$SEL4_IMG" "::$KERNEL_NAME"

rm "$MTOOLSRC"

echo ""
echo "=== Image Contents ==="
mdir -i "$OUTPUT_IMG@@$PART_OFFSET"

echo ""
echo "=== Direct Boot SD Card Ready ==="
echo "Flash with:"
echo "  sudo dd if=$OUTPUT_IMG of=/dev/sdX bs=4M status=progress conv=fsync"
echo ""
echo "This bypasses U-Boot entirely - RPi firmware loads seL4 directly."
echo "Watch for activity LED blinking (indicates boot activity)."
