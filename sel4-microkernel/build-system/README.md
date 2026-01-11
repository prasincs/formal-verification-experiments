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
├── config/
│   ├── defaults.mk            # OS detection, shared paths
│   ├── versions.mk            # Pinned version numbers
│   ├── platforms/             # Platform configurations
│   │   ├── rpi4.mk
│   │   ├── qemu-aarch64.mk
│   │   └── qemu-riscv64.mk
│   └── products/              # Product configurations
│       ├── graphics.mk
│       ├── hello.mk
│       └── tvdemo.mk
├── include/                    # Build rules
│   ├── rust.mk                # Cargo/Rust rules
│   ├── microkit.mk            # Microkit tool rules
│   ├── qemu.mk                # QEMU run rules
│   └── sdcard.mk              # SD card creation
├── scripts/                    # Build scripts
│   ├── create-sdcard.sh
│   ├── download-sdk.sh
│   ├── build-uboot.sh
│   └── detect-toolchain.sh
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
