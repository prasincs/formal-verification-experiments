#!/bin/bash
# Create a bootable SD card image with seL4Test for RPi4
#
# Usage:
#   ./scripts/create-sel4test-sdcard.sh [--8gb|--4gb]
#
# The memory variant must match your Raspberry Pi 4's RAM size

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build"
CACHE_DIR="$BUILD_DIR/firmware-cache"
SELTEST_DIR="$PROJECT_DIR/../sel4test"

OUTPUT_IMG="$BUILD_DIR/rpi4-sel4test.img"
IMG_SIZE_MB=64
PART_OFFSET=$((2048 * 512))

# Default to 2GB (for testing)
MEMORY_SIZE="2gb"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --8gb)
            MEMORY_SIZE="8gb"
            shift
            ;;
        --4gb)
            MEMORY_SIZE="4gb"
            shift
            ;;
        --2gb)
            MEMORY_SIZE="2gb"
            shift
            ;;
        -h|--help)
            head -8 "$0" | grep "^#" | cut -c3-
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

echo "=== Creating seL4Test SD Card Image ==="
echo "Memory: $MEMORY_SIZE"

# Check for seL4Test image
if [[ "$MEMORY_SIZE" == "8gb" ]]; then
    SEL4TEST_IMG="$BUILD_DIR/sel4test-8gb.img"
elif [[ "$MEMORY_SIZE" == "2gb" ]]; then
    SEL4TEST_IMG="$BUILD_DIR/sel4test-2gb.img"
else
    SEL4TEST_IMG="$BUILD_DIR/sel4test.img"
fi

if [[ ! -f "$SEL4TEST_IMG" ]]; then
    echo "Error: seL4Test image not found: $SEL4TEST_IMG"
    echo ""
    echo "Build seL4Test first:"
    echo "  cd ../sel4test/build"
    if [[ "$MEMORY_SIZE" == "8gb" ]]; then
        echo "  ../init-build.sh -DPLATFORM=rpi4 -DAARCH64=1"
    else
        echo "  ../init-build.sh -DPLATFORM=rpi4 -DAARCH64=1 -DRPI4_MEMORY=4096"
    fi
    echo "  ninja"
    exit 1
fi

# Check for firmware cache
if [[ ! -d "$CACHE_DIR" ]]; then
    echo "Error: Firmware cache not found. Run 'make firmware' first."
    exit 1
fi

# Check for U-Boot
if [[ ! -f "$BUILD_DIR/u-boot.bin" ]]; then
    echo "Error: U-Boot not found. Run 'make uboot' first."
    exit 1
fi

# Check for mkimage
if ! command -v mkimage &>/dev/null; then
    echo "Error: mkimage not found. Install uboot-tools."
    exit 1
fi

echo ""
echo "Creating ${IMG_SIZE_MB}MB image..."
dd if=/dev/zero of="$OUTPUT_IMG" bs=1M count=$IMG_SIZE_MB status=none
echo 'type=c, bootable' | sfdisk "$OUTPUT_IMG" >/dev/null 2>&1

# Setup mtools config
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

# Create config.txt for U-Boot
echo "Creating config.txt..."
TMP_CONFIG=$(mktemp)
cat > "$TMP_CONFIG" << 'EOF'
# seL4Test via U-Boot on Raspberry Pi 4
arm_64bit=1
kernel=u-boot.bin

# Display - 720p
hdmi_force_hotplug=1
hdmi_group=1
hdmi_mode=4
disable_overscan=1

# GPU memory
gpu_mem=64

# UART debug
enable_uart=1
EOF
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$TMP_CONFIG" ::config.txt
rm "$TMP_CONFIG"

# Copy U-Boot
echo "Copying U-Boot..."
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/u-boot.bin" ::

# Copy seL4Test image
echo "Copying seL4Test image..."
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$SEL4TEST_IMG" ::sel4test.img

# Create boot script
echo "Creating boot script..."
TMP_BOOT_CMD=$(mktemp)
TMP_BOOT_SCR=$(mktemp)

cat > "$TMP_BOOT_CMD" << 'EOF'
echo ""
echo "  ____  _____ _     _  _   _____         _   "
echo " / ___|| ____| |   | || | |_   _|__  ___| |_ "
echo " \___ \|  _| | |   | || |_  | |/ _ \/ __| __|"
echo "  ___) | |___| |___|__   _| | |  __/\__ \ |_ "
echo " |____/|_____|_____|  |_|   |_|\___||___/\__|"
echo ""
echo "     seL4Test on Raspberry Pi 4"
echo ""
echo "=== Board Info ==="
bdinfo
echo ""
echo "=== Loading seL4Test... ==="
fatload mmc 0 0x10000000 sel4test.img
echo "=== Starting seL4Test at 0x10000000 ==="
go 0x10000000
EOF

mkimage -A arm64 -T script -C none -d "$TMP_BOOT_CMD" "$TMP_BOOT_SCR" > /dev/null
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$TMP_BOOT_SCR" ::boot.scr
rm "$TMP_BOOT_CMD" "$TMP_BOOT_SCR"

# Cleanup
rm "$MTOOLSRC"

echo ""
echo "=== Image Contents ==="
mdir -i "$OUTPUT_IMG@@$PART_OFFSET"

echo ""
echo "=== seL4Test SD Card Ready ($MEMORY_SIZE) ==="
echo "Flash with:"
echo "  sudo dd if=$OUTPUT_IMG of=/dev/sdX bs=4M status=progress conv=fsync"
echo ""
echo "Note: The boot script will show board info (bdinfo) before loading seL4Test."
echo "      This will tell you your actual RAM size."
