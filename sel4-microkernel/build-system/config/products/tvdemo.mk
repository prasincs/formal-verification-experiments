# Product: tvdemo (TV Demo with HDMI output)
#
# Supported platforms: rpi4
# Uses rpi4-graphics with HDMI framebuffer backend
#
# Build modes (set ISOLATED=1 for two-PD architecture):
#   make PRODUCT=tvdemo PLATFORM=rpi4              - Single PD (UART in graphics PD)
#   make PRODUCT=tvdemo PLATFORM=rpi4 ISOLATED=1   - Isolated PDs (Input PD + Graphics PD)

# Validate platform
ifneq ($(PLATFORM),rpi4)
$(error Product 'tvdemo' only supports platform 'rpi4', not '$(PLATFORM)')
endif

# Product info
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-graphics
INPUT_PD_SRC_DIR := $(ROOT_DIR)/rpi4-input-pd

# Check for isolated mode
ifdef ISOLATED
PRODUCT_NAME := TV Demo (Isolated Input)

# Two-PD mode: Input PD + Graphics PD
INPUT_PD_NAME := input_pd
GRAPHICS_PD_NAME := graphics_input_pd
INPUT_PD_ELF := $(BUILD_DIR)/$(INPUT_PD_NAME).elf
GRAPHICS_PD_ELF := $(BUILD_DIR)/$(GRAPHICS_PD_NAME).elf

# Primary PD for build system
PD_NAME := $(GRAPHICS_PD_NAME)
PD_ELF := $(GRAPHICS_PD_ELF)

# System descriptor (two-PD version)
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/tvdemo-input.system

# Source files including input-pd and protocol
PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml \
                   $(wildcard $(INPUT_PD_SRC_DIR)/src/*.rs) \
                   $(INPUT_PD_SRC_DIR)/Cargo.toml \
                   $(wildcard $(ROOT_DIR)/rpi4-tvdemo/src/*.rs) \
                   $(wildcard $(ROOT_DIR)/rpi4-input/src/*.rs) \
                   $(wildcard $(ROOT_DIR)/rpi4-input-protocol/src/*.rs)

else
PRODUCT_NAME := TV Demo (HDMI)

# Single PD mode: tvdemo_pd only
PD_NAME := tvdemo_pd
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf

# System descriptor (single PD)
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/tvdemo.system

# Source files for dependency tracking
PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml \
                   $(wildcard $(ROOT_DIR)/rpi4-tvdemo/src/*.rs) \
                   $(wildcard $(ROOT_DIR)/rpi4-input/src/*.rs)
endif

# Output files
SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt

# Additional rules for isolated mode
ifdef ISOLATED
# Build Input PD
$(INPUT_PD_ELF): $(PRODUCT_SOURCES) | $(BUILD_DIR)
	@echo "=== Building $(INPUT_PD_NAME) Protection Domain ($(PLATFORM_ARCH)) ==="
	cd $(INPUT_PD_SRC_DIR) && $(CARGO) build \
		--release \
		--target $(TARGET_SPEC) \
		$(CARGO_BUILD_STD)
	cp $(INPUT_PD_SRC_DIR)/target/$(CARGO_TARGET)/release/$(INPUT_PD_NAME).elf $@
	@echo "Built: $@"

# Build Graphics PD with IPC
$(GRAPHICS_PD_ELF): $(PRODUCT_SOURCES) | $(BUILD_DIR)
	@echo "=== Building $(GRAPHICS_PD_NAME) Protection Domain ($(PLATFORM_ARCH)) ==="
	cd $(PRODUCT_SRC_DIR) && $(CARGO) build \
		--release \
		--target $(TARGET_SPEC) \
		--bin $(GRAPHICS_PD_NAME) \
		$(CARGO_BUILD_STD)
	cp $(CARGO_TARGET_DIR)/$(CARGO_TARGET)/release/$(GRAPHICS_PD_NAME).elf $@
	@echo "Built: $@"

# Ensure both PDs are built before system image
.PHONY: build-isolated-pds
build-isolated-pds: $(INPUT_PD_ELF) $(GRAPHICS_PD_ELF)

$(LOADER_ELF): build-isolated-pds
endif
