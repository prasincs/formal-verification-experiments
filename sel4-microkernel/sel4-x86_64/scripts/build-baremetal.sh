#!/usr/bin/env bash
# Build a bootable GRUB image (legacy BIOS and UEFI): patched seL4 kernel + Rust root
# task, instrumented to print boot timestamps (BOOTPROF lines) to COM1 and to
# the screen (VGA text on BIOS, GOP framebuffer on UEFI).
#
#   scripts/build-baremetal.sh [--tsc-hz HZ]
#
# --tsc-hz 0 (default) keeps seL4's PIT calibration (~100 ms). Pass the
# machine's TSC frequency in Hz (the first boot prints tsc_mhz) to skip it.
#
# Output: build/baremetal/tsc-<HZ>/sel4-bootprof.iso (dd it to a USB stick).
set -euo pipefail

TSC_HZ=0
while [ $# -gt 0 ]; do
    case "$1" in
        --tsc-hz) TSC_HZ="${2:?--tsc-hz needs a value}"; shift 2 ;;
        *) echo "usage: $0 [--tsc-hz HZ]" >&2; exit 2 ;;
    esac
done
[[ "$TSC_HZ" =~ ^[0-9]+$ ]] || { echo "--tsc-hz must be an integer (Hz)" >&2; exit 2; }

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BM="$PROJECT_DIR/baremetal"
cd "$PROJECT_DIR"

# seL4 commit pinned by the rust-sel4 revision in Cargo.lock (968090ce);
# bump both together.
SEL4_REV=44b2b946fc51fc6ebdaf126ff72b9e073575dc81
SEL4_SRC="$PROJECT_DIR/sel4-kit/seL4-$SEL4_REV"
OUT="$PROJECT_DIR/build/baremetal/tsc-$TSC_HZ"
TOOLCHAIN="+$(sed -n 's/^channel = "\(.*\)"/\1/p' ../rust-toolchain.toml)"

for cmd in git cmake ninja gcc objcopy python3 cargo grub-mkrescue xorriso mformat; do
    command -v "$cmd" >/dev/null || { echo "missing: $cmd" >&2; exit 1; }
done
for d in /usr/lib/grub/i386-pc /usr/lib/grub/x86_64-efi; do
    [ -d "$d" ] || { echo "missing $d (grub-pc-bin / grub-efi-amd64-bin)" >&2; exit 1; }
done
python3 -c 'import ply, jinja2, yaml' 2>/dev/null \
    || { echo "missing python modules: pip3 install ply jinja2 pyyaml" >&2; exit 1; }

# --- seL4 source at the pinned commit, with the BOOTPROF patch ---
if [ ! -d "$SEL4_SRC/.git" ]; then
    git init -q "$SEL4_SRC"
    git -C "$SEL4_SRC" fetch -q --depth 1 https://github.com/seL4/seL4.git "$SEL4_REV"
    git -C "$SEL4_SRC" checkout -q FETCH_HEAD
fi
[ "$(git -C "$SEL4_SRC" rev-parse HEAD)" = "$SEL4_REV" ] \
    || { echo "seL4 checkout is not at $SEL4_REV" >&2; exit 1; }
# Always start from the pristine pinned tree so patch edits take effect.
git -C "$SEL4_SRC" checkout -q -- .
git -C "$SEL4_SRC" clean -fdq
for patch in "$BM"/patches/0*.patch; do
    git -C "$SEL4_SRC" apply "$patch"
done

# --- kernel ---
cmake -G Ninja -S "$SEL4_SRC" -B "$OUT/kernel" \
    -DCMAKE_TOOLCHAIN_FILE="$SEL4_SRC/gcc.cmake" -DCROSS_COMPILER_PREFIX= \
    -DCMAKE_INSTALL_PREFIX="$OUT/sel4-prefix" \
    -C "$BM/kernel-settings.cmake" \
    -DKernelPC99TSCFrequency="$TSC_HZ" >/dev/null
ninja -C "$OUT/kernel" all >/dev/null
ninja -C "$OUT/kernel" install >/dev/null
# QEMU's multiboot1 -kernel loader only accepts ELF32; GRUB takes either.
objcopy -O elf32-i386 "$OUT/sel4-prefix/bin/kernel.elf" "$OUT/kernel32.elf"

# --- root task ---
SEL4_PREFIX="$OUT/sel4-prefix" cargo "$TOOLCHAIN" build --release --locked \
    --target "$BM/x86_64-sel4-roottask.json" \
    --target-dir "$OUT/target" \
    -Z build-std=core,alloc,compiler_builtins \
    -Z build-std-features=compiler-builtins-mem \
    -Zjson-target-spec
ROOTTASK="$OUT/target/x86_64-sel4-roottask/release/sel4-x86-rootserver.elf"
cp "$ROOTTASK" "$OUT/rootserver.elf"  # with kernel32.elf: the QEMU/KVM -kernel/-initrd pair

# --- GRUB image ---
rm -rf "$OUT/iso"
mkdir -p "$OUT/iso/boot/grub"
cp "$OUT/sel4-prefix/bin/kernel.elf" "$OUT/iso/boot/kernel.elf"
cp "$ROOTTASK" "$OUT/iso/boot/rootserver.elf"
cp "$BM/grub.cfg" "$OUT/iso/boot/grub/grub.cfg"
grub-mkrescue -o "$OUT/sel4-bootprof.iso" "$OUT/iso" 2>/dev/null

echo "kernel:    $OUT/sel4-prefix/bin/kernel.elf"
echo "roottask:  $ROOTTASK"
echo "image:     $OUT/sel4-bootprof.iso"
(cd "$OUT" && sha256sum sel4-bootprof.iso iso/boot/kernel.elf iso/boot/rootserver.elf)
