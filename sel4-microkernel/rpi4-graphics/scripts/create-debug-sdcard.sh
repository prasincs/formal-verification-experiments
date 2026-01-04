#!/bin/bash
# Debug SD card - try multiple boot methods
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build"
CACHE_DIR="$BUILD_DIR/firmware-cache"

OUTPUT_IMG="$BUILD_DIR/rpi4-debug.img"
IMG_SIZE_MB=64
PART_OFFSET=$((2048 * 512))

echo "=== Creating Debug SD Card ==="

dd if=/dev/zero of="$OUTPUT_IMG" bs=1M count=$IMG_SIZE_MB status=none
echo 'type=c, bootable' | sfdisk "$OUTPUT_IMG" >/dev/null 2>&1

MTOOLSRC=$(mktemp)
cat > "$MTOOLSRC" << EOF
drive x:
    file="$OUTPUT_IMG"
    partition=1
EOF
export MTOOLSRC
mformat -F x:

# Copy firmware
for file in start4.elf fixup4.dat bcm2711-rpi-4-b.dtb; do
    mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$CACHE_DIR/$file" ::
done

# Config for U-Boot
cat > /tmp/config.txt << 'EOF'
arm_64bit=1
kernel=u-boot.bin
hdmi_force_hotplug=1
hdmi_group=1
hdmi_mode=4
gpu_mem=64
enable_uart=1
EOF
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" /tmp/config.txt ::

# Copy U-Boot and images
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/u-boot.bin" ::
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" "$BUILD_DIR/sel4test-2gb.img" ::sel4test.img

# Create interactive boot script - NO auto-boot, just menu
cat > /tmp/boot.cmd << 'EOF'
echo ""
echo "=========================================="
echo "      seL4 Debug Boot Menu"
echo "=========================================="
echo ""
echo "Board info:"
bdinfo
echo ""
echo "Current EL (Exception Level):"
# Check exception level if possible
echo ""
echo "Commands to try manually:"
echo ""
echo "  1) fatload mmc 0 0x10000000 sel4test.img"
echo "     go 0x10000000"
echo ""
echo "  2) fatload mmc 0 0x10000000 sel4test.img"
echo "     dcache flush"
echo "     icache flush"
echo "     go 0x10000000"
echo ""
echo "  3) fatload mmc 0 0x10000000 sel4test.img"
echo "     booti 0x10000000 - ${fdtaddr}"
echo ""
echo "Type commands at U-Boot prompt below."
echo "=========================================="
EOF

mkimage -A arm64 -T script -C none -d /tmp/boot.cmd /tmp/boot.scr > /dev/null
mcopy -i "$OUTPUT_IMG@@$PART_OFFSET" /tmp/boot.scr ::

rm "$MTOOLSRC" /tmp/config.txt /tmp/boot.cmd /tmp/boot.scr

echo ""
mdir -i "$OUTPUT_IMG@@$PART_OFFSET"
echo ""
echo "=== Debug SD Card Ready ==="
echo "Flash: sudo dd if=$OUTPUT_IMG of=/dev/sdX bs=4M status=progress conv=fsync"
echo ""
echo "This card shows a menu and lets you try commands manually."
