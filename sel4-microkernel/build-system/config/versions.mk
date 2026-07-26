# Pinned versions for reproducible builds

MICROKIT_VERSION := 2.1.0
MICROKIT_SDK_SHA256 := faff1b6d6b546cbb0bfea134588499533130d406ae2a5e533e791ddf23ac7599

RPI_FIRMWARE_TAG := 1.20250915
UBOOT_VERSION := v2025.10

# Raspberry Pi boot firmware SHA-256s at RPI_FIRMWARE_TAG.
# Must match rpi4-graphics/checksums.sha256 (scripts/check-pins.sh enforces this).
RPI_FIRMWARE_START4_SHA256 := 61d198caf99fdf3b82467dc7b5319a6bae1a99fe22c94fdafaa86711490a0c23
RPI_FIRMWARE_FIXUP4_SHA256 := 304b53a7a5a7129531b2674b0fd55240086c5bbd276ff029c63cd68fcf28c0e1
RPI_FIRMWARE_DTB_SHA256 := 4bc9dd6182025add23a750b18ff748e46d4e939434530e8057c6bfa1ce6f1c16

# sha256sum on Linux, shasum on stock macOS
SHA256SUM := $(shell command -v sha256sum >/dev/null 2>&1 && echo sha256sum || echo "shasum -a 256")
