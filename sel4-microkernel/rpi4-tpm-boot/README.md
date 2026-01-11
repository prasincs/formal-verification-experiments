# TPM 2.0 Boot Verification for seL4/Microkit

Verified TPM 2.0 boot measurement and remote attestation library for
Raspberry Pi 4 with seL4 microkernel and Microkit framework.

## Hardware Support

This library is designed for the **GeeekPi TPM9670** module featuring the
**Infineon Optiga SLB 9670** TPM 2.0 chip.

### Supported Hardware

| Module | Chip | Interface | Status |
|--------|------|-----------|--------|
| GeeekPi TPM9670 | Infineon SLB 9670 | SPI | ✅ Primary |
| LetsTrust TPM | Infineon SLB 9672 | SPI | ✅ Compatible |
| Generic TPM 2.0 | Any TCG-compliant | SPI | ⚠️ May work |

### GPIO Pinout (Raspberry Pi 4)

| TPM Pin | RPi GPIO | Physical Pin | Function |
|---------|----------|--------------|----------|
| SCLK | GPIO 11 | Pin 23 | SPI Clock |
| MOSI | GPIO 10 | Pin 19 | Master Out |
| MISO | GPIO 9 | Pin 21 | Master In |
| CS | GPIO 8 | Pin 24 | Chip Select |
| RST | GPIO 24 | Pin 18 | Reset (opt) |
| IRQ | GPIO 25 | Pin 22 | Interrupt |

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    seL4 Microkernel                              │
│              (Formally verified, capability-based)               │
└─────────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          │                   │                   │
          ▼                   ▼                   ▼
    ┌──────────┐       ┌──────────┐       ┌──────────┐
    │  TPM PD  │◄─────►│Graphics  │       │ Input PD │
    │(Isolated)│  IPC  │   PD     │       │          │
    └────┬─────┘       └──────────┘       └──────────┘
         │
         ▼ SPI (10 MHz)
    ┌──────────────────┐
    │  Infineon SLB    │
    │  9670 TPM 2.0    │
    │  (Hardware RoT)  │
    └──────────────────┘
```

## Boot Measurement Chain

The TPM maintains Platform Configuration Registers (PCRs) that record
measurements of each boot stage:

| PCR | Contents | Extended By |
|-----|----------|-------------|
| 0 | Firmware (bootcode.bin, start4.elf) | VideoCore |
| 1 | seL4 Kernel Image | Bootloader |
| 2 | Microkit System Configuration | Bootloader |
| 3 | Protection Domain Images | Kernel |
| 4 | Runtime Measurements | PDs |
| 7 | Secure Boot Policy | Firmware |

### PCR Extension

Each measurement extends the PCR using:
```
PCR_new = SHA-256(PCR_old || measurement)
```

This creates an unforgeable chain - any modification to a boot component
will result in different final PCR values.

## Formal Verification

This library uses [Verus](https://github.com/verus-lang/verus) for
formal verification of critical components:

### Verified Properties

1. **PCR Index Safety**: All PCR indices proven in range (0-23)
2. **Measurement Chain Integrity**: Extension operations are correct
3. **Policy Evaluation Soundness**: Policy checks are complete
4. **Constant-Time Comparison**: Digest comparisons resist timing attacks

### Enabling Verification

```bash
# Build with Verus verification
cargo build --features verus

# Run Verus proofs
verus src/lib.rs
```

## Usage

### Basic Boot Measurement

```rust
use rpi4_tpm_boot::{BootChain, BootStage, compute_sha256};

// Create boot chain
let mut chain = BootChain::new();

// Measure kernel
let kernel_data: &[u8] = /* load kernel image */;
let digest = chain.measure_component(
    BootStage::Kernel,
    0x0001,  // Component ID
    kernel_data,
)?;

// Measure system config
let system_data: &[u8] = /* load system XML */;
chain.measure_component(BootStage::System, 0x0002, system_data)?;

// Verify chain integrity
assert!(chain.replay_and_verify());
```

### TPM Hardware Access

```rust
use rpi4_tpm_boot::{Slb9670Tpm, Sha256Digest};

// Create TPM driver (with mapped register addresses)
let mut tpm = Slb9670Tpm::new(SPI_BASE, GPIO_BASE);

// Initialize
tpm.startup()?;
tpm.self_test(true)?;

// Extend PCR with measurement
let digest = compute_sha256(b"kernel image data");
tpm.pcr_extend(1, &digest)?;

// Get random bytes
let mut random = [0u8; 32];
tpm.get_random(&mut random)?;
```

### Remote Attestation

```rust
use rpi4_tpm_boot::attestation::{
    AttestationRequest, AttestationVerifier, PcrSelection
};

// Verifier creates challenge
let nonce = [0x42u8; 32];
let request = AttestationRequest::boot_attestation(nonce);

// Prover generates quote (on TPM PD)
// let response = tpm_pd.quote(&request)?;

// Verifier validates
let verifier = AttestationVerifier::new();
// verifier.set_expected_pcr(0, expected_firmware_digest);
// let result = verifier.verify(&request, &response);
```

## Microkit Integration

### System Description

```xml
<system>
    <!-- TPM Protection Domain -->
    <protection_domain name="tpm" priority="250" pp="true">
        <map mr="spi_regs" vaddr="0x5_0100_0000" perms="rw"
             cached="false" setvar_vaddr="spi_base" />
        <map mr="gpio_regs" vaddr="0x5_0200_0000" perms="rw"
             cached="false" setvar_vaddr="gpio_base" />
        <program_image path="tpm_pd.elf" />
    </protection_domain>

    <!-- Graphics PD can request attestation -->
    <protection_domain name="graphics" priority="150">
        <!-- ... -->
    </protection_domain>

    <!-- IPC channel for TPM requests -->
    <channel>
        <end pd="tpm" id="0" />
        <end pd="graphics" id="2" />
    </channel>

    <!-- Hardware memory regions -->
    <memory_region name="spi_regs" size="0x1000"
                   phys_addr="0xfe204000" />
    <memory_region name="gpio_regs" size="0x1000"
                   phys_addr="0xfe200000" />
</system>
```

## Security Considerations

### Threat Model

- **In Scope**: Software attacks, boot-time tampering, remote verification
- **Out of Scope**: Physical attacks on TPM chip, side-channel attacks on TPM

### Trust Boundaries

1. **TPM Hardware**: Root of trust, tamper-resistant
2. **seL4 Kernel**: Formally verified, trusted
3. **TPM PD**: Isolated, sole TPM access
4. **Other PDs**: Untrusted, capability-limited

### Best Practices

1. Always verify TPM device ID before use
2. Use constant-time comparison for digests
3. Include nonce in all attestation requests
4. Keep golden measurements in secure storage
5. Rotate attestation keys periodically

## Building

```bash
# Build library
cd sel4-microkernel/rpi4-tpm-boot
cargo build --release

# Build TPM protection domain
cd ../rpi4-tpm-pd
cargo build --release --target aarch64-sel4-microkit

# Build complete system (requires Microkit SDK)
cd ../build-system
make PRODUCT=tpm-demo PLATFORM=rpi4
```

## Testing

```bash
# Run unit tests
cargo test

# Run with Verus verification
verus --crate-type=lib src/lib.rs

# Hardware test (requires RPi4 + TPM module)
# Flash SD card and check serial output
```

## References

- [TCG TPM 2.0 Library Specification](https://trustedcomputinggroup.org/resource/tpm-library-specification/)
- [TCG PC Client Platform TPM Profile (PTP)](https://trustedcomputinggroup.org/resource/pc-client-platform-tpm-profile-ptp-specification/)
- [Infineon SLB 9670 Datasheet](https://www.infineon.com/cms/en/product/security-smart-card-solutions/optiga-embedded-security-solutions/optiga-tpm/slb-9670/)
- [seL4 Microkernel](https://sel4.systems/)
- [Microkit Framework](https://github.com/seL4/microkit)
- [Verus Verification](https://github.com/verus-lang/verus)

## License

MIT License - See LICENSE file for details.
