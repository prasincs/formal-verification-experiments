#!/bin/bash
# Check that the pinned versions duplicated across the tree all agree.
#
# Sources of truth:
#   sel4-microkernel/build-system/config/versions.mk  (SDK, firmware, U-Boot)
#   sel4-microkernel/rust-toolchain.toml              (Rust nightly)
#
# Everything else — workflow env blocks, Containerfile ARGs, the standalone
# rpi4-graphics Makefile, checksums.sha256, helper scripts — must match, or
# CI would build/attest something different from developer builds.
#
# Usage: ./scripts/check-pins.sh   (exits non-zero on any mismatch)

set -u

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

VERSIONS_MK="sel4-microkernel/build-system/config/versions.mk"
TOOLCHAIN_TOML="sel4-microkernel/rust-toolchain.toml"

mk_var() { sed -n "s/^$1 := //p" "$VERSIONS_MK"; }

MICROKIT_VERSION="$(mk_var MICROKIT_VERSION)"
MICROKIT_SDK_SHA256="$(mk_var MICROKIT_SDK_SHA256)"
RPI_FIRMWARE_TAG="$(mk_var RPI_FIRMWARE_TAG)"
UBOOT_VERSION="$(mk_var UBOOT_VERSION)"
FW_START4="$(mk_var RPI_FIRMWARE_START4_SHA256)"
FW_FIXUP4="$(mk_var RPI_FIRMWARE_FIXUP4_SHA256)"
FW_DTB="$(mk_var RPI_FIRMWARE_DTB_SHA256)"
RUST_TOOLCHAIN="$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$TOOLCHAIN_TOML")"

FAIL=0
fail() { echo "PIN MISMATCH: $*" >&2; FAIL=1; }

for v in MICROKIT_VERSION MICROKIT_SDK_SHA256 RPI_FIRMWARE_TAG UBOOT_VERSION \
         FW_START4 FW_FIXUP4 FW_DTB RUST_TOOLCHAIN; do
    if [ -z "${!v}" ]; then
        fail "could not extract $v from its source of truth"
    fi
done
[ "$FAIL" -eq 0 ] || exit 1

echo "Sources of truth:"
echo "  MICROKIT_VERSION    = $MICROKIT_VERSION"
echo "  MICROKIT_SDK_SHA256 = $MICROKIT_SDK_SHA256"
echo "  RPI_FIRMWARE_TAG    = $RPI_FIRMWARE_TAG"
echo "  UBOOT_VERSION       = $UBOOT_VERSION"
echo "  RUST_TOOLCHAIN      = $RUST_TOOLCHAIN"
echo ""

# check_value FILE PATTERN EXPECTED
# Every line of FILE matching PATTERN must contain EXPECTED.
check_value() {
    local file="$1" pattern="$2" expected="$3" lines
    lines="$(grep -E "$pattern" "$file" 2>/dev/null || true)"
    if [ -z "$lines" ]; then
        fail "$file: expected a line matching '$pattern', found none"
        return
    fi
    while IFS= read -r line; do
        case "$line" in
            *"$expected"*) ;;
            *) fail "$file: '$line' does not contain expected '$expected'" ;;
        esac
    done <<< "$lines"
}

# Standalone rpi4-graphics Makefile carries its own copy of every pin
GRAPHICS_MK="sel4-microkernel/rpi4-graphics/Makefile"
check_value "$GRAPHICS_MK" '^MICROKIT_VERSION :=' "$MICROKIT_VERSION"
check_value "$GRAPHICS_MK" '^MICROKIT_SDK_SHA256 :=' "$MICROKIT_SDK_SHA256"
check_value "$GRAPHICS_MK" '^RPI_FIRMWARE_TAG :=' "$RPI_FIRMWARE_TAG"
check_value "$GRAPHICS_MK" '^UBOOT_VERSION :=' "$UBOOT_VERSION"

# Build container ARGs
CONTAINERFILE="sel4-microkernel/qemu-e2e.Containerfile"
check_value "$CONTAINERFILE" '^ARG RUST_TOOLCHAIN=' "$RUST_TOOLCHAIN"
check_value "$CONTAINERFILE" '^ARG MICROKIT_VERSION=' "$MICROKIT_VERSION"
check_value "$CONTAINERFILE" '^ARG MICROKIT_SDK_SHA256=' "$MICROKIT_SDK_SHA256"

# checksums.sha256: SDK line and the three firmware hashes
CHECKSUMS="sel4-microkernel/rpi4-graphics/checksums.sha256"
check_value "$CHECKSUMS" '^[0-9a-f]{64}  microkit-sdk-' \
    "$MICROKIT_SDK_SHA256  microkit-sdk-$MICROKIT_VERSION-linux-x86-64.tar.gz"
check_value "$CHECKSUMS" 'start4\.elf$' "$FW_START4"
check_value "$CHECKSUMS" 'fixup4\.dat$' "$FW_FIXUP4"
check_value "$CHECKSUMS" 'bcm2711-rpi-4-b\.dtb$' "$FW_DTB"
check_value "$CHECKSUMS" '^# FIRMWARE_TAG=' "$RPI_FIRMWARE_TAG"

# Helper scripts with default versions
check_value "sel4-microkernel/microkit-hello/setup.sh" '^MICROKIT_VERSION=' "$MICROKIT_VERSION"
check_value "scripts/download-microkit-sdk.sh" '^VERSION="\$\{MICROKIT_SDK_VERSION:-' "$MICROKIT_VERSION"

# Workflows: every occurrence of these pins in any workflow must match
for wf in .github/workflows/*.yml; do
    # Rust nightly pin (any nightly-YYYY-MM-DD literal)
    while IFS= read -r line; do
        [ -z "$line" ] && continue
        found="$(printf '%s\n' "$line" | grep -oE 'nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}' | head -1)"
        if [ "$found" != "$RUST_TOOLCHAIN" ]; then
            fail "$wf: '$line' pins '$found', expected '$RUST_TOOLCHAIN'"
        fi
    done <<< "$(grep -E 'nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}' "$wf" || true)"

    # Microkit version env vars
    while IFS= read -r line; do
        [ -z "$line" ] && continue
        case "$line" in
            *"\"$MICROKIT_VERSION\""*|*": $MICROKIT_VERSION"*) ;;
            *) fail "$wf: '$line' does not match MICROKIT_VERSION $MICROKIT_VERSION" ;;
        esac
    done <<< "$(grep -E '^\s*(MICROKIT_VERSION|MICROKIT_SDK_VERSION):' "$wf" || true)"

    # SDK SHA-256: env var definitions and any inline 64-hex literal
    # (usages of ${{ env.MICROKIT_SDK_SHA256 }} are references, not pins)
    while IFS= read -r line; do
        [ -z "$line" ] && continue
        case "$line" in
            *"$MICROKIT_SDK_SHA256"*) ;;
            *) fail "$wf: '$line' does not match MICROKIT_SDK_SHA256" ;;
        esac
    done <<< "$(grep -E '^\s*MICROKIT_SDK_SHA256:|[0-9a-f]{64}.*microkit-sdk' "$wf" || true)"

    # Firmware tag env vars
    while IFS= read -r line; do
        [ -z "$line" ] && continue
        case "$line" in
            *"\"$RPI_FIRMWARE_TAG\""*|*": $RPI_FIRMWARE_TAG"*) ;;
            *) fail "$wf: '$line' does not match RPI_FIRMWARE_TAG $RPI_FIRMWARE_TAG" ;;
        esac
    done <<< "$(grep -E '^\s*(RPI_FIRMWARE_TAG|FIRMWARE_TAG):' "$wf" || true)"
done

if [ "$FAIL" -ne 0 ]; then
    echo "" >&2
    echo "Pins have drifted. Update the sources of truth ($VERSIONS_MK," >&2
    echo "$TOOLCHAIN_TOML) and propagate to the locations above." >&2
    exit 1
fi

echo "All pins consistent."
