# seL4 Raspberry Pi 4/5 Graphics Architecture

## Overview

This document describes the architecture for running seL4 with verified graphics output on both Raspberry Pi 4 (BCM2711) and Raspberry Pi 5 (BCM2712).

## Hardware Comparison

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     RASPBERRY PI 4 vs 5 ARCHITECTURE                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────┐     ┌─────────────────────────────────┐   │
│  │     RASPBERRY PI 4          │     │       RASPBERRY PI 5            │   │
│  │        BCM2711              │     │          BCM2712                │   │
│  ├─────────────────────────────┤     ├─────────────────────────────────┤   │
│  │  ┌───────────────────────┐  │     │  ┌───────────────────────────┐  │   │
│  │  │   4× Cortex-A72       │  │     │  │   4× Cortex-A76           │  │   │
│  │  │      1.8 GHz          │  │     │  │      2.4 GHz              │  │   │
│  │  │   512KB L2 per core   │  │     │  │   512KB L2 + 2MB L3       │  │   │
│  │  └───────────────────────┘  │     │  └───────────────────────────┘  │   │
│  │           │                 │     │           │                     │   │
│  │  ┌───────────────────────┐  │     │  ┌───────────────────────────┐  │   │
│  │  │   VideoCore VI        │  │     │  │   VideoCore VII           │  │   │
│  │  │   (GPU + Display)     │  │     │  │   (GPU + Display)         │  │   │
│  │  │                       │  │     │  │                           │  │   │
│  │  │  ┌─────────────────┐  │  │     │  │  ┌─────────────────────┐  │  │   │
│  │  │  │ Mailbox I/F     │  │  │     │  │  │ Mailbox I/F         │  │  │   │
│  │  │  │ (Channel 8)     │  │  │     │  │  │ (Channel 8)         │  │  │   │
│  │  │  └─────────────────┘  │  │     │  │  └─────────────────────┘  │  │   │
│  │  │  ┌─────────────────┐  │  │     │  │  ┌─────────────────────┐  │  │   │
│  │  │  │ 2× HDMI 2.0     │  │  │     │  │  │ 2× HDMI 2.1         │  │  │   │
│  │  │  │ (4K@60Hz)       │  │  │     │  │  │ (4K@60Hz)           │  │  │   │
│  │  │  └─────────────────┘  │  │     │  │  └─────────────────────┘  │  │   │
│  │  └───────────────────────┘  │     │  └───────────────────────────┘  │   │
│  │           │                 │     │           │                     │   │
│  │  ┌───────────────────────┐  │     │  ┌───────────────────────────┐  │   │
│  │  │  Peripherals (SoC)    │  │     │  │   PCIe ×4 Bus            │  │   │
│  │  │  - GPIO               │  │     │  │         │                │  │   │
│  │  │  - I2C/SPI/UART       │  │     │  │  ┌──────┴──────┐         │  │   │
│  │  │  - USB 3.0            │  │     │  │  │   RP1 SoC   │         │  │   │
│  │  │  - Ethernet           │  │     │  │  │ (Southbridge)│         │  │   │
│  │  └───────────────────────┘  │     │  │  │ - GPIO      │         │  │   │
│  │                             │     │  │  │ - I2C/SPI   │         │  │   │
│  │  Peripheral Base:           │     │  │  │ - USB 3.0   │         │  │   │
│  │    0xFE00_0000              │     │  │  │ - Ethernet  │         │  │   │
│  │                             │     │  │  └─────────────┘         │  │   │
│  │                             │     │  │                           │  │   │
│  │                             │     │  │  BCM2712 Periph Base:     │  │   │
│  │                             │     │  │    0x1_0000_0000          │  │   │
│  │                             │     │  │  RP1 Base (via PCIe):     │  │   │
│  │                             │     │  │    0x1F_0000_0000         │  │   │
│  └─────────────────────────────┘     └─────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      seL4 + VERIFIED GRAPHICS STACK                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                     USER SPACE (Protection Domains)                   │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │   │
│  │  │ Diagram     │  │ Font        │  │ Input       │  │ Application │  │   │
│  │  │ Renderer    │  │ Renderer    │  │ Handler     │  │ Logic       │  │   │
│  │  │ (Verus ✓)   │  │ (Verus ✓)   │  │             │  │ (Verus ✓)   │  │   │
│  │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  │   │
│  │         │                │                │                │         │   │
│  │         └────────────────┼────────────────┼────────────────┘         │   │
│  │                          │                │                           │   │
│  │  ┌───────────────────────┴────────────────┴───────────────────────┐  │   │
│  │  │                   GRAPHICS DRIVER PD                            │  │   │
│  │  │  ┌─────────────────────────────────────────────────────────┐   │  │   │
│  │  │  │              Verified Framebuffer (Verus ✓)              │   │  │   │
│  │  │  │  - Bounds-checked pixel writes                          │   │  │   │
│  │  │  │  - Proven no buffer overflow                            │   │  │   │
│  │  │  │  - Verified coordinate transforms                       │   │  │   │
│  │  │  └─────────────────────────────────────────────────────────┘   │  │   │
│  │  │  ┌─────────────────────────────────────────────────────────┐   │  │   │
│  │  │  │           Hardware Abstraction Layer (HAL)               │   │  │   │
│  │  │  │  ┌──────────────────┐    ┌──────────────────┐           │   │  │   │
│  │  │  │  │   Pi4 Backend    │    │   Pi5 Backend    │           │   │  │   │
│  │  │  │  │  (BCM2711)       │    │  (BCM2712)       │           │   │  │   │
│  │  │  │  │                  │    │                  │           │   │  │   │
│  │  │  │  │ Periph: 0xFE..   │    │ Periph: 0x1_00.. │           │   │  │   │
│  │  │  │  │ Mailbox: direct  │    │ Mailbox: direct  │           │   │  │   │
│  │  │  │  └──────────────────┘    └──────────────────┘           │   │  │   │
│  │  │  └─────────────────────────────────────────────────────────┘   │  │   │
│  │  └────────────────────────────────────────────────────────────────┘  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    │                                        │
│  ┌─────────────────────────────────┴─────────────────────────────────────┐ │
│  │                         seL4 MICROKERNEL                               │ │
│  │                    (Isabelle/HOL Verified ✓)                          │ │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │ │
│  │  │ Capability  │  │ Memory      │  │ IPC         │  │ Scheduling  │  │ │
│  │  │ Management  │  │ Management  │  │ Primitives  │  │             │  │ │
│  │  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘  │ │
│  └───────────────────────────────────────────────────────────────────────┘ │
│                                    │                                        │
│  ┌─────────────────────────────────┴─────────────────────────────────────┐ │
│  │                           HARDWARE                                     │ │
│  │  ┌───────────────────────────┐  ┌───────────────────────────────────┐ │ │
│  │  │    Raspberry Pi 4         │  │      Raspberry Pi 5               │ │ │
│  │  │    BCM2711                │  │      BCM2712 + RP1                │ │ │
│  │  └───────────────────────────┘  └───────────────────────────────────┘ │ │
│  └───────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Verification Boundaries

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         VERIFICATION BOUNDARIES                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐ │
│  │                    FORMALLY VERIFIED (Verus)                          │ │
│  │                                                                        │ │
│  │  ✓ Framebuffer pixel operations (bounds checking)                    │ │
│  │  ✓ Coordinate transforms (no overflow, within bounds)                │ │
│  │  ✓ Color space conversions (no data loss)                            │ │
│  │  ✓ Font glyph lookups (proven in-bounds)                             │ │
│  │  ✓ Line/rectangle drawing algorithms (termination, bounds)           │ │
│  │  ✓ Diagram layout calculations (no overflow)                         │ │
│  │  ✓ Text rendering bounds (string length validation)                  │ │
│  │  ✓ Memory region management (no overlaps)                            │ │
│  │  ✓ Capability-based access control                                   │ │
│  └───────────────────────────────────────────────────────────────────────┘ │
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐ │
│  │                    FORMALLY VERIFIED (seL4/Isabelle)                  │ │
│  │                                                                        │ │
│  │  ✓ Memory isolation between Protection Domains                       │ │
│  │  ✓ Capability system (unforgeable, proper derivation)                │ │
│  │  ✓ IPC correctness                                                   │ │
│  │  ✓ Scheduling fairness                                               │ │
│  │  ✓ No kernel crashes/panics                                          │ │
│  │  ✓ Information flow (confidentiality)                                │ │
│  │  ✓ ARM binary verification (Pi 4 only - BCM2711)                     │ │
│  └───────────────────────────────────────────────────────────────────────┘ │
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐ │
│  │              TRUSTED BUT NOT FORMALLY VERIFIED                        │ │
│  │                    (Trusted Computing Base)                           │ │
│  │                                                                        │ │
│  │  ⚠ Mailbox driver (hardware interface)                               │ │
│  │  ⚠ Framebuffer physical memory mapping                               │ │
│  │  ⚠ VideoCore firmware interactions                                   │ │
│  │  ⚠ HDMI signal generation (done by GPU firmware)                     │ │
│  │  ⚠ Boot process (config.txt, start4.elf)                             │ │
│  │  ⚠ Hardware register access                                          │ │
│  │  ⚠ Pi 5: RP1 southbridge communication                              │ │
│  │  ⚠ Pi 5: PCIe enumeration and BAR mapping                           │ │
│  └───────────────────────────────────────────────────────────────────────┘ │
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐ │
│  │                    EXTERNAL TRUSTED (Firmware)                        │ │
│  │                                                                        │ │
│  │  ✗ Raspberry Pi bootloader (closed source)                           │ │
│  │  ✗ VideoCore firmware (start4.elf / start4cd.elf)                    │ │
│  │  ✗ HDMI PHY initialization                                           │ │
│  │  ✗ DDR memory training                                               │ │
│  └───────────────────────────────────────────────────────────────────────┘ │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Memory Map Abstraction

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PLATFORM MEMORY MAPS                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  RASPBERRY PI 4 (BCM2711)              RASPBERRY PI 5 (BCM2712)             │
│  ─────────────────────────             ─────────────────────────             │
│                                                                             │
│  0x0000_0000 ┌─────────────┐           0x0_0000_0000 ┌─────────────┐        │
│              │             │                         │             │        │
│              │    RAM      │                         │    RAM      │        │
│              │  (1-8 GB)   │                         │  (4-8 GB)   │        │
│              │             │                         │             │        │
│  0x3C00_0000 ├─────────────┤                         │             │        │
│              │ Framebuffer │                         │             │        │
│              │  (GPU alloc)│           0x_8000_0000  ├─────────────┤        │
│  0x4000_0000 ├─────────────┤                         │ Framebuffer │        │
│              │ Local Periph│                         │  (GPU alloc)│        │
│              │ (ARM)       │           0x1_0000_0000 ├─────────────┤        │
│  0xFC00_0000 ├─────────────┤                         │ BCM2712     │        │
│              │ PCIe Window │                         │ Peripherals │        │
│  0xFE00_0000 ├─────────────┤                         │             │        │
│              │ BCM2711     │           0x1_0040_0000 ├─────────────┤        │
│              │ Peripherals │                         │ (reserved)  │        │
│              │             │                         │             │        │
│  0xFE00_B880 │ ┌─────────┐ │           0x1F_0000_0000├─────────────┤        │
│              │ │ Mailbox │ │                         │ RP1 Periph  │        │
│              │ └─────────┘ │                         │ (via PCIe)  │        │
│  0xFF80_0000 ├─────────────┤           0x1F_000B_0880│ ┌─────────┐ │        │
│              │ VideoCore   │                         │ │ Mailbox │ │        │
│              │             │                         │ └─────────┘ │        │
│  0xFFFF_FFFF └─────────────┘           0x1F_FFFF_FFFF└─────────────┘        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Framebuffer Initialization Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    FRAMEBUFFER INITIALIZATION                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  BOOT SEQUENCE:                                                             │
│                                                                             │
│  ┌──────────────┐                                                           │
│  │ GPU Firmware │  1. GPU powers on first                                   │
│  │ (start4.elf) │  2. Reads config.txt                                      │
│  └──────┬───────┘  3. Initializes HDMI                                      │
│         │          4. Sets up simple framebuffer                            │
│         ▼                                                                   │
│  ┌──────────────┐                                                           │
│  │ ARM Cores    │  5. GPU releases ARM from reset                           │
│  │   Boot       │  6. ARM starts at 0x80000 (kernel8.img)                   │
│  └──────┬───────┘                                                           │
│         │                                                                   │
│         ▼                                                                   │
│  ┌──────────────┐                                                           │
│  │    seL4      │  7. seL4 kernel initializes                               │
│  │   Kernel     │  8. Creates root task                                     │
│  └──────┬───────┘                                                           │
│         │                                                                   │
│         ▼                                                                   │
│  ┌──────────────┐  ┌────────────────────────────────────────────────────┐  │
│  │  Graphics    │  │  MAILBOX FRAMEBUFFER SETUP (Property Tags)        │  │
│  │  Driver PD   │  │                                                    │  │
│  │              │  │  1. Allocate buffer:                               │  │
│  │   Step 1:    │  │     Tag 0x40001 - Get physical size → 1920×1080   │  │
│  │   Query      │──│     Tag 0x40005 - Get depth → 32bpp               │  │
│  │   Display    │  │     Tag 0x40008 - Get pitch → bytes per row       │  │
│  │              │  │                                                    │  │
│  │   Step 2:    │  │  2. Allocate framebuffer:                         │  │
│  │   Allocate   │──│     Tag 0x40001 - Allocate buffer                 │  │
│  │   Buffer     │  │     Returns: physical address + size              │  │
│  │              │  │                                                    │  │
│  │   Step 3:    │  │  3. seL4 maps physical → virtual:                 │  │
│  │   Map to     │──│     Create device untyped                         │  │
│  │   Virtual    │  │     Map into PD's virtual address space           │  │
│  └──────────────┘  └────────────────────────────────────────────────────┘  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Hardware Abstraction Layer Design

### Crate Dependencies

| Platform | Crate | Status | Notes |
|----------|-------|--------|-------|
| RPi 4 (BCM2711) | [`bcm2711-lpa`](https://crates.io/crates/bcm2711-lpa) | ✅ Available | svd2rust PAC, `no_std` |
| RPi 5 (BCM2712) | `bcm2712-lpa` | ❌ **Not available** | No public datasheet |

### bcm2711-lpa Integration

The `bcm2711-lpa` crate provides register-level access but has limitations:

1. **Physical address expectation**: Expects registers at physical addresses
   - Solution: Map device memory with identity mapping in seL4
   - Or use `--base-address-shift` when regenerating the PAC

2. **Mailbox not included**: The VideoCore mailbox may not be in the SVD
   - Solution: Implement mailbox driver separately (well-documented interface)

3. **Verification boundary**: `bcm2711-lpa` is **NOT verified**
   - It's generated code from SVD, trusted but not formally proven
   - We wrap it in verified Verus code that checks preconditions

```toml
# Cargo.toml
[dependencies]
bcm2711-lpa = "0.5"  # Pi 4 only - no Pi 5 equivalent exists

[features]
rpi4 = ["bcm2711-lpa"]
rpi5 = []  # Manual register definitions until bcm2712-lpa exists
```

### HAL Trait Design

```rust
/// Platform-agnostic framebuffer interface
pub trait FramebufferHAL {
    /// Initialize the framebuffer, returns (base_addr, width, height, pitch)
    fn init(&mut self) -> Result<FramebufferInfo, FramebufferError>;

    /// Get the mailbox base address for this platform
    fn mailbox_base(&self) -> usize;

    /// Platform-specific address translation (GPU bus → ARM physical)
    fn gpu_to_arm_addr(&self, gpu_addr: u32) -> usize;
}

/// BCM2711 (Raspberry Pi 4) implementation
/// Uses bcm2711-lpa for peripheral access
#[cfg(feature = "rpi4")]
pub struct BCM2711HAL {
    // Peripheral base: 0xFE000000
    // Mailbox base: 0xFE00B880
    periph: bcm2711_lpa::Peripherals,
}

/// BCM2712 (Raspberry Pi 5) implementation
/// Manual register definitions (no PAC available)
#[cfg(feature = "rpi5")]
pub struct BCM2712HAL {
    // Peripheral base: 0x1_00000000
    // Mailbox through VideoCore VII (similar interface)
    // Derived from Linux kernel sources
}

/// Framebuffer information (verified struct)
pub struct FramebufferInfo {
    pub base: usize,      // Physical base address
    pub width: u32,       // Width in pixels
    pub height: u32,      // Height in pixels
    pub pitch: u32,       // Bytes per row
    pub depth: u32,       // Bits per pixel (typically 32)
}
```

## Verified Graphics Components

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    VERIFIED GRAPHICS COMPONENTS                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    VerifiedFramebuffer                               │   │
│  │                                                                       │   │
│  │  verus! {                                                            │   │
│  │      pub struct VerifiedFramebuffer {                                │   │
│  │          buffer: *mut u32,     // Raw pointer to framebuffer        │   │
│  │          width: u32,                                                 │   │
│  │          height: u32,                                                │   │
│  │          pitch: u32,           // Bytes per scanline                │   │
│  │      }                                                               │   │
│  │                                                                       │   │
│  │      impl VerifiedFramebuffer {                                      │   │
│  │          // PROVEN: x < width && y < height                         │   │
│  │          pub fn put_pixel(&mut self, x: u32, y: u32, color: u32)    │   │
│  │              requires                                                │   │
│  │                  x < self.width,                                     │   │
│  │                  y < self.height,                                    │   │
│  │              ensures                                                 │   │
│  │                  // Pixel at (x,y) is now `color`                   │   │
│  │          { ... }                                                     │   │
│  │                                                                       │   │
│  │          // PROVEN: line endpoints within bounds                    │   │
│  │          pub fn draw_line(&mut self, x0: u32, y0: u32,              │   │
│  │                           x1: u32, y1: u32, color: u32)             │   │
│  │              requires                                                │   │
│  │                  x0 < self.width && x1 < self.width,                │   │
│  │                  y0 < self.height && y1 < self.height,              │   │
│  │          { ... }                                                     │   │
│  │                                                                       │   │
│  │          // PROVEN: rectangle within bounds                         │   │
│  │          pub fn fill_rect(&mut self, x: u32, y: u32,                │   │
│  │                           w: u32, h: u32, color: u32)               │   │
│  │              requires                                                │   │
│  │                  x + w <= self.width,                               │   │
│  │                  y + h <= self.height,                              │   │
│  │          { ... }                                                     │   │
│  │      }                                                               │   │
│  │  }                                                                   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    DiagramRenderer                                    │   │
│  │                                                                       │   │
│  │  Verified operations for drawing architecture diagrams:              │   │
│  │                                                                       │   │
│  │  ✓ draw_box(x, y, w, h, label) - Bounds-checked box with label      │   │
│  │  ✓ draw_arrow(from, to)        - Arrow between two points           │   │
│  │  ✓ draw_text(x, y, text)       - Text rendering with font lookup    │   │
│  │  ✓ layout_tree(nodes)          - Tree layout algorithm              │   │
│  │  ✓ layout_stack(layers)        - Vertical stack layout              │   │
│  │                                                                       │   │
│  │  All coordinate calculations proven to:                              │   │
│  │  - Not overflow                                                      │   │
│  │  - Stay within framebuffer bounds                                    │   │
│  │  - Terminate (for iterative algorithms)                              │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## File Structure

```
sel4-microkernel/rpi-graphics/
├── ARCHITECTURE.md           # This document
├── src/
│   ├── lib.rs               # Crate root
│   ├── hal/
│   │   ├── mod.rs           # HAL trait definitions
│   │   ├── bcm2711.rs       # Pi 4 implementation
│   │   └── bcm2712.rs       # Pi 5 implementation
│   ├── mailbox.rs           # Mailbox interface (shared)
│   ├── framebuffer.rs       # Verified framebuffer operations
│   ├── graphics/
│   │   ├── mod.rs
│   │   ├── primitives.rs    # Verified drawing primitives
│   │   ├── font.rs          # Bitmap font with verified lookup
│   │   └── diagram.rs       # Architecture diagram renderer
│   └── verified/
│       ├── mod.rs
│       ├── bounds.rs        # Bounds checking proofs
│       └── coordinates.rs   # Coordinate transform proofs
├── hello.system             # Microkit system description
├── Makefile
└── config/
    ├── pi4-config.txt       # Pi 4 boot config
    └── pi5-config.txt       # Pi 5 boot config
```

## Boot Configuration

### config.txt (common settings)
```ini
# Force simple framebuffer mode (both Pi 4 and 5)
dtoverlay=vc4-kms-v3d
disable_overscan=1
hdmi_force_hotplug=1
hdmi_group=2
hdmi_mode=82         # 1920x1080@60Hz

# seL4 kernel
kernel=kernel8.img
arm_64bit=1

# Pi 5 specific (ignored on Pi 4)
# pciex4_reset=0     # Keep RP1 initialized for bare metal
```

## seL4 Platform Support Status

| Platform | seL4 Kernel | Microkit | Binary Proof | Status |
|----------|-------------|----------|--------------|--------|
| RPi 4 (BCM2711) | ✅ Supported | ✅ Yes | ✅ ARM verified | Ready |
| RPi 5 (BCM2712) | ⚠️ WIP | ❌ Not yet | ❌ Not yet | Needs porting |

### Raspberry Pi 5 Porting Notes

The Pi 5 requires additional work:

1. **BCM2712 Device Tree**: New interrupt controller, timer configuration
2. **Memory Map**: Different peripheral base addresses
3. **RP1 Initialization**: PCIe enumeration for GPIO/I2C (not needed for display)
4. **VideoCore VII**: Similar mailbox interface, different GPU

**Strategy**: Use firmware-initialized framebuffer (simple mode) to avoid
needing full VC7 driver. The mailbox property interface is largely compatible.

## Boot-Time Firmware Verification

### Secure Boot Chain (Pi 4 rev 1.4+, Pi 5)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         SECURE BOOT CHAIN                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌────────────────┐                                                         │
│  │  Boot ROM      │  ← Hardcoded in silicon (immutable root of trust)      │
│  │  (VideoCore)   │     Verifies bootcode.bin signature                    │
│  └───────┬────────┘                                                         │
│          │                                                                  │
│          ▼                                                                  │
│  ┌────────────────┐                                                         │
│  │  bootcode.bin  │  ← Signed by: Raspberry Pi                             │
│  │  (2nd stage)   │     + Customer key on Pi 5                             │
│  └───────┬────────┘                                                         │
│          │                                                                  │
│          ▼                                                                  │
│  ┌────────────────┐                                                         │
│  │  start4.elf    │  ← GPU firmware (closed source)                        │
│  │  (VC firmware) │     Signed by Raspberry Pi                             │
│  │                │     Initializes HDMI, framebuffer, clocks              │
│  └───────┬────────┘                                                         │
│          │                                                                  │
│          ▼                                                                  │
│  ┌────────────────┐                                                         │
│  │  kernel8.img   │  ← seL4 kernel + root task                             │
│  │  (seL4)        │     Signed by customer private key                     │
│  └────────────────┘                                                         │
│                                                                             │
│  OTP (One-Time Programmable) Memory:                                        │
│  ┌─────────────────────────────────────────────┐                            │
│  │  SHA256(customer_public_key) - IRREVERSIBLE │                            │
│  │  secure_boot_enabled flag                   │                            │
│  │  jtag_lock flag (optional)                  │                            │
│  └─────────────────────────────────────────────┘                            │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Mailbox Verification Tags

Query these at boot to verify hardware/firmware state:

```rust
/// Firmware verification mailbox tags
pub mod verify_tags {
    /// Get VC firmware revision (git hash as u32)
    pub const GET_FIRMWARE_REV: u32 = 0x00000001;

    /// Get board model (Pi 4 = 0x11, Pi 5 = 0x17)
    pub const GET_BOARD_MODEL: u32 = 0x00010001;

    /// Get board revision (encodes memory, manufacturer, etc.)
    pub const GET_BOARD_REV: u32 = 0x00010002;

    /// Get unique board serial number (u64)
    pub const GET_BOARD_SERIAL: u32 = 0x00010004;

    /// Get ARM memory base and size
    pub const GET_ARM_MEMORY: u32 = 0x00010005;

    /// Get VideoCore memory base and size
    pub const GET_VC_MEMORY: u32 = 0x00010006;
}

verus! {
    /// Verified boot check - ensures we're on expected hardware
    pub fn verify_boot_environment(
        expected_model: u32,
        min_firmware_rev: u32,
    ) -> (result: Result<BootInfo, BootError>)
        ensures
            result.is_ok() ==> result.unwrap().model == expected_model,
            result.is_ok() ==> result.unwrap().firmware_rev >= min_firmware_rev,
    {
        // Query mailbox and verify responses
        // ...
    }
}
```

### What Can Be Verified

| Check | Method | Verification Level |
|-------|--------|-------------------|
| Firmware version | Mailbox tag 0x1 | ✅ Can compare against known-good |
| Board identity | Tags 0x10001-0x10004 | ✅ Cryptographic binding via serial |
| Memory layout | Tags 0x10005-0x10006 | ✅ Sanity check |
| Kernel signature | Secure Boot | ✅ If OTP programmed |
| Bootloader signature | Secure Boot | ✅ Raspberry Pi signed |
| GPU firmware behavior | N/A | ❌ Closed source, no proofs |
| Runtime integrity | N/A | ❌ No measured boot/TPM |

### Verified Boot Checker (Verus)

```rust
verus! {
    /// Known-good firmware revisions (git commit hashes truncated to u32)
    pub const KNOWN_GOOD_FIRMWARE: &[u32] = &[
        0x8ba1771f,  // 2024-01-15 release
        0xa2c3d4e5,  // 2024-03-20 release
        // Add more as verified
    ];

    /// Verify firmware is in allowlist
    pub fn is_firmware_known_good(rev: u32) -> (good: bool)
        ensures
            good <==> exists|i: int| 0 <= i < KNOWN_GOOD_FIRMWARE.len()
                       && KNOWN_GOOD_FIRMWARE[i] == rev,
    {
        let mut i = 0;
        while i < KNOWN_GOOD_FIRMWARE.len()
            invariant
                i <= KNOWN_GOOD_FIRMWARE.len(),
                forall|j: int| 0 <= j < i ==> KNOWN_GOOD_FIRMWARE[j] != rev,
        {
            if KNOWN_GOOD_FIRMWARE[i] == rev {
                return true;
            }
            i = i + 1;
        }
        false
    }
}
```

### Enabling Secure Boot (One-Time, Irreversible)

```bash
# Generate signing keys
openssl genrsa -out private.pem 2048
openssl rsa -in private.pem -outform PEM -pubout -out public.pem

# Sign seL4 kernel
rpi-eeprom-digest --sign kernel8.img private.pem

# Program OTP (⚠️ PERMANENT - CANNOT BE UNDONE)
# Only do this after verifying signed boot works!
vcmailbox 0x00038044  # Enable secure boot in OTP
```

### Limitations (Without TPM)

| Limitation | Impact | Mitigation |
|------------|--------|------------|
| No TPM | Cannot do remote attestation | Use device serial for binding |
| Closed GPU firmware | Cannot verify start4.elf behavior | Accept as trusted, minimize interface |
| No measured boot | Cannot detect runtime tampering | Rely on seL4 isolation |
| OTP is permanent | Cannot recover if keys lost | Test thoroughly before programming |

## TPM 2.0 Integration (ST33KTPM2I3WBZA9)

Adding an ST33K TPM 2.0 chip enables **measured boot** and **remote attestation**.

### Hardware Connection

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         TPM 2.0 HARDWARE SETUP                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Raspberry Pi 4 GPIO Header              ST33KTPM2I3WBZA9                  │
│   ┌─────────────────────────┐            ┌─────────────────┐               │
│   │  Pin 19 (GPIO 10) MOSI  │───────────→│ MOSI            │               │
│   │  Pin 21 (GPIO 9)  MISO  │←───────────│ MISO            │               │
│   │  Pin 23 (GPIO 11) SCLK  │───────────→│ SCLK            │               │
│   │  Pin 24 (GPIO 8)  CE0   │───────────→│ CS              │               │
│   │  Pin 18 (GPIO 24) RST   │───────────→│ RST (optional)  │               │
│   │  Pin 1          3.3V    │───────────→│ VCC (3.3V)      │               │
│   │  Pin 6          GND     │───────────→│ GND             │               │
│   └─────────────────────────┘            └─────────────────┘               │
│                                                                             │
│   SPI Configuration:                                                        │
│   - Mode: 0 (CPOL=0, CPHA=0)                                               │
│   - Speed: 10 MHz max                                                       │
│   - Chip Select: Active Low                                                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Measured Boot Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         MEASURED BOOT WITH TPM                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   BOOT STAGE              MEASUREMENT                    PCR                │
│   ──────────              ───────────                    ───                │
│                                                                             │
│   ┌────────────────┐                                                        │
│   │  Boot ROM      │─────→ (not measured - root of trust)                  │
│   └───────┬────────┘                                                        │
│           ▼                                                                 │
│   ┌────────────────┐      SHA256(bootcode.bin)                             │
│   │  bootcode.bin  │─────→ TPM2_PCR_Extend ─────→ PCR[0] (Firmware)        │
│   └───────┬────────┘                                                        │
│           ▼                                                                 │
│   ┌────────────────┐      SHA256(start4.elf)                               │
│   │  start4.elf    │─────→ TPM2_PCR_Extend ─────→ PCR[0] (Firmware)        │
│   └───────┬────────┘                                                        │
│           ▼                                                                 │
│   ┌────────────────┐      SHA256(kernel8.img)                              │
│   │  seL4 Kernel   │─────→ TPM2_PCR_Extend ─────→ PCR[1] (Kernel)          │
│   └───────┬────────┘                                                        │
│           ▼                                                                 │
│   ┌────────────────┐      SHA256(system config)                            │
│   │  Microkit      │─────→ TPM2_PCR_Extend ─────→ PCR[2] (System)          │
│   └───────┬────────┘                                                        │
│           ▼                                                                 │
│   ┌────────────────┐      SHA256(PD images)                                │
│   │  Protection    │─────→ TPM2_PCR_Extend ─────→ PCR[3] (PDs)             │
│   │  Domains       │                                                        │
│   └────────────────┘                                                        │
│                                                                             │
│   Final PCR State = Hash chain of all boot components                      │
│   Any modification → Different PCR values → Attestation fails              │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Remote Attestation Protocol

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       REMOTE ATTESTATION FLOW                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   DEVICE (seL4 + TPM)                    VERIFIER (Attestation Server)     │
│   ───────────────────                    ─────────────────────────────     │
│                                                                             │
│   1. Boot with measured chain                                               │
│      PCR[0..7] contain measurements                                         │
│                                                                             │
│                              ←────────── 2. Challenge (nonce)               │
│                                             Random value to prevent replay │
│                                                                             │
│   3. Generate Quote                                                         │
│      TPM2_Quote(                                                            │
│        AK,              // Attestation Key (TPM-bound)                     │
│        PCR_Selection,   // Which PCRs to include                           │
│        nonce            // Verifier's challenge                            │
│      )                                                                      │
│                                                                             │
│   4. Quote Response ─────────────────→                                      │
│      {                                                                      │
│        pcr_values: [...],                                                  │
│        signature: Sign_AK(pcr_values || nonce),                            │
│        ak_cert: TPM_Certificate                                            │
│      }                                                                      │
│                                                                             │
│                                          5. Verify Quote                    │
│                                             a. Verify AK cert chain        │
│                                             b. Verify signature            │
│                                             c. Check nonce matches         │
│                                             d. Compare PCRs to policy      │
│                                                                             │
│                              ←────────── 6. Attestation Result              │
│                                             PASS: Device is trusted        │
│                                             FAIL: Device compromised       │
│                                                                             │
│                                          7. (Optional) Provision secrets   │
│                                             Only if attestation passed     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### TPM Driver Integration

```rust
/// PCR indices for measured boot
pub mod pcr {
    pub const FIRMWARE: usize = 0;    // bootcode.bin, start4.elf
    pub const KERNEL: usize = 1;      // seL4 kernel
    pub const SYSTEM: usize = 2;      // Microkit config
    pub const PD: usize = 3;          // Protection Domain images
    pub const RUNTIME: usize = 4;     // Runtime measurements
}

/// Measured boot sequence
pub fn measured_boot(tpm: &mut Tpm) -> Result<(), TpmError> {
    // Each component extends its measurement into the appropriate PCR
    tpm.pcr_extend(pcr::KERNEL, &sha256(kernel_image))?;
    tpm.pcr_extend(pcr::SYSTEM, &sha256(system_config))?;
    tpm.pcr_extend(pcr::PD, &sha256(graphics_pd))?;
    Ok(())
}

/// Generate attestation quote for remote verifier
pub fn generate_quote(
    tpm: &mut Tpm,
    nonce: &[u8; 32],
    pcr_selection: &[usize],
) -> Result<Quote, TpmError> {
    // TPM signs PCR values with Attestation Key
    tpm.quote(nonce, pcr_selection)
}
```

### Verification Boundaries with TPM

| Component | Verification | TPM Measurement |
|-----------|--------------|-----------------|
| seL4 kernel | ✅ Isabelle/HOL | ✅ PCR[1] |
| Microkit | ✅ Design verified | ✅ PCR[2] |
| Graphics PD | 🔄 Verus (planned) | ✅ PCR[3] |
| TPM driver | ⚠️ Trusted | N/A (measures others) |
| VideoCore firmware | ❌ Closed | ✅ PCR[0] |
| Boot ROM | ❌ Closed | Root of trust |

### What TPM 2.0 Enables

| Capability | Without TPM | With ST33K TPM |
|------------|-------------|----------------|
| Measured boot | ❌ | ✅ All boot stages hashed |
| Remote attestation | ❌ | ✅ Cryptographic proof of state |
| Sealed secrets | ❌ | ✅ Keys bound to PCR state |
| Anti-rollback | ❌ | ✅ Monotonic counters |
| Random numbers | Pseudo-random | ✅ Hardware RNG |
| Device identity | Board serial only | ✅ Unique EK certificate |

### Device Tree Overlay for TPM

```dts
// tpm-st33.dtso - Device tree overlay for ST33K TPM
/dts-v1/;
/plugin/;

/ {
    compatible = "brcm,bcm2711";

    fragment@0 {
        target = <&spi0>;
        __overlay__ {
            status = "okay";
            #address-cells = <1>;
            #size-cells = <0>;

            tpm: tpm@0 {
                compatible = "st,st33htpm-spi", "tcg,tpm_tis-spi";
                reg = <0>;
                spi-max-frequency = <10000000>;
                status = "okay";
            };
        };
    };
};
```

## References

- [seL4 Raspberry Pi 4 Docs](https://docs.sel4.systems/Hardware/Rpi4.html)
- [Raspberry Pi Mailbox Interface](https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interface)
- [Raspberry Pi Secure Boot](https://github.com/raspberrypi/usbboot/blob/master/secure-boot-recovery/README.md)
- [ST33K TPM Application Note](https://www.st.com/resource/en/application_note/an5714-integrating-the-st33tphf2xspi-and-st33tphf2xi2c-trusted-platform-modules-with-linux-stmicroelectronics.pdf)
- [STPM4RasPI Extension Board](https://www.st.com/resource/en/data_brief/stpm4raspi.pdf)
- [BCM2711 Peripherals](https://datasheets.raspberrypi.com/bcm2711/bcm2711-peripherals.pdf)
- [RPi4 Bare Metal Framebuffer](https://www.rpi4os.com/part5-framebuffer/)
- [Verus Lang](https://github.com/verus-lang/verus)

## License

MIT
