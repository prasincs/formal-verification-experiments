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

`scripts/build-baremetal.sh` builds seL4 (pinned to the commit rust-sel4's
`Cargo.lock` revision targets) plus this root task. It produces a hybrid
BIOS + UEFI GRUB image and direct-boot kernels for QEMU and microVM VMMs.
Patches in `baremetal/patches/`, applied in order:

| Patch | What | Upstream? |
|---|---|---|
| `0001-bootprof-*` | TSC checkpoints. A console for serial-less machines: COM1, plus the boot loader's framebuffer (EGA text on BIOS, UEFI GOP with a public-domain 8x8 font). Output is buffered until the root task's first print, which keeps console I/O out of the boot timestamps but **not out of time-to-ready**: a `BOOTPROF flush` line reports what the deferred output cost. | no (instrumentation) |
| `0002-x86-boot-module-placement` | Load the root task in place when the boot module doesn't overlap its destination. Upstream ELF-loads into scratch just past the module, which falls off the end of RAM when a PVH loader puts the module at the top of RAM. Also fixes a `find_load_paddr` underflow (it could return non-RAM), replaces its `assert` with an error, and refuses to boot if the image lands anywhere but the expected address. | candidate |
| `0003-x86-pvh-boot-and-acpi-xsdt` | PVH entry (`XEN_ELFNOTE_PHYS32_ENTRY` and an `hvm_start_info` parser). Every pointer and range is checked to lie below 4 GiB with no wrap-around, and memory is clipped to whole pages. Also ACPI XSDT support: it prefers the XSDT on ACPI 2.0+, as the spec says, since QEMU `microvm`, Firecracker and Cloud Hypervisor publish nothing else. Tables above 4 GiB are skipped with a warning, and with the IOMMU enabled the kernel refuses to boot rather than risk losing the DMAR silently. | candidate |
| `0004-x86-pinnable-apic-timer-frequency` | `KernelPC99APICTimerFrequency`. The non-MCS `apic_init()` otherwise always measures the APIC timer against the PIT (50–100 ms). Pins below 1 MHz (TSC or APIC) are compile errors. On bare metal, a pinned APIC frequency is cross-checked against the CPUID 15H crystal clock, with a warning on mismatch. | candidate |
| `0005-x86-mbi2-mem-lower-units` | Upstream bug: the multiboot2 path stores `mem_lower` in bytes, but `copy_boot_code_aps()` expects KiB, which disables the SMP AP boot-code bounds check. | candidate |

This is a measurement build (debug + printing on, IOMMU off), not the verified
configuration.

```bash
pip3 install ply jinja2 pyyaml   # + git cmake ninja gcc xorriso mtools
                                 #   grub-pc-bin grub-efi-amd64-bin ovmf qemu-system-x86
scripts/test-baremetal.sh        # QEMU regression test (see header)
scripts/build-baremetal.sh [--tsc-hz HZ] [--apic-hz HZ]
#   -> build/baremetal/tsc-<HZ>-apic-<HZ>/{sel4-bootprof.iso, kernel32.elf, kernel-pvh.elf, rootserver.elf}
```

seL4 measures two clocks against the PIT at boot, about 100 ms each. Pin both:
`--tsc-hz` and `--apic-hz`. The first boot prints `tsc_mhz` and `apic_khz`.
On Skylake and later Intel CPUs the APIC timer runs at the crystal clock
(24 MHz on client parts); under KVM and QEMU it runs at 1 GHz.

### On real hardware

Before flashing, boot an Ubuntu live USB on the target and run the probe. It
reports:

- the CPU model, and which `tsc_init` path seL4 will take
- the TSC and APIC timer frequencies, with the `--tsc-hz` / `--apic-hz` values
  to use
- whether the TSC counts from reset (`IA32_TSC_ADJUST`, FPDT firmware time)
- the UEFI framebuffer and the resulting console geometry
- UARTs, DMAR, and whether the memory seL4 loads into is usable RAM
- virtualization support (see below)

It is read-only and needs only the standard library:

```bash
sudo python3 baremetal/tools/nuc_probe.py   # prints a report, writes ./sel4-probe.txt
sudo dd if=build/baremetal/tsc-<HZ>-apic-<HZ>/sel4-bootprof.iso of=/dev/sdX bs=4M conv=fsync
```

Disable Secure Boot in firmware setup (F2), since GRUB is unsigned. Then pick
the USB stick from the F10 menu, booting it either UEFI or legacy. Output:

```
BOOTPROF kernel tsc_mhz=… tsc_init_us=…          # TSC: PIT wait unless pinned
BOOTPROF kernel apic_khz=… apic_cal_us=…         # APIC timer: PIT wait unless pinned
BOOTPROF kernel entry_us=… exit_us=… kernel_us=… # µs since power-on (TSC)
BOOTPROF flush bytes=… early=… flush_us=… done_us=… # deferred console output
BOOTPROF user tsc_mhz=… user_us=…                # root task main()
```

- `entry_us` is roughly firmware + boot loader time, if the firmware did not
  adjust the TSC (the probe checks `IA32_TSC_ADJUST`). In a VM, the TSC
  starts at vCPU creation. `kernel_us` is seL4's boot. `user_us - exit_us` is
  kernel exit to root-task entry.
- `flush_us` is the cost of the output deferred out of those numbers, and
  `done_us` is when it finished. Under QEMU TCG: 3.6 ms for BIOS text and
  68 ms for the UEFI framebuffer. If `early=1`, the buffer overflowed during
  boot, and console I/O is inside `kernel_us`.
- The screen console wraps to the top instead of scrolling; scrolling would
  copy megabytes of uncached video memory per line. If the root task faults
  before its first print, buffered kernel output only appears on a kernel halt.

### Toward microVMs: PVH, microvm, and an instruction-count baseline

`kernel-pvh.elf` is `kernel32.elf` with its multiboot1 magic zeroed
(`baremetal/tools/pvh_only.py` checks for the PVH note first). QEMU's
`x86_load_linux()` tries `load_multiboot()` before `load_elfboot()`, and the
source comment says this is deliberate. It does so in 8.2.2, in 11.1.3 (the
latest release) and on master as of 2026-11. So `kernel32.elf` takes the
multiboot path even on `microvm`, silently and about 50% slower. PVH loading
also uses `I386_ELF_MACHINE`, so the PVH image must be ELF32 (hence the
`objcopy`). `kvm_bench.py` reports the
path it saw, and `--expect-path pvh` fails such runs.

Without KVM, QEMU's `-icount` makes guest time an instruction count
(1 insn = 1 ns). Runs are bit-identical. With TSC and APIC pinned
(`scripts/test-baremetal.sh` asserts these):

| Machine | Boot | `entry_us` | `kernel_us` |
|---|---|---:|---:|
| `pc` (SeaBIOS) | PVH | 5728 | 1085 |
| `microvm` (qboot) | PVH | 5963 | 1074 |
| `microvm` (qboot) | multiboot | 5913 | 1583 |
| `microvm`, clocks not pinned | PVH | 5969 | 201275 |

What this does and does not show:

- **Holds up:** the two PIT measurements are 200 of the 201 ms. PVH saves about
  0.5 M instructions over multiboot (no scratch copy). `kernel_us` varies by
  less than 1% across guest RAM from 64 MiB to 8 GiB and between debug and
  non-debug builds.
- **About 1.07 M instructions is an upper bound for this build, not "seL4's
  boot".** It includes formatting about 1.6 KB of boot `printf`s (not measured
  separately). Converting to wall time (about 0.3–0.5 ms at 2.4 GHz) assumes
  roughly 1 instruction per cycle with no cache or TLB stalls; that's optimistic
  for boot code dominated by zeroing and copying.
- **icount is blind to I/O cost and VM exits.** That's where KVM time goes:
  CPUID, MSR writes, PIC/IOAPIC and COM1 port accesses, EPT first-touch faults.
  So the firmware `entry_us` above is an instruction count, not a claim that
  firmware dominates wall time. The 55 µs `flush_us` under icount means the
  same thing; under Firecracker every serial byte is a VM exit.
- **For snapshot-resume sandboxes, guest boot isn't on the hot path.** The
  number that matters there is restore latency on real KVM: VMM spawn, memory
  fault-in, and the first network packet.

```bash
python3 baremetal/tools/kvm_bench.py --accel tcg --icount 0 --machine microvm,acpi=on --runs 3 \
    --expect-path pvh \
    --kernel build/baremetal/tsc-1000000000-apic-1000000000/kernel-pvh.elf \
    --rootserver build/baremetal/tsc-1000000000-apic-1000000000/rootserver.elf
```

With `/dev/kvm` (e.g. on the NUC), drop `--accel tcg --icount 0` and build with
the host's TSC frequency and `--apic-hz 1000000000`. KVM's default APIC rate is
1 GHz, configurable since Linux 6.11, and TDX uses 25 MHz.

The probe's virtualization section reports what a microVM host depends on:

- VT-x state in firmware
- EPT, VPID and unrestricted guest
- APICv and posted interrupts
- PML, TSC scaling, and 1 GiB EPT pages
- VT-d interrupt remapping and posted interrupts
- SR-IOV

**Where seL4 fits** (from an independent architecture review):

- seL4 as a guest (PVH) adds no isolation against the host. Linux, KVM and the
  VMM still underpin it.
- On x86_64, seL4's proofs cover functional correctness of the verified
  configuration only: not VT-x, the IOMMU, SMP, MCS, or these debug builds.

The pragmatic split is to run agent sandboxes as Linux guests on Firecracker or
Cloud Hypervisor, and to use seL4 for small trusted components such as a
credential or policy broker, as a microVM here, or on AArch64/RISC-V where the
proofs go further.

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
