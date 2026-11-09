#!/usr/bin/env bash
# Regression test for the bare-metal BOOTPROF image, run under QEMU (BIOS +
# GRUB, i.e. the same boot path as a real legacy-boot PC).
#
#  1. host unit tests for src/bootprof.rs
#  2. PIT-calibrated build: boots, prints sane BOOTPROF lines on COM1 and the
#     VGA text console, and tsc_init takes >= 40 ms (the PIT wait)
#  3. pinned-TSC build (frequency taken from step 2): tsc_init < 1 ms
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"
TOOLCHAIN="+$(sed -n 's/^channel = "\(.*\)"/\1/p' ../rust-toolchain.toml)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

echo "== host unit tests"
rustc "$TOOLCHAIN" --edition 2021 --test src/bootprof.rs -o "$WORK/bootprof-test"
"$WORK/bootprof-test" -q

# boot_iso ISO TAG -> writes $WORK/TAG.serial and $WORK/TAG.vga (decoded text)
boot_iso() {
    local iso="$1" tag="$2" port
    port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
    timeout 120 qemu-system-x86_64 \
        -cpu Nehalem,-vme,+pdpe1gb,-xsave,-xsaveopt,-xsavec,-fsgsbase,-invpcid,+syscall,+lm,enforce \
        -m 512M -display none -vga std -no-reboot \
        -serial "file:$WORK/$tag.serial" \
        -monitor "tcp:127.0.0.1:$port,server,nowait" \
        -cdrom "$iso" &
    local qemu=$!
    for _ in $(seq 1 200); do
        grep -q "BOOTPROF done" "$WORK/$tag.serial" 2>/dev/null && break
        kill -0 "$qemu" 2>/dev/null || break
        sleep 0.5
    done
    python3 - "$port" "$WORK/$tag.vgaraw" > "$WORK/$tag.vga" <<'EOF'
import socket, sys, time, os
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=10)
time.sleep(0.2); s.recv(65536)
# Quote the path: HMP otherwise parses "4000 /tmp/..." as a division.
s.sendall(f'pmemsave 0xb8000 4000 "{sys.argv[2]}"\n'.encode())
for _ in range(50):
    if os.path.exists(sys.argv[2]) and os.path.getsize(sys.argv[2]) == 4000:
        break
    time.sleep(0.1)
s.sendall(b"quit\n")
b = open(sys.argv[2], "rb").read()
for r in range(25):
    print("".join(chr(b[(r * 80 + c) * 2]) for c in range(80)).rstrip())
EOF
    wait "$qemu" 2>/dev/null || true
}

# field TAG LINE_PREFIX KEY -> integer value
field() {
    sed -n "s/.*$2.* $3=\([0-9]*\).*/\1/p" "$WORK/$1.serial" | head -1
}

check_common() {
    local tag="$1"
    grep -q "BOOTPROF done" "$WORK/$tag.serial" || { cat "$WORK/$tag.serial"; fail "$tag: no BOOTPROF done on serial"; }
    grep -q "BOOTPROF done" "$WORK/$tag.vga" || { cat "$WORK/$tag.vga"; fail "$tag: VGA console mirror missing output"; }
    local entry exit user mhz
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

echo "== PIT-calibrated build"
scripts/build-baremetal.sh --tsc-hz 0 >/dev/null 2>&1 || fail "build (calibrated)"
boot_iso build/baremetal/tsc-0/sel4-bootprof.iso calibrated
check_common calibrated
cal_tsc_init="$(field calibrated "BOOTPROF kernel" tsc_init_us)"
[ "$cal_tsc_init" -ge 40000 ] || fail "calibrated: tsc_init_us=$cal_tsc_init, expected the ~100 ms PIT wait"
mhz="$(field calibrated "BOOTPROF kernel" tsc_mhz)"
grep BOOTPROF "$WORK/calibrated.serial"

echo "== pinned-TSC build (${mhz} MHz)"
scripts/build-baremetal.sh --tsc-hz "${mhz}000000" >/dev/null 2>&1 || fail "build (pinned)"
boot_iso "build/baremetal/tsc-${mhz}000000/sel4-bootprof.iso" pinned
check_common pinned
pin_tsc_init="$(field pinned "BOOTPROF kernel" tsc_init_us)"
[ "$pin_tsc_init" -lt 1000 ] || fail "pinned: tsc_init_us=$pin_tsc_init, expected < 1000"
[ "$(field pinned "BOOTPROF kernel" tsc_mhz)" = "$mhz" ] || fail "pinned: tsc_mhz not the pinned value"
grep BOOTPROF "$WORK/pinned.serial"

echo "PASS (tsc_init: ${cal_tsc_init} us calibrated -> ${pin_tsc_init} us pinned)"
