# seL4 x86_64 with Rust Root Server

This directory contains a seL4 x86_64 system with a Rust-based root server.

> **Note:** Microkit does not support x86. For x86_64, you must use seL4 directly.
> For easier development, consider using Microkit with AArch64 or RISC-V instead.

## Verification Status

| Property | Status |
|----------|--------|
| Functional correctness | ✅ Proven |
| Binary verification | ❌ ARM only |
| Information flow | ✅ Proven |

The x86_64 port shares seL4's proven design, but full binary verification
is currently only available for ARM.

## Quick Start

```bash
# Set up seL4 build environment (takes a while)
./scripts/setup.sh

# Build the system
./scripts/build.sh

# Boot in QEMU
./scripts/run.sh
```

## Bare-Metal Boot-Time Measurement

`scripts/build-baremetal.sh` builds a hybrid (legacy BIOS + UEFI) GRUB image:
seL4, pinned to the commit that rust-sel4's `Cargo.lock` revision targets, plus
this root task. Patches in `baremetal/patches/`:

- `0001-bootprof-*`: TSC checkpoints, and a console for machines without a
  serial port. Output goes to COM1 and to the boot loader's framebuffer: EGA
  text on BIOS, or the UEFI GOP framebuffer drawn with a public-domain 8x8
  font. Kernel output is buffered in memory until the root task's first
  print, so console I/O doesn't affect the timestamps.
- `0002-x86-allow-boot-modules-below-kernel-image`: GRUB on UEFI loads the
  root task below the kernel, which upstream asserts against. Candidate for
  upstreaming.

This is a measurement build (debug + printing on, IOMMU off), not the verified
configuration.

```bash
pip3 install ply jinja2 pyyaml   # + git cmake ninja gcc xorriso mtools
                                 #   grub-pc-bin grub-efi-amd64-bin ovmf qemu-system-x86
scripts/test-baremetal.sh        # QEMU regression test: BIOS + UEFI, both TSC modes
scripts/build-baremetal.sh       # -> build/baremetal/tsc-0/sel4-bootprof.iso
sudo dd if=build/baremetal/tsc-0/sel4-bootprof.iso of=/dev/sdX bs=4M conv=fsync
```

Before flashing, boot an Ubuntu live USB on the target and run the probe. It
reports the CPU model and which `tsc_init` path seL4 will take, the TSC
frequency (Linux calibration, CPUID 15H/16H, `MSR_PLATFORM_INFO`), whether the
TSC counts from reset (`IA32_TSC_ADJUST`, firmware time from FPDT), the UEFI
framebuffer and resulting console geometry, UARTs, DMAR, and whether the memory
seL4 loads into is usable RAM. It is read-only and needs only the standard library:

```bash
sudo python3 baremetal/tools/nuc_probe.py   # prints a report, writes ./sel4-probe.txt
```

On the target (e.g. Intel NUC8/NUC9), disable Secure Boot in firmware setup
(F2), since GRUB is unsigned. Then pick the USB stick from the F10 menu,
booting it either UEFI or legacy. Output:

```
BOOTPROF kernel tsc_mhz=… tsc_init_us=…          # tsc_init: ~100 ms PIT wait
BOOTPROF kernel entry_us=… exit_us=… kernel_us=… # µs since power-on (TSC)
BOOTPROF user tsc_mhz=… user_us=…                # root task main()
```

- `entry_us` is roughly firmware + GRUB time. `kernel_us` is seL4's boot.
  `user_us - exit_us` is kernel exit to root-task entry.
- seL4 only reads the TSC frequency from an MSR on Nehalem–Skylake model IDs;
  Kaby/Coffee Lake (NUC8, NUC9 Ghost Canyon) fall back to the PIT. Rebuild with
  the printed frequency to remove it: `scripts/build-baremetal.sh --tsc-hz 2400000000`.
- The screen console wraps to the top instead of scrolling; scrolling would copy
  megabytes of uncached video memory per line. If the root task faults before
  its first print, buffered kernel output only appears on a kernel halt.

### Toward microVMs: KVM baseline

The probe's virtualization section reports what a microVM host depends on:
VT-x state in firmware, EPT/VPID/unrestricted guest, APICv and posted
interrupts, PML (hardware dirty logging, for snapshots), TSC scaling, 1 GiB EPT
pages, VT-d interrupt remapping and posted interrupts, SR-IOV, and the
`kvm_intel` module parameters.

With `/dev/kvm` available, boot the same seL4 image as a KVM guest repeatedly:

```bash
sudo python3 baremetal/tools/kvm_bench.py --runs 20 \
    --kernel build/baremetal/tsc-<HZ>/kernel32.elf \
    --rootserver build/baremetal/tsc-<HZ>/rootserver.elf
```

It reports min/median/p90 for `host_ms` (QEMU spawn to the root task's first
serial line), plus the guest's `entry_us` (SeaBIOS and the multiboot loader:
the guest TSC starts near 0 at VM creation), `kernel_us` and `tsc_init_us`.
Under KVM the guest TSC runs at the host frequency, so use the probe's
`--tsc-hz`. The `pc` machine's firmware cost is the target of the next step: a
PVH entry point, so Firecracker, Cloud Hypervisor or QEMU `microvm` can load
seL4 without firmware.

## Project Structure

```
sel4-x86_64/
├── src/
│   └── main.rs         # Rust root server
├── scripts/
│   ├── setup.sh        # Downloads and builds seL4
│   ├── build.sh        # Builds the Rust code
│   └── run.sh          # Boots in QEMU
├── Cargo.toml
└── README.md
```

## Building seL4 for x86_64

The seL4 build system uses:
- Google's `repo` tool to fetch sources
- CMake/Ninja for building
- Custom tooling for capability derivation

The `setup.sh` script automates this process.

## Verified Components

The root server includes Verus-verified code:

- **VerifiedCap**: Capability with proven derivation properties
- **VerifiedMsgBuffer**: IPC buffer with bounds checking proofs
- **VerifiedUntypedDesc**: Memory region descriptor with containment proofs

## Why Use seL4 Directly (vs Microkit)?

Use seL4 directly when you need:
- x86_64 platform support
- Full control over capability management
- Custom boot flow
- Integration with existing seL4 projects

Use Microkit when you want:
- Simpler API
- Static system configuration
- Standard protection domain model
- AArch64 or RISC-V platforms

## Resources

- [seL4 x86 Porting Guide](https://docs.sel4.systems/projects/sel4/porting.html)
- [rust-sel4 Documentation](https://github.com/seL4/rust-sel4)
- [seL4 Build System](https://docs.sel4.systems/projects/buildsystem/)
