# Product: photoframe (Secure Photo Frame with HDMI output)
#
# Supported platforms: rpi4
# Uses rpi4-photoframe with isolated input handling
#
# Build:
#   make PRODUCT=photoframe PLATFORM=rpi4 sdcard
#
# Security Architecture:
#   - Input PD: UART input handling (isolated from display)
#   - Photoframe PD: Photo decoding and display
#   - Only shared memory: 4KB ring buffer for input events

# Validate platform
ifneq ($(PLATFORM),rpi4)
$(error Product 'photoframe' only supports platform 'rpi4', not '$(PLATFORM)')
endif

# Product info
PRODUCT_NAME := Secure Photo Frame
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-photoframe
INPUT_PD_SRC_DIR := $(ROOT_DIR)/rpi4-input-pd

# Protection Domain names
INPUT_PD_NAME := input_pd
PHOTOFRAME_PD_NAME := photoframe_pd
INPUT_PD_ELF := $(BUILD_DIR)/$(INPUT_PD_NAME).elf
PHOTOFRAME_PD_ELF := $(BUILD_DIR)/$(PHOTOFRAME_PD_NAME).elf

# Primary PD for build system
PD_NAME := $(PHOTOFRAME_PD_NAME)
PD_ELF := $(PHOTOFRAME_PD_ELF)

# System descriptor (two-PD architecture)
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/photoframe.system

# Source files for dependency tracking
PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml \
                   $(wildcard $(INPUT_PD_SRC_DIR)/src/*.rs) \
                   $(INPUT_PD_SRC_DIR)/Cargo.toml \
                   $(wildcard $(ROOT_DIR)/rpi4-input/src/*.rs) \
                   $(wildcard $(ROOT_DIR)/rpi4-input-protocol/src/*.rs) \
                   $(wildcard $(ROOT_DIR)/rpi4-photo-protocol/src/*.rs)

# Output files
SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt

# Build Input PD
$(INPUT_PD_ELF): $(PRODUCT_SOURCES) | $(BUILD_DIR)
	@echo "=== Building $(INPUT_PD_NAME) Protection Domain ($(PLATFORM_ARCH)) ==="
	cd $(INPUT_PD_SRC_DIR) && $(CARGO) build \
		--release \
		--target $(TARGET_SPEC) \
		$(CARGO_BUILD_STD)
	cp $(INPUT_PD_SRC_DIR)/target/$(CARGO_TARGET)/release/$(INPUT_PD_NAME).elf $@
	@echo "Built: $@"

# Build Photoframe PD
$(PHOTOFRAME_PD_ELF): $(PRODUCT_SOURCES) | $(BUILD_DIR)
	@echo "=== Building $(PHOTOFRAME_PD_NAME) Protection Domain ($(PLATFORM_ARCH)) ==="
	cd $(PRODUCT_SRC_DIR) && $(CARGO) build \
		--release \
		--target $(TARGET_SPEC) \
		--bin $(PHOTOFRAME_PD_NAME) \
		$(CARGO_BUILD_STD)
	cp $(PRODUCT_SRC_DIR)/target/$(CARGO_TARGET)/release/$(PHOTOFRAME_PD_NAME).elf $@
	@echo "Built: $@"

# Ensure both PDs are built before system image
.PHONY: build-photoframe-pds
build-photoframe-pds: $(INPUT_PD_ELF) $(PHOTOFRAME_PD_ELF)

$(LOADER_ELF): build-photoframe-pds
