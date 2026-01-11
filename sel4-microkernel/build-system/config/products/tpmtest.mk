# Product: tpmtest (TPM Boot Verification Test)
#
# Supported platforms: rpi4
# Tests TPM 2.0 module (GeeekPi TPM9670 / Infineon SLB 9670) via HDMI output
#
# This product displays TPM status on screen since GPIO pins are occupied
# by the TPM module and serial debug is not available.
#
# Hardware requirements:
#   - Raspberry Pi 4
#   - GeeekPi TPM9670 module (on SPI0, GPIO 7-11)
#   - HDMI display
#
# Build:
#   make PRODUCT=tpmtest PLATFORM=rpi4 sdcard

# Validate platform
ifneq ($(PLATFORM),rpi4)
$(error Product 'tpmtest' only supports platform 'rpi4', not '$(PLATFORM)')
endif

# Product info
PRODUCT_NAME := TPM Boot Verification Test
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-graphics

# Single PD with TPM + Graphics
PD_NAME := tpmtest_pd
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf

# System descriptor
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/tpmtest.system

# Source files for dependency tracking
PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml \
                   $(wildcard $(ROOT_DIR)/rpi4-tpm-boot/src/*.rs) \
                   $(ROOT_DIR)/rpi4-tpm-boot/Cargo.toml

# Output files
SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt
