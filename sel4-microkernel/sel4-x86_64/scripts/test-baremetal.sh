#!/usr/bin/env bash
# Regression test for the bare-metal BOOTPROF image, run under QEMU with the
# same GRUB boot paths as a real PC (legacy BIOS and UEFI).
#
#  1. host unit tests: src/bootprof.rs, baremetal/tools/{nuc_probe,kvm_bench,pvh_only}.py
#  2. calibrated build, BIOS: sane BOOTPROF lines on COM1 and the EGA text
#     console; tsc_init and the APIC timer measurement each wait >= 40 ms
#  3. pinned TSC + APIC build (TSC from step 2), BIOS: no PIT waits
#  4. the pinned image under UEFI (OVMF): GRUB passes a GOP framebuffer and
#     the kernel's 8x8-font console renders the BOOTPROF lines intact
#  5. direct -kernel boots under -icount (deterministic, instruction-timed):
#     PVH on microvm (XSDT-only ACPI, module at top of RAM -> in-place load),
#     multiboot on microvm (scratch load), PVH on pc; runs are bit-identical
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
python3 -I baremetal/tools/test_kvm_bench.py
python3 -I baremetal/tools/test_pvh_only.py

# hmp PORT COMMAND: run one QEMU monitor command, then quit QEMU.
hmp() {
    python3 -I -c '
import socket, sys, time
for attempt in range(50):  # the monitor may not be listening yet
    try:
        s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=10)
        break
    except OSError:
        time.sleep(0.2)
else:
    sys.exit("QEMU monitor not reachable")
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
    hmp "$port" "$dump" || true
    # Bounded: never sit on a QEMU that ignored "quit" until the 180 s timeout.
    for _ in $(seq 1 20); do kill -0 "$qemu" 2>/dev/null || break; sleep 0.25; done
    kill "$qemu" 2>/dev/null || true
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
scripts/build-baremetal.sh >/dev/null 2>&1 || fail "build (calibrated)"
boot_iso build/baremetal/tsc-0-apic-0/sel4-bootprof.iso calibrated bios
check_common calibrated
cal_tsc_init="$(field calibrated "BOOTPROF kernel" tsc_init_us)"
cal_apic="$(field calibrated "BOOTPROF kernel" apic_cal_us)"
[ "$cal_tsc_init" -ge 40000 ] || fail "calibrated: tsc_init_us=$cal_tsc_init, expected the ~100 ms PIT wait"
[ "$cal_apic" -ge 40000 ] || fail "calibrated: apic_cal_us=$cal_apic, expected the 50-100 ms PIT wait"
mhz="$(field calibrated "BOOTPROF kernel" tsc_mhz)"
grep BOOTPROF "$WORK/calibrated.serial"

# QEMU's APIC timer runs at 1 GHz (1 ns bus cycle) under TCG and KVM.
APIC=1000000000
echo "== pinned TSC (${mhz} MHz) + APIC (1 GHz) build (BIOS)"
PIN="build/baremetal/tsc-${mhz}000000-apic-$APIC"
scripts/build-baremetal.sh --tsc-hz "${mhz}000000" --apic-hz "$APIC" >/dev/null 2>&1 || fail "build (pinned)"
boot_iso "$PIN/sel4-bootprof.iso" pinned bios
check_common pinned
pin_tsc_init="$(field pinned "BOOTPROF kernel" tsc_init_us)"
[ "$pin_tsc_init" -lt 1000 ] || fail "pinned: tsc_init_us=$pin_tsc_init, expected < 1000"
[ "$(field pinned "BOOTPROF kernel" apic_cal_us)" = 0 ] || fail "pinned: APIC timer still measured"
[ "$(field pinned "BOOTPROF kernel" apic_khz)" = "${APIC%000}" ] || fail "pinned: apic_khz not the pinned value"
[ "$(field pinned "BOOTPROF kernel" tsc_mhz)" = "$mhz" ] || fail "pinned: tsc_mhz not the pinned value"
grep BOOTPROF "$WORK/pinned.serial"

echo "== pinned image under UEFI (OVMF, GOP framebuffer console)"
boot_iso "$PIN/sel4-bootprof.iso" uefi uefi
check_common uefi
grep -q "Got framebuffer info.* type=1" "$WORK/uefi.serial" || fail "uefi: GRUB did not pass an RGB framebuffer"
grep BOOTPROF "$WORK/uefi.screen"

# --- direct kernel boots (no GRUB) under icount: guest time = instructions ---
# Under -icount the TSC runs at 1 GHz, so pin it to that.
ICT="build/baremetal/tsc-1000000000-apic-$APIC"
echo "== icount builds: direct -kernel boots (multiboot and PVH)"
scripts/build-baremetal.sh --tsc-hz 1000000000 --apic-hz "$APIC" >/dev/null 2>&1 || fail "build (icount)"

# boot_direct TAG MACHINE KERNEL -> $WORK/TAG.serial
boot_direct() {
    timeout 120 qemu-system-x86_64 -accel tcg -icount shift=0,sleep=off \
        -cpu Nehalem,-vme,+pdpe1gb,-xsave,-xsaveopt,-xsavec,-fsgsbase,-invpcid,+syscall,+lm \
        -machine "$2" -m 256M -nodefaults -no-reboot -display none -monitor none \
        -serial "file:$WORK/$1.serial" -kernel "$3" -initrd "$ICT/rootserver.elf" 2>/dev/null &
    local qemu=$!
    for _ in $(seq 1 240); do
        grep -q "BOOTPROF done" "$WORK/$1.serial" 2>/dev/null && break
        kill -0 "$qemu" 2>/dev/null || break
        sleep 0.5
    done
    kill "$qemu" 2>/dev/null; wait "$qemu" 2>/dev/null || true
    grep -q "BOOTPROF done" "$WORK/$1.serial" || { tail -20 "$WORK/$1.serial"; fail "$1: no BOOTPROF done"; }
}
# expect_path TAG pvh|multiboot
expect_path() {
    local pvh; pvh="$(grep -c '^PVH: start_info' "$WORK/$1.serial" || true)"
    case "$2" in
        pvh) [ "$pvh" = 1 ] || fail "$1: did not boot via PVH" ;;
        multiboot) [ "$pvh" = 0 ] || fail "$1: booted via PVH, expected multiboot" ;;
    esac
}
# Patch 0002: in place when the module is clear of the destination, else scratch.
expect_load() {
    local from to
    from="$(sed -n 's/.*final location: from=\(0x[0-9a-f]*\) to=.*/\1/p' "$WORK/$1.serial")"
    to="$(sed -n 's/.*final location: from=0x[0-9a-f]* to=\(0x[0-9a-f]*\).*/\1/p' "$WORK/$1.serial")"
    if [ "$2" = in-place ]; then [ "$from" = "$to" ]; else [ "$from" != "$to" ]; fi \
        || fail "$1: expected $2 user-image load, got from=$from to=$to"
}

boot_direct mv-pvh-1 microvm,acpi=on "$ICT/kernel-pvh.elf"
boot_direct mv-pvh-2 microvm,acpi=on "$ICT/kernel-pvh.elf"
expect_path mv-pvh-1 pvh
expect_load mv-pvh-1 in-place   # QEMU puts the PVH module at the top of RAM
grep -q "XSDT" "$WORK/mv-pvh-1.serial" || fail "microvm: ACPI not found via XSDT"
cmp -s <(grep BOOTPROF "$WORK/mv-pvh-1.serial") <(grep BOOTPROF "$WORK/mv-pvh-2.serial") \
    || fail "icount runs differ: not deterministic"
pvh_kernel="$(sed -n 's/.* kernel_us=\([0-9]*\).*/\1/p' "$WORK/mv-pvh-1.serial")"
[ "$pvh_kernel" -lt 5000 ] || fail "microvm PVH: kernel_us=$pvh_kernel (~instructions/1000), expected < 5000"

boot_direct mv-mb microvm,acpi=on "$ICT/kernel32.elf"
expect_path mv-mb multiboot
expect_load mv-mb scratch       # QEMU's multiboot loader puts the module right after the kernel
boot_direct pc-pvh pc "$ICT/kernel-pvh.elf"
expect_path pc-pvh pvh
expect_load pc-pvh in-place

printf "%-14s %10s %10s %10s\n" boot entry_us kernel_us user_us
for t in mv-pvh-1 mv-mb pc-pvh; do
    printf "%-14s %10s %10s %10s\n" "$t" \
        "$(sed -n 's/.*entry_us=\([0-9]*\).*/\1/p' "$WORK/$t.serial")" \
        "$(sed -n 's/.* kernel_us=\([0-9]*\).*/\1/p' "$WORK/$t.serial")" \
        "$(sed -n 's/.*user_us=\([0-9]*\).*/\1/p' "$WORK/$t.serial")"
done

echo "== kvm_bench.py plumbing (TCG + icount, microvm PVH, 1 run)"
python3 -I baremetal/tools/kvm_bench.py --accel tcg --icount 0 --machine microvm,acpi=on --runs 1 \
    --kernel "$ICT/kernel-pvh.elf" --rootserver "$ICT/rootserver.elf" >/dev/null || fail "kvm_bench (tcg)"

echo "PASS (tsc_init ${cal_tsc_init} us + apic_cal ${cal_apic} us calibrated -> ${pin_tsc_init} us + 0 pinned;"
echo "      BIOS + UEFI consoles; microvm PVH kernel_us=${pvh_kernel} (icount, deterministic))"
