#!/usr/bin/env bash
# Regression test for the bare-metal BOOTPROF image, run under QEMU with the
# same GRUB boot paths as a real PC (legacy BIOS and UEFI).
#
#  1. host unit tests for src/bootprof.rs and baremetal/tools/nuc_probe.py
#  2. PIT-calibrated build, BIOS: sane BOOTPROF lines on COM1 and the EGA text
#     console, and tsc_init takes >= 40 ms (the PIT wait)
#  3. pinned-TSC build (frequency taken from step 2), BIOS: tsc_init < 1 ms
#  4. the pinned image under UEFI (OVMF): GRUB passes a GOP framebuffer and
#     the kernel's 8x8-font console renders the BOOTPROF lines intact
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"
TOOLCHAIN="+$(sed -n 's/^channel = "\(.*\)"/\1/p' ../rust-toolchain.toml)"
FONT="sel4-kit/seL4-44b2b946fc51fc6ebdaf126ff72b9e073575dc81/include/arch/x86/arch/bootprof_font8x8.h"
OVMF_CODE=/usr/share/OVMF/OVMF_CODE_4M.fd
OVMF_VARS=/usr/share/OVMF/OVMF_VARS_4M.fd
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

[ -f "$OVMF_CODE" ] && [ -f "$OVMF_VARS" ] || fail "OVMF not found (apt install ovmf)"

echo "== host unit tests"
rustc "$TOOLCHAIN" --edition 2021 --test src/bootprof.rs -o "$WORK/bootprof-test"
"$WORK/bootprof-test" -q
python3 -I baremetal/tools/test_nuc_probe.py

# hmp PORT COMMAND: run one QEMU monitor command, then quit QEMU.
hmp() {
    python3 -I -c '
import socket, sys, time
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=10)
time.sleep(0.2); s.recv(65536)
s.sendall(sys.argv[2].encode() + b"\n")
time.sleep(2)  # HMP has no completion signal; dumps are written synchronously
s.sendall(b"quit\n")
' "$1" "$2"
}

# boot_iso ISO TAG bios|uefi -> $WORK/TAG.serial and $WORK/TAG.screen (decoded)
boot_iso() {
    local iso="$1" tag="$2" fw="$3" port dump fwargs=()
    port="$(python3 -I -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
    if [ "$fw" = uefi ]; then
        cp "$OVMF_VARS" "$WORK/$tag.vars"
        fwargs=(-drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE"
                -drive "if=pflash,format=raw,file=$WORK/$tag.vars")
        # Quote paths: HMP otherwise parses "4000 /tmp/..." as a division.
        dump="screendump \"$WORK/$tag.ppm\""
    else
        dump="pmemsave 0xb8000 4000 \"$WORK/$tag.ega\""
    fi
    timeout 180 qemu-system-x86_64 \
        -cpu Nehalem,-vme,+pdpe1gb,-xsave,-xsaveopt,-xsavec,-fsgsbase,-invpcid,+syscall,+lm,enforce \
        -m 512M -display none -vga std -no-reboot "${fwargs[@]}" \
        -serial "file:$WORK/$tag.serial" \
        -monitor "tcp:127.0.0.1:$port,server,nowait" \
        -cdrom "$iso" &
    local qemu=$!
    for _ in $(seq 1 300); do
        grep -q "BOOTPROF done" "$WORK/$tag.serial" 2>/dev/null && break
        kill -0 "$qemu" 2>/dev/null || break
        sleep 0.5
    done
    hmp "$port" "$dump"
    wait "$qemu" 2>/dev/null || true
    if [ "$fw" = uefi ]; then
        python3 -I baremetal/tools/decode_screen.py "$WORK/$tag.ppm" "$FONT" > "$WORK/$tag.screen"
    else
        python3 -I baremetal/tools/decode_screen.py --ega "$WORK/$tag.ega" > "$WORK/$tag.screen"
    fi
}

# field TAG LINE_PREFIX KEY -> integer value from the serial log
field() {
    sed -n "s/.*$2.* $3=\([0-9]*\).*/\1/p" "$WORK/$1.serial" | head -1
}

check_common() {
    local tag="$1" entry exit user mhz
    grep -q "BOOTPROF done" "$WORK/$tag.serial" || { cat "$WORK/$tag.serial"; fail "$tag: no BOOTPROF done on serial"; }
    grep -q "^BOOTPROF done$" "$WORK/$tag.screen" || { cat "$WORK/$tag.screen"; fail "$tag: screen console missing output"; }
    grep -q "^BOOTPROF user tsc_mhz=[0-9]* user_us=[0-9]*$" "$WORK/$tag.screen" \
        || { cat "$WORK/$tag.screen"; fail "$tag: BOOTPROF user line not rendered intact"; }
    entry="$(field "$tag" "BOOTPROF kernel" entry_us)"
    exit="$(field "$tag" "BOOTPROF kernel" exit_us)"
    user="$(field "$tag" "BOOTPROF user" user_us)"
    mhz="$(field "$tag" "BOOTPROF kernel" tsc_mhz)"
    [ -n "$entry" ] && [ -n "$exit" ] && [ -n "$user" ] && [ -n "$mhz" ] || fail "$tag: missing BOOTPROF fields"
    [ "$mhz" -gt 0 ] || fail "$tag: tsc_mhz=0"
    [ "$entry" -lt "$exit" ] && [ "$exit" -le "$user" ] \
        || fail "$tag: timestamps not ordered: entry=$entry exit=$exit user=$user"
    [ "$(field "$tag" "BOOTPROF user" tsc_mhz)" = "$mhz" ] || fail "$tag: kernel/user tsc_mhz disagree"
}

echo "== PIT-calibrated build (BIOS)"
scripts/build-baremetal.sh --tsc-hz 0 >/dev/null 2>&1 || fail "build (calibrated)"
boot_iso build/baremetal/tsc-0/sel4-bootprof.iso calibrated bios
check_common calibrated
cal_tsc_init="$(field calibrated "BOOTPROF kernel" tsc_init_us)"
[ "$cal_tsc_init" -ge 40000 ] || fail "calibrated: tsc_init_us=$cal_tsc_init, expected the ~100 ms PIT wait"
mhz="$(field calibrated "BOOTPROF kernel" tsc_mhz)"
grep BOOTPROF "$WORK/calibrated.serial"

echo "== pinned-TSC build (${mhz} MHz, BIOS)"
PINNED="build/baremetal/tsc-${mhz}000000/sel4-bootprof.iso"
scripts/build-baremetal.sh --tsc-hz "${mhz}000000" >/dev/null 2>&1 || fail "build (pinned)"
boot_iso "$PINNED" pinned bios
check_common pinned
pin_tsc_init="$(field pinned "BOOTPROF kernel" tsc_init_us)"
[ "$pin_tsc_init" -lt 1000 ] || fail "pinned: tsc_init_us=$pin_tsc_init, expected < 1000"
[ "$(field pinned "BOOTPROF kernel" tsc_mhz)" = "$mhz" ] || fail "pinned: tsc_mhz not the pinned value"
grep BOOTPROF "$WORK/pinned.serial"

echo "== pinned image under UEFI (OVMF, GOP framebuffer console)"
boot_iso "$PINNED" uefi uefi
check_common uefi
grep -q "Got framebuffer info.* type=1" "$WORK/uefi.serial" || fail "uefi: GRUB did not pass an RGB framebuffer"
grep BOOTPROF "$WORK/uefi.screen"

echo "PASS (tsc_init: ${cal_tsc_init} us calibrated -> ${pin_tsc_init} us pinned; BIOS + UEFI consoles)"
