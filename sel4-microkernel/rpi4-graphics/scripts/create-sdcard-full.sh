#!/bin/bash
# Create a complete bootable SD card image with RPi firmware + seL4
#
# Usage:
#   ./scripts/create-sdcard-full.sh [options]
#
# Options:
#   --uboot         Include U-Boot bootloader (for debugging)
#   --direct        Direct boot without U-Boot (default)
#   --output FILE   Output image filename
#   --size SIZE     Image size in MB (default: 64)
#   --no-cache      Re-download firmware even if cached
#
# This script downloads official Raspberry Pi firmware and creates
# a complete bootable image that works like RPi OS but boots seL4.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build"
CACHE_DIR="$BUILD_DIR/firmware-cache"

# Raspberry Pi firmware repository
FIRMWARE_REPO="https://github.com/raspberrypi/firmware/raw/master/boot"

# Required firmware files
FIRMWARE_FILES=(
    "start4.elf"
    "fixup4.dat"
    "bcm2711-rpi-4-b.dtb"
)

# Optional but recommended files
EXTRA_FIRMWARE=(
    "start4x.elf"
    "fixup4x.dat"
    "start4cd.elf"
    "fixup4cd.dat"
)

# Defaults
USE_UBOOT=false
OUTPUT_IMG="$BUILD_DIR/rpi4-sel4-full.img"
IMG_SIZE_MB=64
USE_CACHE=true

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --uboot)
            USE_UBOOT=true
            shift
            ;;
        --direct)
            USE_UBOOT=false
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
        --no-cache)
            USE_CACHE=false
            shift
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
for cmd in mcopy mkfs.vfat sfdisk mformat; do
    if ! command -v $cmd &>/dev/null; then
        echo "Error: $cmd not found. Install mtools and dosfstools."
        exit 1
    fi
done

# Check for mkimage (optional but recommended)
if ! command -v mkimage &>/dev/null; then
    echo "Warning: mkimage not found. Install uboot-tools for boot script support."
    echo "         Continuing without boot.scr..."
    HAS_MKIMAGE=false
else
    HAS_MKIMAGE=true
fi

# Check build files
if [[ ! -f "$BUILD_DIR/loader.img" ]]; then
    echo "Error: $BUILD_DIR/loader.img not found. Run 'make' first."
    exit 1
fi

if [[ "$USE_UBOOT" == true && ! -f "$BUILD_DIR/u-boot.bin" ]]; then
    echo "U-Boot not found. Building from submodule..."
    make -C "$PROJECT_DIR" uboot
    if [[ ! -f "$BUILD_DIR/u-boot.bin" ]]; then
        echo "Error: Failed to build U-Boot."
        echo "You can also build manually with: make uboot"
        exit 1
    fi
fi

# Download firmware files
download_firmware() {
    mkdir -p "$CACHE_DIR"

    echo "Downloading Raspberry Pi firmware..."

    for file in "${FIRMWARE_FILES[@]}" "${EXTRA_FIRMWARE[@]}"; do
        if [[ "$USE_CACHE" == true && -f "$CACHE_DIR/$file" ]]; then
            echo "  [cached] $file"
        else
            echo "  [download] $file"
            curl -sL -o "$CACHE_DIR/$file" "$FIRMWARE_REPO/$file" || true
        fi
    done

    # Download vc4 overlay for display support
    mkdir -p "$CACHE_DIR/overlays"
    if [[ "$USE_CACHE" == true && -f "$CACHE_DIR/overlays/vc4-kms-v3d.dtbo" ]]; then
        echo "  [cached] overlays/vc4-kms-v3d.dtbo"
    else
        echo "  [download] overlays/vc4-kms-v3d.dtbo"
        curl -sL -o "$CACHE_DIR/overlays/vc4-kms-v3d.dtbo" \
            "$FIRMWARE_REPO/overlays/vc4-kms-v3d.dtbo" || true
    fi
}

# Create U-Boot boot script
create_boot_script() {
    local boot_cmd="$1"
    local boot_scr="$2"

    cat > "$boot_cmd" << 'EOF'
echo "=== seL4 Microkit Boot ==="
echo "Loading seL4 image..."
fatload mmc 0 0x10000000 sel4.img
echo "Starting seL4 at 0x10000000..."
go 0x10000000
EOF

    if [[ "$HAS_MKIMAGE" == true ]]; then
        mkimage -A arm64 -T script -C none -d "$boot_cmd" "$boot_scr" > /dev/null
        echo "Created boot.scr with mkimage"
    else
        # Fallback: just copy the text file (won't auto-boot but can be sourced)
        cp "$boot_cmd" "$boot_scr"
        echo "Created boot.scr as plain text (mkimage not available)"
    fi
}

# Create config.txt
create_config() {
    local config_file="$1"

    if [[ "$USE_UBOOT" == true ]]; then
        cat > "$config_file" << 'EOF'
# seL4 via U-Boot on Raspberry Pi 4
arm_64bit=1
kernel=u-boot.bin

# Display - force 720p for compatibility
hdmi_force_hotplug=1
hdmi_group=1
hdmi_mode=4
disable_overscan=1

# GPU memory for framebuffer
gpu_mem=128

# UART debug output
enable_uart=1
EOF
    else
        cat > "$config_file" << 'EOF'
# seL4 Microkit on Raspberry Pi 4
arm_64bit=1
kernel=loader.img
kernel_address=0x10000000

# Display
hdmi_force_hotplug=1
disable_overscan=1
dtoverlay=vc4-kms-v3d
max_framebuffers=2

# GPU memory
gpu_mem=64

# UART debug output
enable_uart=1
EOF
    fi
}

echo "=== Creating Complete SD Card Image ==="
echo "Output: $OUTPUT_IMG"
echo "Size: ${IMG_SIZE_MB}MB"
echo "U-Boot: $USE_UBOOT"
echo ""

# Download firmware
download_firmware

# Create empty image
echo ""
echo "Creating ${IMG_SIZE_MB}MB image..."
dd if=/dev/zero of="$OUTPUT_IMG" bs=1M count=$IMG_SIZE_MB status=none

# Create MBR partition table
echo "Creating partition table..."
echo 'type=c, bootable' | sfdisk "$OUTPUT_IMG" >/dev/null 2>&1

# Calculate partition offset (2048 sectors = 1MB)
PART_OFFSET=$((2048 * 512))

# Setup mtools config
MTOOLSRC=$(mktemp)
cat > "$MTOOLSRC" << EOF
drive x:
    file="$OUTPUT_IMG"
    partition=1
EOF
export MTOOLSRC

# Format partition
echo "Formatting partition..."
mformat -F x:

# Copy firmware files
echo "Copying firmware..."
for file in "${FIRMWARE_FILES[@]}"; do
    if [[ -f "$CACHE_DIR/$file" ]]; then
        mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$CACHE_DIR/$file" ::
    fi
done

for file in "${EXTRA_FIRMWARE[@]}"; do
    if [[ -f "$CACHE_DIR/$file" ]]; then
        mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$CACHE_DIR/$file" ::
    fi
done

# Create overlays directory and copy
echo "Copying overlays..."
mmd -i "$OUTPUT_IMG@@$PART_OFFSET" ::overlays 2>/dev/null || true
if [[ -f "$CACHE_DIR/overlays/vc4-kms-v3d.dtbo" ]]; then
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$CACHE_DIR/overlays/vc4-kms-v3d.dtbo" ::overlays/
fi

# Create and copy config.txt
echo "Creating config.txt..."
TMP_CONFIG=$(mktemp)
create_config "$TMP_CONFIG"
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$TMP_CONFIG" ::config.txt
rm "$TMP_CONFIG"

# Copy seL4/U-Boot files
if [[ "$USE_UBOOT" == true ]]; then
    echo "Copying U-Boot..."
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/u-boot.bin" ::
    echo "Copying seL4 loader as sel4.img..."
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/loader.img" ::sel4.img

    # Create and copy boot script for auto-boot
    echo "Creating boot script..."
    TMP_BOOT_CMD=$(mktemp)
    TMP_BOOT_SCR=$(mktemp)
    create_boot_script "$TMP_BOOT_CMD" "$TMP_BOOT_SCR"
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$TMP_BOOT_SCR" ::boot.scr
    rm "$TMP_BOOT_CMD" "$TMP_BOOT_SCR"
else
    echo "Copying seL4 loader..."
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/loader.img" ::
fi

# Cleanup
rm "$MTOOLSRC"

# Show contents
echo ""
echo "=== Image Contents ==="
mdir -i "$OUTPUT_IMG@@$PART_OFFSET"

echo ""
echo "=== SD Card Image Ready ==="
echo "Flash with:"
echo "  sudo dd if=$OUTPUT_IMG of=/dev/sdX bs=4M status=progress conv=fsync"
echo ""

if [[ "$USE_UBOOT" == true ]]; then
    echo "At U-Boot prompt, run:"
    echo "  fatload mmc 0 0x10000000 sel4.img"
    echo "  go 0x10000000"
fi
