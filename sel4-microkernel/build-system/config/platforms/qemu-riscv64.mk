# Platform: QEMU RISC-V 64-bit Virtual Machine

PLATFORM_ARCH := riscv64
CROSS_COMPILE := $(RISCV64_PREFIX)
MICROKIT_BOARD := qemu_virt_riscv64

# Target spec for Rust
TARGET_SPEC := $(TARGETS_DIR)/riscv64gc-sel4-microkit.json
CARGO_TARGET := riscv64gc-sel4-microkit

# QEMU settings
QEMU := qemu-system-riscv64
QEMU_MACHINE := virt
QEMU_CPU := rv64
QEMU_MEMORY := 2G

# QEMU platform
IS_QEMU := true

# Platform-specific QEMU args (RISC-V needs bios)
QEMU_EXTRA_ARGS := -bios default
