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
| `0001-bootprof-*` | TSC checkpoints. A console for serial-less machines: COM1, plus the boot loader's framebuffer (EGA text on BIOS, UEFI GOP with a public-domain 8x8 font). Output is buffered until the root task's first print, so console I/O doesn't skew the timestamps. | no (instrumentation) |
| `0002-x86-boot-module-placement` | Load the root task in place when the boot module doesn't overlap its destination. Upstream asserts the module is above the kernel, and ELF-loads into scratch past it, which falls off the end of RAM when a PVH loader puts the module at the top of RAM. | candidate |
| `0003-x86-pvh-boot-and-acpi-xsdt` | PVH entry (`XEN_ELFNOTE_PHYS32_ENTRY` and an `hvm_start_info` parser), plus ACPI XSDT support. QEMU `microvm`, Firecracker and Cloud Hypervisor publish XSDT-only tables; upstream reads only the RSDT. | candidate |
| `0004-x86-pinnable-apic-timer-frequency` | `KernelPC99APICTimerFrequency`, like the existing TSC option. In the non-MCS kernel, `apic_init()` otherwise always measures the APIC timer against the PIT (50–100 ms). | candidate |

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
BOOTPROF user tsc_mhz=… user_us=…                # root task main()
```

- `entry_us` is roughly firmware + boot loader time. `kernel_us` is seL4's
  boot. `user_us - exit_us` is kernel exit to root-task entry.
- The screen console wraps to the top instead of scrolling; scrolling would
  copy megabytes of uncached video memory per line. If the root task faults
  before its first print, buffered kernel output only appears on a kernel halt.

### Toward microVMs: PVH, microvm, and an instruction-count baseline

`kernel-pvh.elf` is `kernel32.elf` with its multiboot1 magic zeroed
(`baremetal/tools/pvh_only.py` checks the PVH note first). QEMU prefers
multiboot whenever both are present; Firecracker and Cloud Hypervisor only use
PVH. Without KVM, QEMU's `-icount` makes guest time an instruction count
(1 insn = 1 ns): runs are bit-identical and comparable across configurations,
but it does not model I/O latency or VM-exit cost. Results with TSC and APIC
both pinned (`scripts/test-baremetal.sh` asserts them):

| Machine | Boot | `entry_us` (firmware) | `kernel_us` (seL4) |
|---|---|---:|---:|
| `pc` (SeaBIOS) | PVH | 5728 | 1085 |
| `microvm` (qboot) | PVH | 5963 | 1074 |
| `microvm` (qboot) | multiboot | 5913 | 1583 |
| `microvm`, clocks not pinned | PVH | 5969 | 201275 |

So seL4's own boot is about 1.07 M instructions. That's roughly 0.3–0.5 ms on
a 2.4 GHz core (an estimate; not measured on hardware). Under QEMU, firmware
dominates, and Firecracker and Cloud Hypervisor have none. Multiboot costs about
0.5 M more instructions, because its module sits where the root task goes, so
the root task is loaded into scratch space and copied down.

```bash
python3 baremetal/tools/kvm_bench.py --accel tcg --icount 0 --machine microvm,acpi=on --runs 3 \
    --kernel build/baremetal/tsc-1000000000-apic-1000000000/kernel-pvh.elf \
    --rootserver build/baremetal/tsc-1000000000-apic-1000000000/rootserver.elf
```

With `/dev/kvm` (e.g. on the NUC), drop `--accel tcg --icount 0` and build with
the host's TSC frequency and `--apic-hz 1000000000`. `host_ms` (QEMU spawn to
the root task's first serial line) then becomes meaningful. The probe's
virtualization section reports what a microVM host depends on:

- VT-x state in firmware
- EPT, VPID and unrestricted guest
- APICv and posted interrupts
- PML (hardware dirty logging, for snapshots)
- TSC scaling, and 1 GiB EPT pages
- VT-d interrupt remapping and posted interrupts
- SR-IOV, and the `kvm_intel` module parameters

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
