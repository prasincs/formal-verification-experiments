# Platform: Raspberry Pi 4 Hardware

PLATFORM_ARCH := aarch64
CROSS_COMPILE := $(AARCH64_PREFIX)

# Memory variant (1gb, 2gb, 4gb, 8gb)
# Default to 8gb - use RPI4_MEMORY=4gb for smaller variants
RPI4_MEMORY ?= 8gb
MICROKIT_BOARD := rpi4b_$(RPI4_MEMORY)

# Target spec for Rust
TARGET_SPEC := $(TARGETS_DIR)/aarch64-sel4-microkit.json
CARGO_TARGET := aarch64-sel4-microkit

# Firmware locations
FIRMWARE_DIR := $(BUILD_DIR)/firmware
FIRMWARE_BASE_URL := https://github.com/raspberrypi/firmware/raw/$(RPI_FIRMWARE_TAG)/boot

# SD card image settings
SDCARD_IMG := $(BUILD_DIR)/rpi4-sel4-$(PRODUCT).img
SDCARD_SIZE_MB := 64

# U-Boot output
UBOOT_BIN := $(BUILD_DIR)/u-boot.bin

# Platform-specific flags
PLATFORM_FLAGS :=

# Not a QEMU platform
IS_QEMU := false
