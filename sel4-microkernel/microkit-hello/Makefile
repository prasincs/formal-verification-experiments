# seL4 Microkit Build System
#
# Builds a bootable seL4 Microkit system for AArch64 or RISC-V
#
# Usage:
#   make ARCH=aarch64    # Build for AArch64 (QEMU virt)
#   make ARCH=riscv64    # Build for RISC-V 64-bit (QEMU virt)
#   make clean           # Clean build artifacts

# Default architecture
ARCH ?= aarch64

# Microkit SDK location (set by setup.sh)
MICROKIT_SDK ?= $(PWD)/microkit-sdk

# Board configuration
ifeq ($(ARCH),aarch64)
    MICROKIT_BOARD := qemu_virt_aarch64
    MICROKIT_CONFIG := debug
    RUST_TARGET := aarch64-unknown-none
    CROSS_COMPILE := aarch64-linux-gnu-
    QEMU := qemu-system-aarch64
    QEMU_MACHINE := virt,virtualization=on
    QEMU_CPU := cortex-a53
else ifeq ($(ARCH),riscv64)
    MICROKIT_BOARD := qemu_virt_riscv64
    MICROKIT_CONFIG := debug
    RUST_TARGET := riscv64gc-unknown-none-elf
    CROSS_COMPILE := riscv64-linux-gnu-
    QEMU := qemu-system-riscv64
    QEMU_MACHINE := virt
    QEMU_CPU := rv64
else
    $(error Unsupported ARCH=$(ARCH). Use aarch64 or riscv64)
endif

# Microkit tool
MICROKIT_TOOL := $(MICROKIT_SDK)/bin/microkit

# Output directories
BUILD_DIR := build/$(ARCH)
RUST_BUILD := target/$(RUST_TARGET)/release

# Protection domain sources
PD_NAME := hello
PD_SRC := src/lib.rs

# Output files
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf
SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
REPORT := $(BUILD_DIR)/report.txt

# Compiler flags for Rust
export RUSTFLAGS := -C link-arg=-T$(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/$(MICROKIT_CONFIG)/pd.ld

.PHONY: all clean run check

all: $(SYSTEM_IMAGE)

# Build the Rust protection domain
$(BUILD_DIR)/lib$(PD_NAME).a: $(PD_SRC) Cargo.toml
	@mkdir -p $(BUILD_DIR)
	@echo "Building Rust protection domain for $(ARCH)..."
	cargo +nightly build \
		--release \
		--target $(RUST_TARGET) \
		-Z build-std=core,alloc \
		-Z build-std-features=compiler-builtins-mem
	cp $(RUST_BUILD)/lib$(PD_NAME).a $@

# Link the protection domain ELF
$(PD_ELF): $(BUILD_DIR)/lib$(PD_NAME).a
	@echo "Linking protection domain..."
	$(CROSS_COMPILE)ld \
		-T$(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/$(MICROKIT_CONFIG)/pd.ld \
		$(BUILD_DIR)/lib$(PD_NAME).a \
		-o $@

# Build the system image using Microkit tool
$(SYSTEM_IMAGE): $(PD_ELF) hello.system
	@echo "Building Microkit system image..."
	$(MICROKIT_TOOL) \
		hello.system \
		--search-path $(BUILD_DIR) \
		--board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) \
		-o $@ \
		-r $(REPORT)
	@echo ""
	@echo "Build complete!"
	@echo "  System image: $@"
	@echo "  Report: $(REPORT)"
	@echo ""
	@echo "Run with: make run ARCH=$(ARCH)"

# Run in QEMU
run: $(SYSTEM_IMAGE)
	@echo "Booting seL4 Microkit in QEMU ($(ARCH))..."
	@echo "Press Ctrl-A X to exit"
	@echo ""
ifeq ($(ARCH),aarch64)
	$(QEMU) \
		-machine $(QEMU_MACHINE) \
		-cpu $(QEMU_CPU) \
		-m 2G \
		-nographic \
		-device loader,file=$<,addr=0x70000000,cpu-num=0
else ifeq ($(ARCH),riscv64)
	$(QEMU) \
		-machine $(QEMU_MACHINE) \
		-cpu $(QEMU_CPU) \
		-m 2G \
		-nographic \
		-bios default \
		-kernel $<
endif

# Check if SDK is set up
check:
	@if [ ! -d "$(MICROKIT_SDK)" ]; then \
		echo "Error: Microkit SDK not found at $(MICROKIT_SDK)"; \
		echo "Run ./setup.sh first"; \
		exit 1; \
	fi
	@echo "Microkit SDK: $(MICROKIT_SDK)"
	@echo "Board: $(MICROKIT_BOARD)"
	@echo "Architecture: $(ARCH)"

clean:
	rm -rf build target
	cargo clean

# Show help
help:
	@echo "seL4 Microkit Build System"
	@echo ""
	@echo "Usage:"
	@echo "  make ARCH=aarch64    Build for AArch64"
	@echo "  make ARCH=riscv64    Build for RISC-V 64-bit"
	@echo "  make run ARCH=...    Build and run in QEMU"
	@echo "  make clean           Clean build artifacts"
	@echo "  make check           Verify SDK setup"
	@echo ""
	@echo "Prerequisites:"
	@echo "  ./setup.sh           Download Microkit SDK"
