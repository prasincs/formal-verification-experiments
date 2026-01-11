# Platform: x86_64 (Future)
#
# This platform is not yet fully supported by Microkit SDK.
# Placeholder for future expansion.

PLATFORM_ARCH := x86_64
CROSS_COMPILE :=
MICROKIT_BOARD := pc99

# Target spec (to be created)
TARGET_SPEC := $(TARGETS_DIR)/x86_64-sel4-microkit.json
CARGO_TARGET := x86_64-sel4-microkit

# QEMU settings
QEMU := qemu-system-x86_64
QEMU_MACHINE := q35
QEMU_CPU := host
QEMU_MEMORY := 2G

IS_QEMU := true
QEMU_EXTRA_ARGS := -enable-kvm

$(error x86_64 platform is not yet implemented)
