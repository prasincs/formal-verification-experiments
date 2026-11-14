#!/usr/bin/env bash
# Boot the seL4 PVH image under Cloud Hypervisor, nested inside QEMU TCG:
#
#   QEMU TCG (-cpu max: emulated AMD SVM)
#     L1 Linux + kvm_amd (npt=0)  ->  cloud-hypervisor  ->  seL4 (PVH)
#
# Functional only: nested emulation makes every timing meaningless. With
# /dev/kvm on real hardware, run cloud-hypervisor directly instead.
#
#   scripts/test-cloud-hypervisor-nested.sh [BUILD_DIR]
#     BUILD_DIR  default build/baremetal/tsc-1000000000-apic-1000000000 (built by
#                test-baremetal.sh; the TSC pin only scales the reported µs)
#
# Needs: qemu-system-x86_64, busybox-static, zstd, cpio, an L1 kernel with
# modules (apt install linux-image-virtual), network for the pinned CH binary.
#
# Why npt=0: QEMU TCG's emulated nested paging breaks guests under L1 KVM
# (even a 6-instruction PVH kernel triple-faults); shadow paging works.
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"
BUILD="${1:-build/baremetal/tsc-1000000000-apic-1000000000}"
CH_VERSION=v54.1
CH_SHA256=f9ffb9646783233d1f0fb09e77db1fca66499d1041e63bc011b90683c0d49933
CH_BIN="sel4-kit/cloud-hypervisor-$CH_VERSION"
L1_KVER="${L1_KVER:-$(ls /boot/vmlinuz-* 2>/dev/null | sed 's|/boot/vmlinuz-||' | sort -V | tail -1)}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

for cmd in qemu-system-x86_64 zstd cpio gzip curl sha256sum; do
    command -v "$cmd" >/dev/null || fail "missing: $cmd"
done
[ -x /bin/busybox ] && file -L /bin/busybox | grep -q static || fail "need static /bin/busybox (busybox-static)"
[ -n "$L1_KVER" ] && [ -f "/boot/vmlinuz-$L1_KVER" ] || fail "no L1 kernel (apt install linux-image-virtual)"
M="/lib/modules/$L1_KVER/kernel"
for k in "$BUILD/kernel64.elf" "$BUILD/rootserver.elf"; do
    [ -f "$k" ] || fail "missing $k (run scripts/build-baremetal.sh --tsc-hz ... --apic-hz ...)"
done

# Pinned Cloud Hypervisor static binary, verified before it is ever executed.
if [ ! -f "$CH_BIN" ]; then
    mkdir -p sel4-kit
    curl -sSfL -o "$CH_BIN.part" \
        "https://github.com/cloud-hypervisor/cloud-hypervisor/releases/download/$CH_VERSION/cloud-hypervisor-static"
    mv "$CH_BIN.part" "$CH_BIN"
fi
echo "$CH_SHA256  $CH_BIN" | sha256sum -c --quiet || fail "cloud-hypervisor checksum mismatch"

# L1 initramfs: busybox, KVM modules, cloud-hypervisor, seL4 kernel + root task.
R="$WORK/root"
mkdir -p "$R"/{bin,dev,proc,sys,tmp,lib/modules,sel4}
cp /bin/busybox "$R/bin/"
for m in virt/lib/irqbypass drivers/crypto/ccp/ccp arch/x86/kvm/kvm arch/x86/kvm/kvm-amd; do
    zstd -qdc "$M/$m.ko.zst" > "$R/lib/modules/$(basename "$m").ko"
done
install -m 755 "$CH_BIN" "$R/bin/cloud-hypervisor"
cp "$BUILD/kernel64.elf" "$BUILD/rootserver.elf" "$R/sel4/"
cat > "$R/init" <<'EOF'
#!/bin/busybox sh
/bin/busybox --install -s /bin
mount -t proc proc /proc; mount -t sysfs sys /sys; mount -t devtmpfs dev /dev
for m in irqbypass ccp kvm; do insmod /lib/modules/$m.ko; done
insmod /lib/modules/kvm-amd.ko npt=0
[ -c /dev/kvm ] || { echo "L1: no /dev/kvm"; poweroff -f; }
echo "L1: cloud-hypervisor $(cloud-hypervisor --version)"
timeout 120 cloud-hypervisor --kernel /sel4/kernel64.elf --initramfs /sel4/rootserver.elf \
    --cpus boot=1 --memory size=256M --serial tty --console off > /tmp/ch.out 2>&1
echo "L1: ---- guest serial ----"
tr -d '\r' < /tmp/ch.out
echo "L1: ---- end ----"
poweroff -f
EOF
chmod +x "$R/init"
(cd "$R" && find . | cpio -o -H newc --quiet | gzip -1) > "$WORK/l1.cpio.gz"

echo "== booting L1 under QEMU TCG (takes ~1 min)"
timeout 600 qemu-system-x86_64 -accel tcg -cpu max -smp 1 -m 2G -nodefaults -no-reboot \
    -display none -monitor none -serial "file:$WORK/l1.log" \
    -kernel "/boot/vmlinuz-$L1_KVER" -initrd "$WORK/l1.cpio.gz" \
    -append "console=ttyS0 panic=-1 loglevel=4" 2>/dev/null || true
LOG="$WORK/guest.log"
sed -n '/^L1: ---- guest serial/,/^L1: ---- end/p' "$WORK/l1.log" > "$LOG"
grep -a "^L1: cloud-hypervisor" "$WORK/l1.log" || fail "L1 did not start cloud-hypervisor"

grep -q "BOOTPROF done" "$LOG" || { tail -40 "$LOG"; fail "seL4 did not reach the root task"; }
grep -q "^PVH: start_info" "$LOG" || fail "did not boot via PVH"
grep -q "MADT_x2APIC apic_id=0x0" "$LOG" || fail "CPU not found via MADT x2APIC entry (patch 0006)"
grep -q "XSDT" "$LOG" || fail "ACPI not found via XSDT (patch 0003)"
from="$(sed -n 's/.*final location: from=\(0x[0-9a-f]*\) to=.*/\1/p' "$LOG")"
to="$(sed -n 's/.*final location: from=0x[0-9a-f]* to=\(0x[0-9a-f]*\).*/\1/p' "$LOG")"
[ -n "$from" ] && [ "$from" = "$to" ] || fail "expected in-place user-image load (patch 0002), got $from -> $to"
grep -E "^PVH:|module #0|MADT_x2APIC|BOOTPROF" "$LOG"
echo "PASS: seL4 boots under Cloud Hypervisor $CH_VERSION via PVH (nested; timings not meaningful)"
