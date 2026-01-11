# Default configuration and OS detection

# Directories (relative to build-system/)
BUILD_SYSTEM_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/..)
ROOT_DIR := $(abspath $(BUILD_SYSTEM_DIR)/..)
BUILD_DIR := $(ROOT_DIR)/build/$(PLATFORM)/$(PRODUCT)
TARGETS_DIR := $(BUILD_SYSTEM_DIR)/targets

# Microkit SDK location
MICROKIT_SDK ?= $(ROOT_DIR)/microkit-sdk
MICROKIT_TOOL := $(MICROKIT_SDK)/bin/microkit

# Detect OS for cross-compiler selection
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
    AARCH64_PREFIX := aarch64-elf-
    RISCV64_PREFIX := riscv64-elf-
    NPROC := $(shell sysctl -n hw.ncpu)
else
    AARCH64_PREFIX := aarch64-linux-gnu-
    RISCV64_PREFIX := riscv64-linux-gnu-
    NPROC := $(shell nproc)
endif

# Cargo settings
# Detect cargo bin directory (handles PATH not including ~/.cargo/bin)
CARGO_HOME ?= $(HOME)/.cargo
CARGO_BIN := $(CARGO_HOME)/bin
ifneq ($(wildcard $(CARGO_BIN)/rustup),)
    RUSTUP := $(CARGO_BIN)/rustup
    CARGO := $(RUSTUP) run nightly cargo
else
    # Fall back to PATH-based lookup
    RUSTUP := rustup
    CARGO := rustup run nightly cargo
endif
CARGO_BUILD_STD := -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem

# Default Microkit configuration
MICROKIT_CONFIG ?= debug

# Shared submodule paths
# Support both new location (vendor/) and legacy location (rpi4-graphics/vendor/)
VENDOR_DIR := $(ROOT_DIR)/vendor
ifneq ($(wildcard $(VENDOR_DIR)/u-boot),)
    UBOOT_DIR := $(VENDOR_DIR)/u-boot
else
    UBOOT_DIR := $(ROOT_DIR)/rpi4-graphics/vendor/u-boot
endif

# Scripts directory
SCRIPTS_DIR := $(BUILD_SYSTEM_DIR)/scripts
