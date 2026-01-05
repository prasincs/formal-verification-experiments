# seL4 Microkit Graphics Demo - Raspberry Pi 4

A bootable SD card image demonstrating seL4 Microkit on Raspberry Pi 4 with
framebuffer graphics output.

> **⚠️ Important**: Microkit SDK 2.1.0 has a boot issue on RPi4B. You must either:
> 1. Build the SDK from source with the fix (see [Building Microkit SDK from Source](#building-microkit-sdk-from-source)), or
> 2. Wait for SDK 2.2.0+ which will include the fix
>
> The framebuffer graphics demo itself is still work in progress.

## What It Does

When booted, the system:

1. Initializes seL4 microkernel on the Cortex-A72
2. Starts a Graphics Protection Domain
3. Allocates a 1280x720 framebuffer via VideoCore mailbox
4. Draws an architecture diagram showing the seL4 stack
5. Outputs debug info on serial UART

```
┌─────────────────────────────────────────────────────────────┐
│              SEL4 MICROKIT ARCHITECTURE                      │
│         Raspberry Pi 4 - Formally Verified Microkernel       │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────┐   ┌──────────┐   ┌──────────┐                 │
│  │GRAPHICS  │   │  APP     │   │ DRIVER   │   USER SPACE    │
│  │   PD     │   │   PD     │   │   PD     │                 │
│  └────┬─────┘   └────┬─────┘   └────┬─────┘                 │
│       └──────────────┼──────────────┘                       │
│                      ▼                                       │
│  ┌───────────────────────────────────────────┐              │
│  │              MICROKIT                      │  FRAMEWORK   │
│  └───────────────────┬───────────────────────┘              │
│                      ▼                                       │
│  ┌───────────────────────────────────────────┐              │
│  │           SEL4 KERNEL                      │  KERNEL     │
│  │      Formally Verified (Isabelle/HOL)      │              │
│  └───────────────────┬───────────────────────┘              │
│                      ▼                                       │
│  ┌───────────────────────────────────────────┐              │
│  │     RASPBERRY PI 4 - BCM2711               │  HARDWARE    │
│  └───────────────────────────────────────────┘              │
└─────────────────────────────────────────────────────────────┘
```

## Requirements

### Hardware
- Raspberry Pi 4 Model B (4GB or 8GB)
- MicroSD card (8GB+)
- HDMI display
- Optional: USB-to-serial adapter for debug output
- Optional: TPM 2.0 module for measured boot (see [TPM Support](#tpm-20-support))

### Software
- Linux or macOS
- Rust nightly (`rustup install nightly && rustup component add rust-src`)
- Cross compiler:
  - Linux: `sudo apt install gcc-aarch64-linux-gnu`
  - macOS: `brew install aarch64-elf-gcc`
- mtools (for creating SD card images without root):
  - Linux: `sudo apt install mtools` or `sudo pacman -S mtools`
  - macOS: `brew install mtools`
- Microkit SDK with RPi4 support

## Quick Start

### 1. Set up Microkit SDK

**Option A: Download pre-built SDK (requires patching for RPi4B)**

```bash
# Download SDK 2.1.0
curl -LO https://github.com/seL4/microkit/releases/download/2.1.0/microkit-sdk-2.1.0-linux-x86-64.tar.gz
mkdir -p microkit-sdk
tar -xzf microkit-sdk-*.tar.gz -C microkit-sdk --strip-components=1
export MICROKIT_SDK=$PWD/microkit-sdk

# IMPORTANT: SDK 2.1.0 has a boot bug on RPi4B - see "Building Microkit SDK from Source" below
```

**Option B: Build SDK from source with RPi4B fix (recommended)**

See [Building Microkit SDK from Source](#building-microkit-sdk-from-source) section below.

### 2. Build the System and Create SD Card Image

```bash
cd rpi4-graphics

# Build everything and create bootable SD card image (single command)
make sdcard RPI4_MEMORY=4gb
```

This will:
1. Build the seL4/Microkit system for Raspberry Pi 4
2. Download Raspberry Pi firmware
3. Create a bootable FAT32 SD card image

### 3. Flash to SD Card

**Option A: Using dd (Linux/macOS)**
```bash
# Find your SD card device (e.g., /dev/sdb or /dev/mmcblk0)
lsblk

# Flash the image
sudo dd if=build/rpi4-sel4-graphics.img of=/dev/sdX bs=4M status=progress conv=fsync
sync
```

**Option B: Using Raspberry Pi Imager**
1. Open Raspberry Pi Imager
2. Choose OS → Use custom → Select `build/rpi4-sel4-graphics.img`
3. Choose storage → Select your SD card
4. Write

### 4. Boot

1. Insert SD card into Raspberry Pi 4
2. Connect HDMI display
3. Power on
4. You should see the architecture diagram on screen!

## Debug Output

Connect a USB-serial adapter to GPIO pins:
- TX (GPIO 14, pin 8)
- RX (GPIO 15, pin 10)
- GND (pin 6)

Then:
```bash
screen /dev/ttyUSB0 115200
```

You'll see output like:
```
=====================================
  seL4 Microkit Graphics Demo
  Raspberry Pi 4
=====================================

Firmware revision: 0x5eaf1234
Board model: 0x00000011
Board serial: 0x10000000abcd1234

Framebuffer allocated: 1280x720 @ 0x3c100000, pitch=5120
Drawing architecture diagram...
Architecture diagram complete!

Graphics PD initialized. Entering event loop...
```

## Project Structure

```
rpi4-graphics/
├── src/
│   ├── lib.rs          # Library root
│   ├── main.rs         # Protection Domain entry point
│   ├── mailbox.rs      # VideoCore mailbox driver
│   ├── framebuffer.rs  # Framebuffer allocation & primitives
│   ├── graphics.rs     # Drawing primitives (colors, shapes)
│   ├── font.rs         # 8x8 bitmap font
│   └── tpm.rs          # TPM 2.0 driver (ST33K via SPI)
├── graphics.system     # Microkit system description
├── Makefile            # Build system
├── Cargo.toml
└── README.md
```

## Configuration

### Display Resolution

Edit `src/main.rs`:
```rust
const SCREEN_WIDTH: u32 = 1920;   // Change resolution
const SCREEN_HEIGHT: u32 = 1080;
```

### Pi 4 Memory Variant

```bash
make RPI4_MEMORY=4gb   # For 4GB model (default)
make RPI4_MEMORY=8gb   # For 8GB model
```

## Verification Status

| Component | Verification |
|-----------|--------------|
| seL4 kernel | ✅ Isabelle/HOL (binary proof for ARM) |
| Microkit framework | ✅ Designed for verified systems |
| SHA-256 | ✅ RustCrypto sha2 (audited) |
| Constant-time compare | ✅ Verus-verified (timing-safe) |
| Color operations | ✅ Verus-verified (ARGB round-trip) |
| Rect containment | ✅ Verus-verified (bounds logic) |
| Pixel bounds check | ✅ Verus-verified (no OOB writes) |
| Framebuffer alloc | ⚠️ Trusted (hardware interface) |
| TPM driver | ⚠️ Trusted (hardware interface) |
| VideoCore firmware | ❌ Closed source (display not verifiable) |

## TPM 2.0 Support

Optional TPM 2.0 support enables **measured boot** and **remote attestation**.

### Compatible TPM Modules

| Board | Chip | Interface | Availability | Where to Buy |
|-------|------|-----------|--------------|--------------|
| **GeeekPi TPM9670** | Infineon SLB9670 | SPI | ✅ In Stock | [Amazon](https://www.amazon.com/GeeekPi-Raspberry-Infineon-OptigaTM-Compatible/dp/B09G2BZQT5) |
| **LetsTrust TPM** | Infineon SLB9672 | SPI | ✅ Ships from EU | [buyzero.de](https://buyzero.de/en/products/letstrust-hardware-tpm-trusted-platform-module), [Pi Hut](https://thepihut.com/products/letstrust-tpm-for-raspberry-pi) |
| STPM4RasPI | ST33TPHF2XSPI | SPI | ⚠️ 19 week lead | [Newark](https://www.newark.com/stmicroelectronics/sct-tpm-raspihe4/trusted-platform-module-st33/dp/49AM3002) |

All boards plug directly onto the Raspberry Pi 4's 40-pin GPIO header. The Infineon SLB9670/SLB9672 chips use the same Linux driver (`tpm-slb9670`) as the ST33.

### TPM Features

| Feature | Description |
|---------|-------------|
| Measured Boot | Each boot stage extends its hash into TPM PCRs |
| Remote Attestation | Cryptographic proof of system state to verifier |
| Sealed Secrets | Keys bound to specific PCR values |
| Hardware RNG | True random number generation |

### GPIO Pinout

```
STPM4RasPI → Raspberry Pi 4
─────────────────────────────
MOSI       → GPIO 10 (Pin 19)
MISO       → GPIO 9  (Pin 21)
SCLK       → GPIO 11 (Pin 23)
CS         → GPIO 8  (Pin 24)
RST        → GPIO 24 (Pin 18)
VCC        → 3.3V    (Pin 1)
GND        → GND     (Pin 6)
```

### PCR Allocation

| PCR | Contents |
|-----|----------|
| 0 | Firmware (bootcode.bin, start4.elf) |
| 1 | seL4 kernel |
| 2 | Microkit system config |
| 3 | Protection Domain images |
| 4 | Runtime measurements |

See [ARCHITECTURE.md](../rpi-graphics/ARCHITECTURE.md) for detailed TPM integration docs.

## U-Boot Debug Boot

For debugging, you can boot via U-Boot which provides HDMI console output:

```bash
# Create image with U-Boot bootloader
./scripts/create-sdcard-full.sh --uboot

# Flash to SD card
sudo dd if=build/rpi4-sel4-full.img of=/dev/sdX bs=4M status=progress conv=fsync
```

U-Boot will display ASCII art and system info before loading seL4. Use `bdinfo`
at the U-Boot prompt to find the framebuffer address.

## Known Issues

### Microkit SDK 2.1.0 Boot Failure on RPi4B (FIXED)

**Status**: ✅ Fixed by building SDK from source

**Symptom**: After U-Boot's `go 0x10000000` command, the loader crashes immediately:
```
LDR|INFO: disabling MMU (if it was enabled)
LDR|ERROR: loader trapped exception: Synchronous (Current Exception level with SP_ELx)
esr_el2: 0x0000000002000000
ec: 0x00000000 (Unknown reason)
```

**Root Cause**: The Microkit loader makes a PSCI (Power State Coordination Interface)
SMC call during boot to check PSCI version. The Raspberry Pi 4's firmware doesn't
properly handle this call, causing an exception.

**Fix**: [PR #402](https://github.com/seL4/microkit/pull/402) (merged Nov 28, 2025)
skips the PSCI check on BCM2711 platforms. This fix was merged 2 days AFTER SDK 2.1.0
was released (Nov 26, 2025).

**Solution**: Build the SDK from source - see [Building Microkit SDK from Source](#building-microkit-sdk-from-source).

### Framebuffer Graphics Not Working

**Status**: seL4 loads and runs, but framebuffer writes don't appear on screen.

**What works**:
- U-Boot boots and displays on HDMI ✅
- seL4 kernel starts (via U-Boot `go` command) ✅
- Protection Domain init function runs ✅

**What doesn't work**:
- Writing to mapped framebuffer memory doesn't produce visible output ❌

**Investigation notes**:
- Framebuffer physical address from U-Boot `bdinfo`: `0x3e876000`
- This address is mapped into the Protection Domain at virtual address `0x5_0001_0000`
- Memory region mapping appears correct in Microkit report
- May require serial adapter for proper debugging

**Possible causes**:
1. GPU framebuffer is invalidated when seL4 takes over from U-Boot
2. Cache coherency issues with device memory
3. seL4/Microkit memory protection preventing writes
4. Need to re-initialize framebuffer via VideoCore mailbox

**No official documentation exists** for framebuffer graphics on seL4 Microkit + RPi4.
The [seL4 RPi4 docs](https://docs.sel4.systems/Hardware/Rpi4.html) only cover serial console.

### Serial Adapter Required for Full Debugging

Without a USB-serial adapter, debug output from seL4 is not visible. The
`debug_println!` macro outputs to UART, not HDMI.

## Troubleshooting

### No display output
- Check HDMI connection
- Verify `config.txt` has `hdmi_force_hotplug=1`
- Try a different HDMI cable/port
- Use U-Boot boot (`--uboot` flag) to verify HDMI works

### Build fails: "Board not found"
- Ensure MICROKIT_SDK points to correct location
- Verify SDK contains `board/rpi4b_4gb/` (or your variant)

### Kernel panic / no serial output
- Check serial connections (TX/RX not swapped)
- Verify baud rate is 115200
- Ensure `enable_uart=1` in config.txt

## Building Microkit SDK from Source

The Microkit SDK 2.1.0 release has a boot bug on Raspberry Pi 4B (see [Known Issues](#microkit-sdk-210-boot-failure-on-rpi4b-fixed)).
Until SDK 2.2.0 is released, you must build the SDK from source to get the fix.

### Prerequisites

- Docker (for reproducible builds)
- seL4 kernel source (from sel4test or standalone)
- Git

### Quick Build (Docker)

```bash
# Clone Microkit with the fix
git submodule add https://github.com/seL4/microkit.git vendor/microkit
cd vendor/microkit
git checkout main  # or specific commit with fix: 23f1f6d

# Get seL4 kernel source (if you don't have it)
cd ../..
mkdir -p sel4test && cd sel4test
repo init -u https://github.com/seL4/sel4test-manifest.git
repo sync
cd ..

# Build SDK using Docker (this takes ~5 minutes)
docker pull trustworthysystems/camkes
docker run --rm -u $(id -u):$(id -g) -e CCACHE_DISABLE=1 \
  -v $(pwd):/workspace \
  -w /workspace/rpi4-graphics/vendor/microkit \
  trustworthysystems/camkes python3 build_sdk.py \
    --sel4 /workspace/sel4test/kernel \
    --boards rpi4b_8gb \
    --configs debug \
    --skip-tool \
    --skip-initialiser \
    --skip-docs \
    --skip-tar \
    --gcc-toolchain-prefix-aarch64 aarch64-linux-gnu
```

### Build Options Explained

| Option | Description |
|--------|-------------|
| `--boards rpi4b_8gb` | Build for 8GB Pi 4 (also: `rpi4b_4gb`, `rpi4b_2gb`, `rpi4b_1gb`) |
| `--configs debug` | Debug build with symbols (also: `release`) |
| `--skip-tool` | Don't build the Microkit tool (use existing from SDK 2.1.0) |
| `--skip-initialiser` | Skip Rust initialiser build (use existing) |
| `--gcc-toolchain-prefix-aarch64` | Use `aarch64-linux-gnu-` prefix for GCC |

### Install the Fixed Loader

After building, copy the fixed loader to your SDK:

```bash
# The fixed loader is at:
# vendor/microkit/release/microkit-sdk-2.1.0-dev/board/rpi4b_8gb/debug/elf/loader.elf

# Copy to your SDK (assuming MICROKIT_SDK is set)
cp vendor/microkit/release/microkit-sdk-2.1.0-dev/board/rpi4b_8gb/debug/elf/loader.elf \
   $MICROKIT_SDK/board/rpi4b_8gb/debug/elf/loader.elf
```

### Understanding the Fix

The fix is in `loader/src/aarch64/init.c`:

```c
// Before (SDK 2.1.0):
uint32_t ret = arm_smc32_call(PSCI_FUNCTION_VERSION, ...);
// This crashes on RPi4B because firmware doesn't handle PSCI SMC properly

// After (PR #402):
#if !defined(CONFIG_PLAT_BCM2711)
    uint32_t ret = arm_smc32_call(PSCI_FUNCTION_VERSION, ...);
    // ... PSCI check code ...
#endif
// BCM2711 (RPi4) skips the problematic PSCI call entirely
```

### Verifying the Fix

After building, confirm the fix is present:

```bash
# Check the commit includes the BCM2711 fix
cd vendor/microkit
git log --oneline -1
# Should show: 23f1f6d loader: hot-fix for RPi4B

# Verify loader.elf was built
ls -la release/microkit-sdk-2.1.0-dev/board/rpi4b_8gb/debug/elf/loader.elf
```

### Troubleshooting SDK Build

**Permission denied errors**:
```bash
# Clean up root-owned files from previous Docker runs
docker run --rm -v $(pwd)/vendor/microkit:/workspace trustworthysystems/camkes \
  rm -rf /workspace/build /workspace/release
```

**Missing pyfdt module** (when building outside Docker):
```bash
pip install pyfdt
```

**Clang too old** (`-mno-outline-atomics` error):
Use `--gcc-toolchain-prefix-aarch64 aarch64-linux-gnu` instead of `--llvm`

## References

- [seL4 Raspberry Pi 4 Docs](https://docs.sel4.systems/Hardware/Rpi4.html)
- [Microkit Manual](https://github.com/seL4/microkit/blob/main/docs/manual.md)
- [Microkit PR #402: RPi4B hot-fix](https://github.com/seL4/microkit/pull/402) - Fix for PSCI boot crash
- [Microkit Issue #401: RPi4B boot failure](https://github.com/seL4/microkit/issues/401) - Original bug report
- [Raspberry Pi Mailbox Interface](https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interface)
- [BCM2711 Peripherals](https://datasheets.raspberrypi.com/bcm2711/bcm2711-peripherals.pdf)
- [STPM4RasPI Data Brief](https://www.st.com/resource/en/data_brief/stpm4raspi.pdf)
- [ST33K TPM Application Note](https://www.st.com/resource/en/application_note/an5714-integrating-the-st33tphf2xspi-and-st33tphf2xi2c-trusted-platform-modules-with-linux-stmicroelectronics.pdf)

## License

MIT
