# Product: tvdemo (TV Demo with HDMI output)
#
# Supported platforms: rpi4
# Uses rpi4-graphics with HDMI framebuffer backend

# Validate platform
ifneq ($(PLATFORM),rpi4)
$(error Product 'tvdemo' only supports platform 'rpi4', not '$(PLATFORM)')
endif

# Product info
PRODUCT_NAME := TV Demo (HDMI)
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-graphics

# Protection domain - use tvdemo_pd binary from rpi4-graphics
PD_NAME := tvdemo_pd
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf

# System descriptor
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/tvdemo.system

# Source files for dependency tracking
PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml \
                   $(wildcard $(ROOT_DIR)/rpi4-tvdemo/src/*.rs) \
                   $(wildcard $(ROOT_DIR)/rpi4-input/src/*.rs)

# Output files
SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt
