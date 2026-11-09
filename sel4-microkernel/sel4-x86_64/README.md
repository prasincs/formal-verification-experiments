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

`scripts/build-baremetal.sh` builds a legacy-BIOS GRUB image: seL4 (pinned to
the commit rust-sel4's `Cargo.lock` revision targets) with
`baremetal/patches/0001-bootprof-*.patch`, plus this root task. The patch
records TSC checkpoints and mirrors the kernel console to the VGA text buffer,
for machines without a serial port. It is a measurement build: debug +
printing on, IOMMU off. Not the verified configuration.

```bash
pip3 install ply jinja2 pyyaml        # + git cmake ninja gcc grub-pc-bin xorriso
scripts/test-baremetal.sh             # QEMU regression test (both TSC modes)
scripts/build-baremetal.sh            # -> build/baremetal/tsc-0/sel4-bootprof.iso
sudo dd if=build/baremetal/tsc-0/sel4-bootprof.iso of=/dev/sdX bs=4M conv=fsync
```

On the target (e.g. Intel NUC8): in BIOS setup (F2) enable **Legacy Boot**
and disable Secure Boot, then pick the USB stick from the F10 menu. Output:

```
BOOTPROF kernel tsc_mhz=… tsc_init_us=…          # tsc_init: ~100 ms PIT wait
BOOTPROF kernel entry_us=… exit_us=… kernel_us=… # µs since power-on (TSC)
BOOTPROF user tsc_mhz=… user_us=…                # root task main()
```

- `entry_us` ≈ firmware + GRUB; `kernel_us` = seL4 boot; `user_us - exit_us`
  = kernel → root task.
- seL4 only reads the TSC frequency from an MSR on Nehalem–Skylake model IDs;
  Kaby/Coffee Lake (NUC8) fall back to the PIT. Rebuild with the printed
  frequency to remove it: `scripts/build-baremetal.sh --tsc-hz 3100000000`.
- `kernel_us` includes console output. With no UART decoded at `0x3F8` that
  costs microseconds; a live UART at 115200 baud adds ~87 µs per character.

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
