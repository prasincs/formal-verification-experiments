# Unified Build System for seL4 Microkit

A unified build system supporting multiple platforms and products for seL4 Microkit development.

## Quick Start

```bash
cd build-system

# Build and run hello world on QEMU AArch64
make PRODUCT=hello PLATFORM=qemu-aarch64 run

# Build graphics demo for Raspberry Pi 4
make PRODUCT=graphics PLATFORM=rpi4

# Create bootable SD card image
make PRODUCT=graphics PLATFORM=rpi4 sdcard
```

## Products

| Product | Description | Platforms |
|---------|-------------|-----------|
| `graphics` | HDMI framebuffer demo | rpi4 |
| `hello` | Hello World demo | qemu-aarch64, qemu-riscv64 |
| `tvdemo` | TV demo with input | rpi4 |

## Platforms

| Platform | Description |
|----------|-------------|
| `rpi4` | Raspberry Pi 4 hardware |
| `qemu-aarch64` | QEMU AArch64 virtual machine |
| `qemu-riscv64` | QEMU RISC-V 64-bit virtual machine |

## Targets

| Target | Description |
|--------|-------------|
| `all` | Build system image (default) |
| `run` | Run in QEMU (QEMU platforms only) |
| `run-debug` | Run with GDB server |
| `sdcard` | Create SD card image (rpi4 only) |
| `sdcard-uboot` | SD card with U-Boot |
| `write-sdcard` | Write image to SD card (requires DEVICE=) |
| `firmware` | Download RPi firmware |
| `uboot` | Build U-Boot |
| `bootfiles` | Create boot files directory |
| `elf` | Build loader.elf |
| `clean` | Remove build artifacts |
| `distclean` | Remove all build artifacts including cargo cache |
| `info` | Show build configuration |
| `check` | Verify prerequisites |
| `setup-sdk` | Download Microkit SDK |
| `help` | Show help message |

## Configuration Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PRODUCT` | (required) | Product to build |
| `PLATFORM` | (required) | Target platform |
| `RPI4_MEMORY` | `4gb` | RPi4 memory variant (1gb/2gb/4gb/8gb) |
| `MICROKIT_SDK` | `../microkit-sdk` | Path to Microkit SDK |
| `MICROKIT_CONFIG` | `debug` | Build configuration (debug/release) |
| `CONFIG_*` | per defconfig | Kconfig option overrides (see below) |
| `DEFCONFIG` | `configs/<product>_defconfig` | Alternate defconfig file |

## Kconfig-Style Configuration

Feature options are declared in [`Kconfig`](Kconfig) and resolved per product
from three layers (later wins): Kconfig `default` lines, the product's
`configs/<product>_defconfig`, and `CONFIG_*=y|n` on the make command line.

```bash
# Photoframe defaults to serial input only; add a USB keyboard:
make PRODUCT=photoframe PLATFORM=rpi4 CONFIG_INPUT_USB_KEYBOARD=y sdcard

# See the resolved configuration:
make PRODUCT=tvdemo PLATFORM=rpi4 ISOLATED=1 info
```

The resolved configuration does two things:

1. **Cargo features** — `CONFIG_INPUT_*` options select which input drivers
   are compiled into `input_pd` (`--no-default-features --features ...`).
2. **System descriptions** — `.system` files may guard blocks with
   `<!-- @if CONFIG_X --> ... <!-- @endif -->`; guarded device mappings are
   stripped unless the option is enabled, so a PD is only granted MMIO for
   drivers it actually contains (least privilege).

`scripts/kconfig.sh` implements both steps (POSIX sh + awk, no external
kconfig tooling); `scripts/test-kconfig.sh` is its self-test, run in CI.
See `../docs/build-configuration.md` for the full description.

## Examples

```bash
# Hello world on QEMU RISC-V
make PRODUCT=hello PLATFORM=qemu-riscv64 run

# Graphics demo for RPi4 with 8GB
make PRODUCT=graphics PLATFORM=rpi4 RPI4_MEMORY=8gb sdcard

# Write SD card image directly to device
make PRODUCT=graphics PLATFORM=rpi4 write-sdcard DEVICE=/dev/sdb

# Just build (no run)
make PRODUCT=hello PLATFORM=qemu-aarch64

# Show what would be built
make PRODUCT=graphics PLATFORM=rpi4 info
```

## Backward Compatibility

Existing project Makefiles continue to work:

```bash
cd rpi4-graphics
make sdcard          # Works, forwards to unified system

cd microkit-hello
make run ARCH=aarch64  # Works, forwards to unified system
```

## Directory Structure

```
build-system/
├── Makefile                    # Main entry point
├── Kconfig                     # Feature option declarations (CONFIG_*)
├── config/
│   ├── defaults.mk            # OS detection, shared paths
│   ├── versions.mk            # Pinned version numbers
│   ├── kconfig.mk             # Kconfig integration (features, .system gen)
│   ├── platforms/             # Platform configurations
│   │   ├── rpi4.mk
│   │   ├── qemu-aarch64.mk
│   │   └── qemu-riscv64.mk
│   └── products/              # Product configurations
│       ├── graphics.mk
│       ├── hello.mk
│       └── tvdemo.mk
├── configs/                    # Per-product default configurations
│   ├── tvdemo_defconfig
│   └── photoframe_defconfig
├── include/                    # Build rules
│   ├── rust.mk                # Cargo/Rust rules
│   ├── microkit.mk            # Microkit tool rules
│   ├── qemu.mk                # QEMU run rules
│   └── sdcard.mk              # SD card creation
├── scripts/                    # Build scripts
│   ├── create-sdcard.sh
│   ├── download-sdk.sh
│   ├── build-uboot.sh
│   ├── detect-toolchain.sh
│   ├── kconfig.sh             # Kconfig resolver + .system preprocessor
│   └── test-kconfig.sh        # Self-test for kconfig.sh (run in CI)
└── targets/                    # Rust target specs
    ├── aarch64-sel4-microkit.json
    └── riscv64gc-sel4-microkit.json
```

## Pinned Versions

For reproducible builds, all external dependencies use pinned versions:

- **Microkit SDK**: 2.1.0
- **RPi Firmware**: 1.20250915
- **U-Boot**: v2025.10

See `config/versions.mk` for details.
