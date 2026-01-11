# Product: rpi4-graphics (HDMI framebuffer demo)
#
# Supported platforms: rpi4

# Validate platform
ifneq ($(PLATFORM),rpi4)
$(error Product 'graphics' only supports platform 'rpi4', not '$(PLATFORM)')
endif

# Product info
PRODUCT_NAME := Graphics Demo
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-graphics

# Protection domain
PD_NAME := graphics_pd
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf

# System descriptor
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/graphics.system

# Source files for dependency tracking
PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml

# Output files
SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt
