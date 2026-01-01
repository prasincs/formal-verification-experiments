# seL4 Microkit Hello World

A minimal seL4 Microkit system demonstrating formally verified OS components running on AArch64 and RISC-V.

## Requirements

### macOS
```bash
brew install qemu aarch64-elf-gcc riscv64-elf-gcc
```

### Linux
```bash
sudo apt install qemu-system-arm qemu-system-misc \
    gcc-aarch64-linux-gnu gcc-riscv64-linux-gnu
```

### Rust
```bash
rustup install nightly
rustup component add rust-src --toolchain nightly
```

## Build & Run

```bash
./setup.sh              # Download Microkit SDK (one-time)

make ARCH=aarch64       # Build for ARM64
make run ARCH=aarch64   # Run in QEMU

make ARCH=riscv64       # Build for RISC-V
make run ARCH=riscv64   # Run in QEMU
```

Press `Ctrl-A X` to exit QEMU.

## What's Demonstrated

The protection domain initializes and demonstrates:

1. **Verified capability derivation** — Child capabilities cannot exceed parent rights (Verus-proven)
2. **Overflow-safe counter** — Increment operations proven to never overflow

## Project Structure

```
├── src/main.rs         # Protection domain code with Verus specs
├── hello.system        # Microkit system description
├── Makefile            # Build system (auto-detects macOS/Linux)
├── setup.sh            # SDK download script
└── support/targets/    # Rust target specifications
```

## Verification Stack

| Layer | Technology | Guarantees |
|-------|-----------|------------|
| Kernel | seL4 (Isabelle/HOL) | Functional correctness, isolation, no crashes |
| Userspace | Rust + Verus | Memory safety, verified invariants |

## SDK Version

This project uses Microkit SDK 2.1.0 with rust-sel4 bindings.
