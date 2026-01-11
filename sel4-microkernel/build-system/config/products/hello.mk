# Product: microkit-hello (Hello World demo)
#
# Supported platforms: qemu-aarch64, qemu-riscv64

# Validate platform
ifeq ($(filter qemu-aarch64 qemu-riscv64,$(PLATFORM)),)
$(error Product 'hello' only supports QEMU platforms (qemu-aarch64, qemu-riscv64), not '$(PLATFORM)')
endif

# Product info
PRODUCT_NAME := Hello World Demo
PRODUCT_SRC_DIR := $(ROOT_DIR)/microkit-hello

# Protection domain
PD_NAME := hello
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf

# System descriptor
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/hello.system

# Source files for dependency tracking
PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml

# Output files
SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt
