#!/bin/bash
# Create a bootable SD card image for Raspberry Pi 4 with seL4/Microkit
#
# Usage:
#   ./scripts/create-sdcard.sh [options]
#
# Options:
#   --uboot       Include U-Boot bootloader (for debugging)
#   --output FILE Output image filename (default: build/rpi4-sel4.img)
#   --size SIZE   Image size in MB (default: 64)
#
# Requires: mtools, dosfstools (mkfs.vfat), util-linux (sfdisk, losetup)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build"

# Defaults
USE_UBOOT=false
OUTPUT_IMG="$BUILD_DIR/rpi4-sel4.img"
IMG_SIZE_MB=64

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --uboot)
            USE_UBOOT=true
            shift
            ;;
        --output)
            OUTPUT_IMG="$2"
            shift 2
            ;;
        --size)
            IMG_SIZE_MB="$2"
            shift 2
            ;;
        -h|--help)
            head -20 "$0" | grep "^#" | cut -c3-
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Check dependencies
for cmd in mcopy mkfs.vfat sfdisk; do
    if ! command -v $cmd &>/dev/null; then
        echo "Error: $cmd not found. Install mtools and dosfstools."
        exit 1
    fi
done

# Check build files exist
if [[ ! -f "$BUILD_DIR/loader.img" ]]; then
    echo "Error: $BUILD_DIR/loader.img not found. Run 'make' first."
    exit 1
fi

if [[ ! -d "$BUILD_DIR/firmware" ]]; then
    echo "Error: $BUILD_DIR/firmware not found. Run 'make firmware' first."
    exit 1
fi

echo "=== Creating SD Card Image ==="
echo "Output: $OUTPUT_IMG"
echo "Size: ${IMG_SIZE_MB}MB"
echo "U-Boot: $USE_UBOOT"
echo ""

# Create empty image
echo "Creating ${IMG_SIZE_MB}MB image..."
dd if=/dev/zero of="$OUTPUT_IMG" bs=1M count=$IMG_SIZE_MB status=none

# Create MBR partition table with bootable FAT32 partition
echo "Creating partition table..."
echo 'type=c, bootable' | sfdisk "$OUTPUT_IMG" >/dev/null 2>&1

# Calculate partition offset (usually 2048 sectors = 1MB)
PART_OFFSET=$((2048 * 512))

# Format the partition using mtools (no root required)
# First, create mtools config for the partition
MTOOLSRC=$(mktemp)
cat > "$MTOOLSRC" << EOF
drive x:
    file="$OUTPUT_IMG"
    partition=1
EOF

export MTOOLSRC

echo "Formatting partition..."
mformat -F x:

echo "Copying boot files..."
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/firmware/start4.elf" ::
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/firmware/fixup4.dat" ::
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/firmware/bcm2711-rpi-4-b.dtb" ::

if [[ "$USE_UBOOT" == true ]]; then
    if [[ ! -f "$BUILD_DIR/u-boot.bin" ]]; then
        echo "Error: $BUILD_DIR/u-boot.bin not found."
        echo "Build U-Boot first or use without --uboot"
        rm "$MTOOLSRC"
        exit 1
    fi

    # U-Boot config
    cat > "$BUILD_DIR/config-uboot.txt" << 'UBOOTCFG'
# seL4 via U-Boot on Raspberry Pi 4
arm_64bit=1
kernel=u-boot.bin
hdmi_force_hotplug=1
hdmi_group=1
hdmi_mode=16
disable_overscan=1
gpu_mem=64
enable_uart=1
UBOOTCFG

    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/config-uboot.txt" ::config.txt
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/u-boot.bin" ::
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/loader.img" ::sel4.img

    echo ""
    echo "U-Boot image created. Boot instructions:"
    echo "  1. Power on Pi 4 with HDMI connected"
    echo "  2. Press any key to stop autoboot"
    echo "  3. Run: fatload mmc 0 0x10000000 sel4.img"
    echo "  4. Run: go 0x10000000"
else
    # Direct boot config
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/config.txt" ::
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/loader.img" ::
fi

echo ""
echo "Image contents:"
mdir -i "$OUTPUT_IMG@@$PART_OFFSET"

rm "$MTOOLSRC"

echo ""
echo "=== SD Card Image Ready ==="
echo "Flash with:"
echo "  sudo dd if=$OUTPUT_IMG of=/dev/sdX bs=4M status=progress conv=fsync"
echo ""
