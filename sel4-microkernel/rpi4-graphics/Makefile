# seL4 Microkit Build System - Raspberry Pi 4 Graphics
#
# Builds a bootable SD card image for Raspberry Pi 4 with seL4 Microkit.
#
# Usage:
#   make                  # Build everything
#   make sdcard           # Create bootable SD card image (direct boot)
#   make sdcard-uboot     # Build + create SD card with U-Boot & bootelf
#   make elf              # Build loader.elf for bootelf command
#   make clean            # Clean build artifacts
#
# Prerequisites:
#   - Microkit SDK with rpi4b support
#   - Rust nightly with aarch64 target
#   - aarch64-linux-gnu-gcc (or aarch64-elf-gcc on macOS)

# Configuration
MICROKIT_SDK ?= $(PWD)/../microkit-sdk

# Raspberry Pi 4 memory variant (1gb, 2gb, 4gb, 8gb)
RPI4_MEMORY ?= 4gb
MICROKIT_BOARD := rpi4b_$(RPI4_MEMORY)
MICROKIT_CONFIG := debug

# Pinned versions for reproducible builds
MICROKIT_VERSION := 2.1.0
MICROKIT_SDK_SHA256 := faff1b6d6b546cbb0bfea134588499533130d406ae2a5e533e791ddf23ac7599
RPI_FIRMWARE_TAG := 1.20250915
UBOOT_VERSION := v2025.10

# Detect OS for cross-compiler
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
    CROSS_COMPILE := aarch64-elf-
else
    CROSS_COMPILE := aarch64-linux-gnu-
endif

# Tools
MICROKIT_TOOL := $(MICROKIT_SDK)/bin/microkit
CARGO := cargo +nightly

# Directories
BUILD_DIR := build
SDK_BOARD := $(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/$(MICROKIT_CONFIG)

# U-Boot submodule directory (must be after BUILD_DIR)
UBOOT_DIR := vendor/u-boot
UBOOT_BIN := $(BUILD_DIR)/u-boot.bin

# Output files
PD_ELF := $(BUILD_DIR)/graphics_pd.elf
SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
REPORT := $(BUILD_DIR)/report.txt
SDCARD_IMG := $(BUILD_DIR)/rpi4-sel4-graphics.img

# Environment
export SEL4_INCLUDE_DIRS := $(SDK_BOARD)/include

.PHONY: all clean sdcard sdcard-uboot firmware check help uboot elf

all: $(SYSTEM_IMAGE)

# Custom target for seL4 Microkit (absolute path for reliability)
TARGET_JSON := $(PWD)/support/targets/aarch64-sel4-microkit.json

# Build the Rust protection domain
$(PD_ELF): src/main.rs src/lib.rs Cargo.toml | $(BUILD_DIR)
	@echo "=== Building Graphics Protection Domain ==="
	$(CARGO) build \
		--release \
		--target $(TARGET_JSON) \
		-Z build-std=core,alloc,compiler_builtins \
		-Z build-std-features=compiler-builtins-mem
	@mkdir -p $(BUILD_DIR)
	@# sel4-microkit produces .elf files
	cp target/aarch64-sel4-microkit/release/graphics_pd.elf $@
	@echo "Built: $@"

$(BUILD_DIR):
	mkdir -p $@

# Build system image with Microkit tool (binary format for go command)
$(SYSTEM_IMAGE): $(PD_ELF) graphics.system | check
	@echo "=== Building Microkit System Image ==="
	$(MICROKIT_TOOL) \
		graphics.system \
		--search-path $(BUILD_DIR) \
		--board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) \
		-o $@ \
		-r $(REPORT)
	@echo ""
	@echo "Build complete!"
	@echo "  System image: $@"
	@echo "  Report: $(REPORT)"

# Also build ELF format for bootelf command
LOADER_ELF := $(BUILD_DIR)/loader.elf

$(LOADER_ELF): $(PD_ELF) graphics.system | check
	@echo "=== Building Microkit Loader ELF ==="
	$(MICROKIT_TOOL) \
		graphics.system \
		--search-path $(BUILD_DIR) \
		--board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) \
		--image-type elf \
		-o $@
	@echo "Built: $@"

.PHONY: elf
elf: $(LOADER_ELF)

# Download Raspberry Pi firmware files (pinned to specific tag)
FIRMWARE_DIR := $(BUILD_DIR)/firmware
FIRMWARE_BASE_URL := https://github.com/raspberrypi/firmware/raw/$(RPI_FIRMWARE_TAG)/boot

$(FIRMWARE_DIR):
	@echo "=== Downloading Raspberry Pi 4 Firmware ($(RPI_FIRMWARE_TAG)) ==="
	mkdir -p $@
	# Download from official Raspberry Pi firmware repository (pinned tag)
	curl -L -o $@/start4.elf $(FIRMWARE_BASE_URL)/start4.elf
	curl -L -o $@/fixup4.dat $(FIRMWARE_BASE_URL)/fixup4.dat
	curl -L -o $@/bcm2711-rpi-4-b.dtb $(FIRMWARE_BASE_URL)/bcm2711-rpi-4-b.dtb
	@echo "Firmware downloaded to $@"
	@echo "Note: Run 'sha256sum $@/*' to verify checksums"

firmware: $(FIRMWARE_DIR)

# Build U-Boot from submodule (pinned to $(UBOOT_VERSION))
$(UBOOT_BIN): | $(BUILD_DIR)
	@echo "=== Building U-Boot $(UBOOT_VERSION) for Raspberry Pi 4 ==="
	@if [ ! -d "$(UBOOT_DIR)/.git" ]; then \
		echo "Initializing U-Boot submodule..."; \
		git submodule update --init $(UBOOT_DIR); \
	fi
	cd $(UBOOT_DIR) && git checkout $(UBOOT_VERSION)
	$(MAKE) -C $(UBOOT_DIR) CROSS_COMPILE=$(CROSS_COMPILE) rpi_4_defconfig
	$(MAKE) -C $(UBOOT_DIR) CROSS_COMPILE=$(CROSS_COMPILE) -j$$(nproc)
	cp $(UBOOT_DIR)/u-boot.bin $@
	@echo "U-Boot built: $@"

uboot: $(UBOOT_BIN)

# Create config.txt for Raspberry Pi
$(BUILD_DIR)/config.txt:
	@echo "=== Creating config.txt ==="
	@mkdir -p $(BUILD_DIR)
	@echo "# seL4 Microkit on Raspberry Pi 4" > $@
	@echo "arm_64bit=1" >> $@
	@echo "kernel=loader.img" >> $@
	@echo "kernel_address=0x10000000" >> $@
	@echo "" >> $@
	@echo "# Display settings" >> $@
	@echo "hdmi_force_hotplug=1" >> $@
	@echo "hdmi_group=2" >> $@
	@echo "hdmi_mode=82" >> $@
	@echo "disable_overscan=1" >> $@
	@echo "" >> $@
	@echo "# GPU memory (minimum for framebuffer)" >> $@
	@echo "gpu_mem=64" >> $@
	@echo "" >> $@
	@echo "# Enable UART for debug output" >> $@
	@echo "enable_uart=1" >> $@
	@echo "uart_2ndstage=1" >> $@
	@echo "Created: $@"

# Create bootable SD card image using mtools (no root required)
# Works on both local machines and CI
$(SDCARD_IMG): $(SYSTEM_IMAGE) $(FIRMWARE_DIR) $(BUILD_DIR)/config.txt
	@echo "=== Creating SD Card Image ==="
	@echo "Creating 64MB FAT32 image..."
	dd if=/dev/zero of=$@ bs=1M count=64 2>/dev/null
	mkfs.vfat -F 32 -n SEL4BOOT $@
	@echo "Copying boot files with mtools..."
	mcopy -i $@ $(FIRMWARE_DIR)/start4.elf ::
	mcopy -i $@ $(FIRMWARE_DIR)/fixup4.dat ::
	mcopy -i $@ $(FIRMWARE_DIR)/bcm2711-rpi-4-b.dtb ::
	mcopy -i $@ $(BUILD_DIR)/config.txt ::
	mcopy -i $@ $(SYSTEM_IMAGE) ::loader.img
	@echo ""
	@echo "=== SD Card Image Ready ==="
	@echo "Image: $@"
	@echo ""
	@echo "Flash to SD card with:"
	@echo "  sudo dd if=$@ of=/dev/sdX bs=4M status=progress conv=fsync"
	@echo ""
	@echo "Or use Raspberry Pi Imager:"
	@echo "  - Select 'Use custom' and choose: $@"

sdcard: $(SDCARD_IMG)

# Build everything and create SD card with U-Boot + bootelf support
sdcard-uboot: $(SYSTEM_IMAGE) $(LOADER_ELF) uboot
	@echo "=== Creating SD Card Image with U-Boot ==="
	./scripts/create-sdcard-full.sh --uboot

# Alternative: Create a directory with all boot files (no root required)
bootfiles: $(SYSTEM_IMAGE) $(FIRMWARE_DIR) $(BUILD_DIR)/config.txt
	@echo "=== Creating Boot Files Directory ==="
	mkdir -p $(BUILD_DIR)/boot
	cp $(FIRMWARE_DIR)/start4.elf $(BUILD_DIR)/boot/
	cp $(FIRMWARE_DIR)/fixup4.dat $(BUILD_DIR)/boot/
	cp $(FIRMWARE_DIR)/bcm2711-rpi-4-b.dtb $(BUILD_DIR)/boot/
	cp $(BUILD_DIR)/config.txt $(BUILD_DIR)/boot/
	cp $(SYSTEM_IMAGE) $(BUILD_DIR)/boot/loader.img
	@echo ""
	@echo "Boot files created in: $(BUILD_DIR)/boot/"
	@echo ""
	@echo "To use:"
	@echo "1. Format an SD card as FAT32"
	@echo "2. Copy all files from $(BUILD_DIR)/boot/ to the SD card root"
	@echo "3. Insert SD card into Raspberry Pi 4 and power on"

# Check prerequisites
check:
	@if [ ! -d "$(MICROKIT_SDK)" ]; then \
		echo "Error: Microkit SDK not found at $(MICROKIT_SDK)"; \
		echo ""; \
		echo "To set up the SDK:"; \
		echo "1. Download from https://github.com/seL4/microkit/releases"; \
		echo "2. Extract and set MICROKIT_SDK=/path/to/sdk"; \
		echo ""; \
		echo "Or run: make setup"; \
		exit 1; \
	fi
	@if [ ! -d "$(SDK_BOARD)" ]; then \
		echo "Error: Board $(MICROKIT_BOARD) not found in SDK"; \
		echo "Available boards:"; \
		ls $(MICROKIT_SDK)/board/ 2>/dev/null || echo "  (none)"; \
		exit 1; \
	fi
	@echo "SDK: $(MICROKIT_SDK)"
	@echo "Board: $(MICROKIT_BOARD)"

# Download and set up Microkit SDK with checksum verification
setup:
	@echo "=== Setting up Microkit SDK $(MICROKIT_VERSION) ==="
	mkdir -p $(MICROKIT_SDK)
	curl -L -o /tmp/microkit-sdk.tar.gz \
		https://github.com/seL4/microkit/releases/download/$(MICROKIT_VERSION)/microkit-sdk-$(MICROKIT_VERSION)-linux-x86-64.tar.gz
	@echo "Verifying SHA256 checksum..."
	@echo "$(MICROKIT_SDK_SHA256)  /tmp/microkit-sdk.tar.gz" | sha256sum -c -
	tar -xzf /tmp/microkit-sdk.tar.gz -C $(MICROKIT_SDK) --strip-components=1
	rm /tmp/microkit-sdk.tar.gz
	@echo "SDK installed at: $(MICROKIT_SDK)"

clean:
	rm -rf build target
	cargo clean

help:
	@echo "seL4 Microkit Graphics - Raspberry Pi 4"
	@echo ""
	@echo "Usage:"
	@echo "  make                Build system image"
	@echo "  make sdcard         Create bootable SD card image"
	@echo "  make bootfiles      Create boot files directory (no root required)"
	@echo "  make firmware       Download Raspberry Pi firmware"
	@echo "  make uboot          Build U-Boot bootloader from submodule"
	@echo "  make setup          Download Microkit SDK"
	@echo "  make check          Verify prerequisites"
	@echo "  make clean          Clean build artifacts"
	@echo ""
	@echo "Configuration:"
	@echo "  RPI4_MEMORY=4gb     Set Pi 4 memory variant (1gb/2gb/4gb/8gb)"
	@echo "  MICROKIT_SDK=path   Path to Microkit SDK"
	@echo ""
	@echo "Example:"
	@echo "  make RPI4_MEMORY=8gb bootfiles"
