# Unified Build System for seL4 Microkit
THIS_MAKEFILE := $(lastword $(MAKEFILE_LIST))
#
# Usage:
#   make PRODUCT=<product> PLATFORM=<platform> [target]
#
# Products:
#   graphics    - RPi4 HDMI framebuffer demo
#   hello       - Hello World for QEMU
#   tvdemo      - TV Demo with input (RPi4)
#
# Platforms:
#   rpi4           - Raspberry Pi 4 hardware
#   qemu-aarch64   - QEMU AArch64 virtual machine
#   qemu-riscv64   - QEMU RISC-V virtual machine
#
# Targets:
#   all          - Build system image (default)
#   run          - Run in QEMU (QEMU platforms only)
#   sdcard       - Create SD card image (rpi4 only)
#   sdcard-uboot - SD card with U-Boot (rpi4 only)
#   write-sdcard - Write image to SD card (requires DEVICE=)
#   clean        - Remove build artifacts
#   help         - Show this message
#
# Examples:
#   make PRODUCT=hello PLATFORM=qemu-aarch64 run
#   make PRODUCT=graphics PLATFORM=rpi4 sdcard
#   make PRODUCT=graphics PLATFORM=rpi4 RPI4_MEMORY=8gb sdcard
#   make PRODUCT=graphics PLATFORM=rpi4 write-sdcard DEVICE=/dev/sdb

# Validate required parameters
ifndef PRODUCT
$(error PRODUCT is required. Use: make PRODUCT=<graphics|hello|tvdemo> PLATFORM=<platform>)
endif

ifndef PLATFORM
$(error PLATFORM is required. Use: make PRODUCT=$(PRODUCT) PLATFORM=<rpi4|qemu-aarch64|qemu-riscv64>)
endif

# Load configurations
include config/defaults.mk
include config/versions.mk

# Load platform configuration
PLATFORM_MK := config/platforms/$(PLATFORM).mk
ifeq ($(wildcard $(PLATFORM_MK)),)
$(error Unknown platform: $(PLATFORM). Available: rpi4, qemu-aarch64, qemu-riscv64)
endif
include $(PLATFORM_MK)

# Load product configuration
PRODUCT_MK := config/products/$(PRODUCT).mk
ifeq ($(wildcard $(PRODUCT_MK)),)
$(error Unknown product: $(PRODUCT). Available: graphics, hello, tvdemo)
endif
include $(PRODUCT_MK)

# Load build rules
include include/rust.mk
include include/microkit.mk
include include/qemu.mk
include include/sdcard.mk

# Default target
.PHONY: all
all: $(SYSTEM_IMAGE)
	@echo ""
	@echo "=== Build Complete ==="
	@echo "Product:  $(PRODUCT_NAME)"
	@echo "Platform: $(PLATFORM)"
	@echo "Board:    $(MICROKIT_BOARD)"
	@echo "Image:    $(SYSTEM_IMAGE)"

# Show configuration
.PHONY: info
info:
	@echo "=== Build Configuration ==="
	@echo "Product:       $(PRODUCT) ($(PRODUCT_NAME))"
	@echo "Platform:      $(PLATFORM)"
	@echo "Board:         $(MICROKIT_BOARD)"
	@echo "Architecture:  $(PLATFORM_ARCH)"
	@echo "Microkit SDK:  $(MICROKIT_SDK)"
	@echo "Build dir:     $(BUILD_DIR)"
	@echo "Target spec:   $(TARGET_SPEC)"
	@echo ""
	@echo "Source: $(PRODUCT_SRC_DIR)"
	@echo "System: $(SYSTEM_DESC)"

# Clean build artifacts
.PHONY: clean
clean:
	rm -rf $(BUILD_DIR)

# Deep clean including cargo cache
.PHONY: distclean
distclean: clean clean-rust
	rm -rf $(ROOT_DIR)/target

# Help
.PHONY: help
help:
	@head -30 $(THIS_MAKEFILE) | grep "^#" | cut -c3-

# Ensure loader.elf is also built
.PHONY: elf
elf: $(LOADER_ELF)

# Check all prerequisites
.PHONY: check
check: check-sdk
	@echo "Product: $(PRODUCT)"
	@echo "Platform: $(PLATFORM)"
	@echo "Ready to build!"
