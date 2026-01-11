# Product: tvdemo (TV Demo with input handling)
#
# Supported platforms: rpi4
# Dependencies: rpi4-tvdemo crate, rpi4-input crate

# Validate platform
ifneq ($(PLATFORM),rpi4)
$(error Product 'tvdemo' only supports platform 'rpi4', not '$(PLATFORM)')
endif

# Product info
PRODUCT_NAME := TV Demo
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-spi-display

# Protection domain
PD_NAME := spi_display
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf

# System descriptor (TODO: create this file)
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/spi-display.system

# Source files for dependency tracking
PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(wildcard $(PRODUCT_SRC_DIR)/src/**/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml

# Output files
SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt
