# seL4 Microkernel OS - Formally Verified

A bootable, formally verified operating system using seL4 and Microkit.

## Platform Support

| Platform | Framework | Verification Status | Build Ready |
|----------|-----------|---------------------|-------------|
| **AArch64** | Microkit | Binary-level proofs | ✅ |
| **RISC-V 64** | Microkit | Functional proofs | ✅ |
| **x86_64** | seL4 direct | Functional proofs | ✅ |

## What Makes This Special

- **seL4**: The world's most secure microkernel with mathematical proofs of correctness
- **Microkit**: Official seL4 SDK for building multi-component systems
- **Rust**: Memory-safe userspace code
- **Verus**: Additional formal verification for Rust components

## Project Structure

```
sel4-microkernel/
├── microkit-hello/           # Microkit system (AArch64 + RISC-V)
│   ├── src/main.rs           # Rust protection domain with Verus specs
│   ├── hello.system          # System description
│   ├── Makefile              # Build system (macOS/Linux)
│   ├── setup.sh              # SDK download script
│   └── support/targets/      # Rust target specifications
├── sel4-x86_64/              # seL4 for x86_64 (no Microkit support)
│   ├── src/                  # Rust rootserver
│   └── scripts/
├── verified/                 # Verus-verified Rust components
│   └── src/lib.rs
└── README.md
```

## Quick Start

### Prerequisites

**macOS:**
```bash
brew install qemu aarch64-elf-gcc riscv64-elf-gcc
rustup install nightly
rustup component add rust-src --toolchain nightly
```

**Linux:**
```bash
sudo apt install qemu-system-arm qemu-system-misc \
    gcc-aarch64-linux-gnu gcc-riscv64-linux-gnu
rustup install nightly
rustup component add rust-src --toolchain nightly
```

### Microkit (AArch64 / RISC-V)

```bash
cd microkit-hello
./setup.sh                   # Download Microkit SDK 2.1.0

make ARCH=aarch64            # Build for ARM64
make run ARCH=aarch64        # Boot in QEMU

make ARCH=riscv64            # Build for RISC-V
make run ARCH=riscv64        # Boot in QEMU
```

Press `Ctrl-A X` to exit QEMU.

### seL4 Direct (x86_64)

```bash
cd sel4-x86_64
./scripts/setup.sh           # Set up seL4 build environment
./scripts/build.sh           # Build kernel + rootserver
./scripts/run.sh             # Boot in QEMU
```

## seL4 Formal Verification

seL4's proofs guarantee:

1. **Functional Correctness**: The implementation matches the specification exactly
2. **Integrity**: Memory isolation cannot be violated
3. **Confidentiality**: Information cannot leak between partitions
4. **Availability**: The kernel cannot be made to fail unexpectedly

The proofs cover ~10,000 lines of C code and 600,000+ lines of Isabelle/HOL.

## Microkit Architecture

```
┌─────────────────────────────────────────────────┐
│                  Application                     │
├─────────────┬─────────────┬─────────────────────┤
│     PD 1    │     PD 2    │        PD 3         │
│   (Rust)    │   (Rust)    │       (Rust)        │
├─────────────┴─────────────┴─────────────────────┤
│              seL4 Microkernel                    │
│         (Formally Verified C Code)               │
├─────────────────────────────────────────────────┤
│                 Hardware                         │
└─────────────────────────────────────────────────┘

PD = Protection Domain (isolated component)
```

Protection Domains communicate via:
- **Protected Procedure Calls (PPC)**: Synchronous RPC
- **Notifications**: Asynchronous signals
- **Shared Memory**: Mapped memory regions

## Verus Verification

Additional verification of Rust components using Verus:

```bash
cd verified
../verus/run.sh           # Verify with Verus
cargo test                # Run tests
```

Example verified code:

```rust
verus! {
    pub fn capability_derive(parent: Capability, child_mask: u64) -> Capability
        requires
            child_mask & !parent.rights == 0,  // Can only reduce rights
        ensures
            result.rights == parent.rights & child_mask,
    {
        Capability { rights: parent.rights & child_mask }
    }
}
```

## References

- [seL4 Website](https://sel4.systems/)
- [Microkit GitHub](https://github.com/seL4/microkit)
- [Microkit Manual](https://github.com/seL4/microkit/blob/main/docs/manual.md)
- [rust-sel4](https://github.com/seL4/rust-sel4)
- [seL4 Proofs (l4v)](https://github.com/seL4/l4v)
- [Verus](https://github.com/verus-lang/verus)

## License

MIT
