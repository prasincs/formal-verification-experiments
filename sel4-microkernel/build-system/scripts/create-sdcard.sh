#!/bin/bash
# Create a bootable SD card image for Raspberry Pi 4
#
# Usage:
#   ./create-sdcard.sh --loader LOADER --firmware DIR --output IMG [options]
#
# Options:
#   --loader FILE       Path to loader.img (required)
#   --loader-elf FILE   Path to loader.elf (for U-Boot bootelf)
#   --firmware DIR      Path to firmware directory (required)
#   --config FILE       Path to config.txt (required)
#   --output FILE       Output image file (required)
#   --uboot FILE        Include U-Boot bootloader
#   --size MB           Image size in MB (default: 64)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Defaults
IMG_SIZE_MB=64
USE_UBOOT=""
LOADER=""
LOADER_ELF=""
FIRMWARE_DIR=""
CONFIG_FILE=""
OUTPUT_IMG=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --loader)
            LOADER="$2"
            shift 2
            ;;
        --loader-elf)
            LOADER_ELF="$2"
            shift 2
            ;;
        --firmware)
            FIRMWARE_DIR="$2"
            shift 2
            ;;
        --config)
            CONFIG_FILE="$2"
            shift 2
            ;;
        --output)
            OUTPUT_IMG="$2"
            shift 2
            ;;
        --uboot)
            USE_UBOOT="$2"
            shift 2
            ;;
        --size)
            IMG_SIZE_MB="$2"
            shift 2
            ;;
        -h|--help)
            head -14 "$0" | grep "^#" | cut -c3-
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Validate required arguments
if [[ -z "$LOADER" ]] || [[ -z "$FIRMWARE_DIR" ]] || [[ -z "$OUTPUT_IMG" ]]; then
    echo "Error: --loader, --firmware, and --output are required"
    exit 1
fi

# Check dependencies
for cmd in mcopy mkfs.vfat sfdisk mformat; do
    if ! command -v $cmd &>/dev/null; then
        echo "Error: $cmd not found. Install mtools and dosfstools."
        exit 1
    fi
done

# Check build files
if [[ ! -f "$LOADER" ]]; then
    echo "Error: Loader not found: $LOADER"
    exit 1
fi

if [[ ! -d "$FIRMWARE_DIR" ]]; then
    echo "Error: Firmware directory not found: $FIRMWARE_DIR"
    exit 1
fi

# Check for mkimage (optional)
HAS_MKIMAGE=false
if command -v mkimage &>/dev/null; then
    HAS_MKIMAGE=true
fi

# Create U-Boot boot script
create_boot_script() {
    local boot_cmd="$1"
    local boot_scr="$2"

    cat > "$boot_cmd" << 'EOF'
echo ""
echo "=== seL4 Microkit on Raspberry Pi 4 ==="
echo ""
echo "=== Trying bootelf with ELF loader ==="
fatload mmc 0 0x20000000 loader.elf
bootelf 0x20000000
echo ""
echo "=== bootelf failed, trying go with binary ==="
fatload mmc 0 0x10000000 sel4.img
go 0x10000000
EOF

    if [[ "$HAS_MKIMAGE" == true ]]; then
        mkimage -A arm64 -T script -C none -d "$boot_cmd" "$boot_scr" > /dev/null
    else
        cp "$boot_cmd" "$boot_scr"
    fi
}

echo "=== Creating SD Card Image ==="
echo "Output: $OUTPUT_IMG"
echo "Size: ${IMG_SIZE_MB}MB"
echo "U-Boot: ${USE_UBOOT:-none}"
echo ""

# Create empty image
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
for file in start4.elf fixup4.dat bcm2711-rpi-4-b.dtb; do
    if [[ -f "$FIRMWARE_DIR/$file" ]]; then
        mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$FIRMWARE_DIR/$file" ::
    fi
done

# Copy config.txt
if [[ -n "$CONFIG_FILE" ]] && [[ -f "$CONFIG_FILE" ]]; then
    echo "Copying config.txt..."
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$CONFIG_FILE" ::config.txt
fi

# Copy loader/U-Boot files
if [[ -n "$USE_UBOOT" ]] && [[ -f "$USE_UBOOT" ]]; then
    echo "Copying U-Boot..."
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$USE_UBOOT" ::u-boot.bin

    echo "Copying seL4 loader as sel4.img..."
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$LOADER" ::sel4.img

    if [[ -n "$LOADER_ELF" ]] && [[ -f "$LOADER_ELF" ]]; then
        echo "Copying loader.elf for bootelf..."
        mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$LOADER_ELF" ::loader.elf
    fi

    # Create boot script
    echo "Creating boot script..."
    TMP_BOOT_CMD=$(mktemp)
    TMP_BOOT_SCR=$(mktemp)
    create_boot_script "$TMP_BOOT_CMD" "$TMP_BOOT_SCR"
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$TMP_BOOT_SCR" ::boot.scr
    rm "$TMP_BOOT_CMD" "$TMP_BOOT_SCR"
else
    echo "Copying seL4 loader..."
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$LOADER" ::loader.img
fi

# Cleanup
rm "$MTOOLSRC"

# Show contents
echo ""
echo "=== Image Contents ==="
mdir -i "$OUTPUT_IMG@@$PART_OFFSET"

echo ""
echo "SD card image created: $OUTPUT_IMG"
