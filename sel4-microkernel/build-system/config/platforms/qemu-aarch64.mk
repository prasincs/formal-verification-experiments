# Platform: QEMU AArch64 Virtual Machine

PLATFORM_ARCH := aarch64
CROSS_COMPILE := $(AARCH64_PREFIX)
MICROKIT_BOARD := qemu_virt_aarch64

# Target spec for Rust
TARGET_SPEC := $(TARGETS_DIR)/aarch64-sel4-microkit.json
CARGO_TARGET := aarch64-sel4-microkit

# QEMU settings
QEMU := qemu-system-aarch64
QEMU_MACHINE := virt,virtualization=on
QEMU_CPU := cortex-a53
QEMU_MEMORY := 2G
QEMU_LOADER_ADDR := 0x70000000

# QEMU platform
IS_QEMU := true

# Platform-specific QEMU args
QEMU_EXTRA_ARGS :=
