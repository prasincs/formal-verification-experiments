# seL4 Microkit Graphics Demo - Raspberry Pi 4

A bootable SD card image demonstrating seL4 Microkit on Raspberry Pi 4 with
framebuffer graphics output.

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
- Raspberry Pi 4 Model B (1GB, 2GB, 4GB, or 8GB)
- MicroSD card (8GB+)
- HDMI display
- Optional: USB-to-serial adapter for debug output

### Software
- Linux or macOS
- Rust nightly (`rustup install nightly`)
- Cross compiler:
  - Linux: `sudo apt install gcc-aarch64-linux-gnu`
  - macOS: `brew install aarch64-elf-gcc`
- Microkit SDK with RPi4 support

## Quick Start

### 1. Set up Microkit SDK

```bash
# Download SDK (update version as needed)
curl -LO https://github.com/seL4/microkit/releases/download/1.4.1/microkit-sdk-1.4.1-linux-x86-64.tar.gz
mkdir -p microkit-sdk
tar -xzf microkit-sdk-*.tar.gz -C microkit-sdk --strip-components=1
export MICROKIT_SDK=$PWD/microkit-sdk
```

### 2. Build the System

```bash
cd rpi4-graphics

# Build for 4GB Pi 4 (adjust as needed)
make RPI4_MEMORY=4gb

# Download firmware and create boot files
make firmware
make bootfiles
```

### 3. Flash to SD Card

**Option A: Using Raspberry Pi Imager**
1. Open Raspberry Pi Imager
2. Choose OS → Use custom → Select `build/rpi4-sel4-graphics.img`
3. Choose storage → Select your SD card
4. Write

**Option B: Manual copy**
1. Format SD card as FAT32
2. Copy all files from `build/boot/` to SD card root
3. Eject safely

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
│   └── font.rs         # 8x8 bitmap font
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
make RPI4_MEMORY=8gb   # For 8GB model
make RPI4_MEMORY=2gb   # For 2GB model
```

## Verification Status

| Component | Verification |
|-----------|--------------|
| seL4 kernel | ✅ Isabelle/HOL (binary proof for ARM) |
| Microkit framework | ✅ Designed for verified systems |
| Framebuffer driver | ⚠️ Trusted (hardware interface) |
| Graphics primitives | 🔄 Verus specs planned |
| VideoCore firmware | ❌ Closed source |

## Troubleshooting

### No display output
- Check HDMI connection
- Verify `config.txt` has `hdmi_force_hotplug=1`
- Try a different HDMI cable/port

### Build fails: "Board not found"
- Ensure MICROKIT_SDK points to correct location
- Verify SDK contains `board/rpi4b_4gb/` (or your variant)

### Kernel panic / no serial output
- Check serial connections (TX/RX not swapped)
- Verify baud rate is 115200
- Ensure `enable_uart=1` in config.txt

## References

- [seL4 Raspberry Pi 4 Docs](https://docs.sel4.systems/Hardware/Rpi4.html)
- [Microkit Manual](https://github.com/seL4/microkit/blob/main/docs/manual.md)
- [Raspberry Pi Mailbox Interface](https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interface)
- [BCM2711 Peripherals](https://datasheets.raspberrypi.com/bcm2711/bcm2711-peripherals.pdf)

## License

MIT
