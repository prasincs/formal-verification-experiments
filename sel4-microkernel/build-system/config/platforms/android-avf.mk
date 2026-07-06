# Platform: Android device — AVF (crosvm) / Termux (QEMU)
#
# Runs the seL4 agent OS image as an isolated guest layer on an Android
# device. One image, three execution venues:
#
#   1. Host QEMU        (make run)           — works today; identical
#      board to qemu-aarch64, kept here for parity testing.
#   2. Termux QEMU      (make termux-bundle) — works today; software
#      emulation of the same `virt` board on the device itself.
#   3. AVF crosvm       (make run-avf)       — deployment path is
#      scripted, but booting this image under crosvm requires a
#      Microkit board port for crosvm's machine model. See
#      docs/android-agent-os.md ("crosvm bring-up status") before
#      expecting output from venue 3.
#
# The image is built for the qemu_virt_aarch64 Microkit board because
# that is the closest SDK board to crosvm's aarch64 machine (GIC +
# virtio + UART), and because it makes venues 1 and 2 exact.

PLATFORM_ARCH := aarch64
CROSS_COMPILE := $(AARCH64_PREFIX)
MICROKIT_BOARD := qemu_virt_aarch64

# Target spec for Rust
TARGET_SPEC := $(TARGETS_DIR)/aarch64-sel4-microkit.json
CARGO_TARGET := aarch64-sel4-microkit

# Host-side QEMU parity settings (same board the image is linked for).
# Keep in sync with config/platforms/qemu-aarch64.mk and with
# scripts/android/run-termux-qemu.sh, which boots the identical
# machine on the device.
QEMU := qemu-system-aarch64
QEMU_MACHINE := virt,virtualization=on
QEMU_CPU := cortex-a53
QEMU_MEMORY := 2G
QEMU_LOADER_ADDR := 0x70000000
QEMU_EXTRA_ARGS :=
IS_QEMU := true

# Android deployment (include/android.mk)
IS_ANDROID := true

# adb binary and optional device serial (-s) for multi-device hosts
ADB ?= adb
ANDROID_SERIAL ?=

# Where the image and launcher are staged on the device. Must be a
# path an unprivileged adb shell can write and root can execute from.
ANDROID_STAGE_DIR ?= /data/local/tmp/sel4-agent

# crosvm from the virtualization APEX (Android 14+ with AVF)
CROSVM_BIN ?= /apex/com.android.virt/bin/crosvm

# Guest resources for the crosvm launch
AVF_MEMORY_MB ?= 1024
AVF_CPUS ?= 1
