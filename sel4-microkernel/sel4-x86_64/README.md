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
