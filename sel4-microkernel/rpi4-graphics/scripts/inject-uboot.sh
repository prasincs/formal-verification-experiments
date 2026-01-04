#!/bin/bash
# Inject U-Boot and seL4 into an existing Raspberry Pi OS SD card
#
# Usage:
#   ./scripts/inject-uboot.sh /dev/sdX
#
# This script:
#   1. Mounts the first partition of the SD card
#   2. Backs up the original kernel
#   3. Copies U-Boot and seL4 loader
#   4. Updates config.txt to boot U-Boot
#
# After booting, at U-Boot prompt run:
#   fatload mmc 0 0x10000000 sel4.img
#   go 0x10000000

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build"

# Check arguments
if [[ $# -lt 1 ]]; then
    echo "Usage: $0 /dev/sdX"
    echo ""
    echo "Example: $0 /dev/sdb"
    exit 1
fi

DEVICE="$1"
PARTITION="${DEVICE}1"

# Verify device exists
if [[ ! -b "$DEVICE" ]]; then
    echo "Error: $DEVICE is not a block device"
    exit 1
fi

# Check build files exist
if [[ ! -f "$BUILD_DIR/u-boot.bin" ]]; then
    echo "Error: $BUILD_DIR/u-boot.bin not found"
    echo "Build U-Boot first with:"
    echo "  cd /tmp && git clone --depth 1 https://github.com/u-boot/u-boot.git"
    echo "  cd u-boot && make CROSS_COMPILE=aarch64-linux-gnu- rpi_4_defconfig"
    echo "  make CROSS_COMPILE=aarch64-linux-gnu- -j\$(nproc)"
    echo "  cp u-boot.bin $BUILD_DIR/"
    exit 1
fi

if [[ ! -f "$BUILD_DIR/loader.img" ]]; then
    echo "Error: $BUILD_DIR/loader.img not found. Run 'make' first."
    exit 1
fi

# Create mount point
MOUNT_DIR=$(mktemp -d)
trap "rmdir $MOUNT_DIR 2>/dev/null || true" EXIT

echo "=== Injecting U-Boot into Raspberry Pi OS SD Card ==="
echo "Device: $DEVICE"
echo "Partition: $PARTITION"
echo ""

# Mount
echo "Mounting $PARTITION..."
sudo mount "$PARTITION" "$MOUNT_DIR"

# Check it's a valid RPi boot partition
if [[ ! -f "$MOUNT_DIR/start4.elf" ]]; then
    echo "Error: This doesn't look like a Raspberry Pi boot partition"
    echo "Flash Raspberry Pi OS first, then run this script"
    sudo umount "$MOUNT_DIR"
    exit 1
fi

# Backup original kernel if not already backed up
if [[ -f "$MOUNT_DIR/kernel8.img" && ! -f "$MOUNT_DIR/kernel8.img.bak" ]]; then
    echo "Backing up kernel8.img..."
    sudo mv "$MOUNT_DIR/kernel8.img" "$MOUNT_DIR/kernel8.img.bak"
fi

# Copy U-Boot and seL4
echo "Copying U-Boot..."
sudo cp "$BUILD_DIR/u-boot.bin" "$MOUNT_DIR/"

echo "Copying seL4 loader..."
sudo cp "$BUILD_DIR/loader.img" "$MOUNT_DIR/sel4.img"

# Update config.txt if needed
if ! grep -q "^kernel=u-boot.bin" "$MOUNT_DIR/config.txt"; then
    echo "Updating config.txt..."
    echo "" | sudo tee -a "$MOUNT_DIR/config.txt" > /dev/null
    echo "# Boot U-Boot instead of Linux" | sudo tee -a "$MOUNT_DIR/config.txt" > /dev/null
    echo "kernel=u-boot.bin" | sudo tee -a "$MOUNT_DIR/config.txt" > /dev/null
fi

# Show results
echo ""
echo "=== Files on SD Card ==="
ls -la "$MOUNT_DIR/u-boot.bin" "$MOUNT_DIR/sel4.img"
echo ""
echo "=== config.txt ==="
cat "$MOUNT_DIR/config.txt"
echo ""

# Unmount
sudo umount "$MOUNT_DIR"
sync

echo "=== Done ==="
echo ""
echo "Boot the Pi and at U-Boot prompt run:"
echo "  fatload mmc 0 0x10000000 sel4.img"
echo "  go 0x10000000"
echo ""
