# Verified SPI LCD + Touchscreen for Raspberry Pi 4

A formally verified display and touch input system for seL4 Microkit on Raspberry Pi 4, bypassing VideoCore for end-to-end verification.

## Project Status

**Status**: Planned
**Prerequisites**: [rpi4-graphics](../rpi4-graphics) project
**Verification**: Verus formal verification

## Motivation

The standard HDMI display path on Raspberry Pi 4 includes closed-source VideoCore firmware, creating an unverifiable trust gap. This project provides a **fully verifiable display path** using SPI-connected LCD with resistive touchscreen.

```
HDMI Path (unverifiable):
  Framebuffer → VideoCore (closed) → HDMI → Display
                    ❌

SPI Path (verifiable):
  Framebuffer → SPI Driver → LCD Controller → Display
       ✓            ✓             ✓
     Verus        Verus       Simple HW
```

## Hardware Requirements

### Recommended Display

**Waveshare 2.8" LCD Touch Module** (~$20)
- Display: ILI9341 controller, 320×240, 16-bit color
- Touch: XPT2046 resistive touch controller
- Interface: SPI (display + touch on same bus, different CS)
- Refresh: ~20 FPS full screen, 60+ FPS partial

Alternative options:
| Display | Resolution | Touch | Controller | Price |
|---------|------------|-------|------------|-------|
| Waveshare 2.8" | 320×240 | Resistive | ILI9341 + XPT2046 | ~$20 |
| Waveshare 3.5" | 480×320 | Resistive | ILI9486 + XPT2046 | ~$25 |
| Adafruit PiTFT 2.8" | 320×240 | Resistive | ILI9341 + STMPE610 | ~$35 |

### Wiring Diagram

```
Raspberry Pi 4 GPIO                 LCD + Touch Module
┌─────────────────────────┐        ┌─────────────────────┐
│                         │        │                     │
│  Pin 1  (3.3V)         ─┼───────→│ VCC                 │
│  Pin 6  (GND)          ─┼───────→│ GND                 │
│                         │        │                     │
│  === DISPLAY (SPI0) === │        │                     │
│  Pin 19 (GPIO10/MOSI)  ─┼───────→│ DIN  (MOSI)         │
│  Pin 23 (GPIO11/SCLK)  ─┼───────→│ CLK  (Clock)        │
│  Pin 24 (GPIO8/CE0)    ─┼───────→│ CS   (LCD Select)   │
│  Pin 22 (GPIO25)       ─┼───────→│ DC   (Data/Command) │
│  Pin 18 (GPIO24)       ─┼───────→│ RST  (Reset)        │
│  Pin 12 (GPIO18)       ─┼───────→│ BL   (Backlight)    │
│                         │        │                     │
│  === TOUCH (SPI0) ===   │        │                     │
│  Pin 21 (GPIO9/MISO)   ─┼←───────│ T_DO (MISO)         │
│  Pin 26 (GPIO7/CE1)    ─┼───────→│ T_CS (Touch Select) │
│  Pin 11 (GPIO17)       ─┼←───────│ T_IRQ (Interrupt)   │
│                         │        │                     │
└─────────────────────────┘        └─────────────────────┘

SPI Configuration:
- SPI0 at 0xFE204000 (BCM2711)
- Display: CS0, Mode 0, 32 MHz max
- Touch: CS1, Mode 0, 2 MHz max
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    VERIFIED SPI DISPLAY ARCHITECTURE                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                     APPLICATION LAYER                                │   │
│  │                                                                       │   │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐  │   │
│  │  │   UI Widgets    │  │  Touch Gestures │  │   Event Loop        │  │   │
│  │  │   (Verus ✓)     │  │   (Verus ✓)     │  │   (Verus ✓)         │  │   │
│  │  │                 │  │                 │  │                     │  │   │
│  │  │  - Button       │  │  - Tap          │  │  - Input dispatch   │  │   │
│  │  │  - Label        │  │  - Drag         │  │  - Render loop      │  │   │
│  │  │  - Slider       │  │  - Long press   │  │  - State machine    │  │   │
│  │  └────────┬────────┘  └────────┬────────┘  └──────────┬──────────┘  │   │
│  └───────────┼────────────────────┼──────────────────────┼──────────────┘   │
│              │                    │                      │                   │
│  ┌───────────┼────────────────────┼──────────────────────┼──────────────┐   │
│  │           ▼                    ▼                      │               │   │
│  │  ┌─────────────────────────────────────────┐         │  DRIVER LAYER │   │
│  │  │            GRAPHICS ENGINE              │         │               │   │
│  │  │               (Verus ✓)                 │         │               │   │
│  │  │                                         │         │               │   │
│  │  │  - Framebuffer (320×240×16bpp)         │         │               │   │
│  │  │  - Primitives (rect, line, text)       │         │               │   │
│  │  │  - Dirty region tracking               │◄────────┘               │   │
│  │  │  - Double buffering (optional)         │                          │   │
│  │  └──────────────────┬──────────────────────┘                          │   │
│  │                     │                                                  │   │
│  │  ┌──────────────────┴──────────────────────────────────────────────┐  │   │
│  │  │                                                                  │  │   │
│  │  │  ┌─────────────────────┐          ┌─────────────────────┐       │  │   │
│  │  │  │   ILI9341 Driver    │          │   XPT2046 Driver    │       │  │   │
│  │  │  │     (Verus ✓)       │          │     (Verus ✓)       │       │  │   │
│  │  │  │                     │          │                     │       │  │   │
│  │  │  │  - Init sequence    │          │  - Read X/Y/Z       │       │  │   │
│  │  │  │  - Window set       │          │  - Calibration      │       │  │   │
│  │  │  │  - Pixel write      │          │  - Filtering        │       │  │   │
│  │  │  │  - Partial update   │          │  - Debouncing       │       │  │   │
│  │  │  └──────────┬──────────┘          └──────────┬──────────┘       │  │   │
│  │  │             │                                │                   │  │   │
│  │  │             └────────────┬───────────────────┘                   │  │   │
│  │  │                          │                                       │  │   │
│  │  │             ┌────────────┴───────────────┐                       │  │   │
│  │  │             │      SPI Driver            │                       │  │   │
│  │  │             │        (Verus ✓)           │                       │  │   │
│  │  │             │                            │                       │  │   │
│  │  │             │  - Register access         │                       │  │   │
│  │  │             │  - DMA transfer            │                       │  │   │
│  │  │             │  - CS management           │                       │  │   │
│  │  │             │  - Clock configuration     │                       │  │   │
│  │  │             └────────────┬───────────────┘                       │  │   │
│  │  │                          │                                       │  │   │
│  │  └──────────────────────────┼───────────────────────────────────────┘  │   │
│  │                             │                                          │   │
│  │             ┌───────────────┴───────────────┐                          │   │
│  │             │       GPIO Driver             │                          │   │
│  │             │         (Verus ✓)             │                          │   │
│  │             │                               │                          │   │
│  │             │  - Pin configuration          │                          │   │
│  │             │  - DC/RST/BL control          │                          │   │
│  │             │  - IRQ handling               │                          │   │
│  │             └───────────────────────────────┘                          │   │
│  │                                                                        │   │
│  └────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                          │
│  ┌─────────────────────────────────┴────────────────────────────────────────┐│
│  │                         seL4 MICROKERNEL                                  ││
│  │                    (Isabelle/HOL Verified ✓)                             ││
│  └───────────────────────────────────────────────────────────────────────────┘│
│                                    │                                          │
│  ┌─────────────────────────────────┴────────────────────────────────────────┐│
│  │                    BCM2711 HARDWARE (Raspberry Pi 4)                      ││
│  │                                                                           ││
│  │   SPI0: 0xFE204000          GPIO: 0xFE200000                             ││
│  │   ┌─────────────────┐       ┌─────────────────┐                          ││
│  │   │  MOSI ──────────┼──────→│ GPIO10          │──→ LCD DIN               ││
│  │   │  MISO ←─────────┼───────│ GPIO9           │←── Touch DOUT            ││
│  │   │  SCLK ──────────┼──────→│ GPIO11          │──→ LCD/Touch CLK         ││
│  │   │  CE0  ──────────┼──────→│ GPIO8           │──→ LCD CS                ││
│  │   │  CE1  ──────────┼──────→│ GPIO7           │──→ Touch CS              ││
│  │   └─────────────────┘       │ GPIO25          │──→ DC                    ││
│  │                             │ GPIO24          │──→ RST                   ││
│  │                             │ GPIO18          │──→ Backlight             ││
│  │                             │ GPIO17          │←── Touch IRQ             ││
│  │                             └─────────────────┘                          ││
│  └───────────────────────────────────────────────────────────────────────────┘│
│                                                                               │
└───────────────────────────────────────────────────────────────────────────────┘
```

## File Structure

```
rpi4-spi-display/
├── README.md                 # This file
├── Cargo.toml
├── graphics.system           # Microkit system description
├── Makefile
│
├── src/
│   ├── lib.rs
│   ├── main.rs               # Protection domain entry
│   │
│   ├── hal/
│   │   ├── mod.rs
│   │   ├── gpio.rs           # GPIO driver (Verus ✓)
│   │   ├── spi.rs            # SPI driver (Verus ✓)
│   │   └── dma.rs            # DMA for fast transfers (optional)
│   │
│   ├── display/
│   │   ├── mod.rs
│   │   ├── ili9341.rs        # ILI9341 LCD driver (Verus ✓)
│   │   ├── framebuffer.rs    # 320×240 RGB565 buffer (Verus ✓)
│   │   └── text.rs           # Font rendering (Verus ✓)
│   │
│   ├── touch/
│   │   ├── mod.rs
│   │   ├── xpt2046.rs        # XPT2046 touch driver (Verus ✓)
│   │   ├── calibration.rs    # 3-point calibration (Verus ✓)
│   │   └── events.rs         # Touch event handling (Verus ✓)
│   │
│   └── ui/
│       ├── mod.rs
│       ├── widget.rs         # Widget trait and base types
│       ├── button.rs         # Touch button (Verus ✓)
│       ├── label.rs          # Text label (Verus ✓)
│       └── layout.rs         # Layout engine (Verus ✓)
│
└── specs/
    ├── spi_spec.rs           # SPI protocol specification
    ├── ili9341_spec.rs       # Display command specification
    └── touch_spec.rs         # Touch coordinate specification
```

## Verification Properties

### SPI Driver

```rust
verus! {
    /// SPI transfer completes with correct byte count
    pub fn spi_transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> (result: Result<(), SpiError>)
        requires
            tx.len() == rx.len(),
            tx.len() <= 65535,
            self.is_initialized(),
        ensures
            result.is_ok() ==> rx.len() == old(tx).len(),
    { ... }

    /// CS assertion is always paired with deassertion
    pub fn with_cs<F, R>(&mut self, cs: ChipSelect, f: F) -> R
        ensures
            self.cs_state(cs) == old(self).cs_state(cs),  // CS restored
    { ... }
}
```

### ILI9341 Display Driver

```rust
verus! {
    /// Pixel write is bounds-checked
    pub fn set_pixel(&mut self, x: u16, y: u16, color: Rgb565)
        requires
            x < 320,
            y < 240,
        ensures
            self.framebuffer[y * 320 + x] == color,
    { ... }

    /// Window setting respects display bounds
    pub fn set_window(&mut self, x0: u16, y0: u16, x1: u16, y1: u16)
        requires
            x0 <= x1 && x1 < 320,
            y0 <= y1 && y1 < 240,
    { ... }

    /// Full refresh sends exactly width × height × 2 bytes
    pub fn refresh(&mut self)
        ensures
            self.bytes_sent == 320 * 240 * 2,
    { ... }
}
```

### XPT2046 Touch Driver

```rust
verus! {
    /// Touch coordinates are within valid range after calibration
    pub fn read_touch(&mut self) -> (result: Option<TouchPoint>)
        ensures
            result.is_some() ==> (
                result.unwrap().x < 320 &&
                result.unwrap().y < 240 &&
                result.unwrap().pressure > 0
            ),
    { ... }

    /// Calibration matrix is invertible (valid)
    pub fn calibrate(&mut self, points: [(RawPoint, ScreenPoint); 3]) -> (valid: bool)
        ensures
            valid ==> self.calibration_valid(),
    { ... }
}
```

## Implementation Phases

### Phase 1: SPI + GPIO Drivers
- [ ] BCM2711 SPI0 register definitions
- [ ] SPI master driver with Verus specs
- [ ] GPIO pin configuration for DC/RST/BL
- [ ] Basic loopback test

### Phase 2: Display Driver
- [ ] ILI9341 initialization sequence
- [ ] Window and pixel commands
- [ ] RGB565 framebuffer
- [ ] Full-screen refresh
- [ ] Partial update optimization

### Phase 3: Touch Driver
- [ ] XPT2046 SPI protocol
- [ ] Raw coordinate reading
- [ ] 3-point calibration
- [ ] Touch event state machine (down/move/up)
- [ ] Filtering and debouncing

### Phase 4: Graphics Primitives
- [ ] Rectangle fill
- [ ] Line drawing (Bresenham)
- [ ] Bitmap font rendering
- [ ] Dirty region tracking

### Phase 5: UI Framework
- [ ] Widget trait definition
- [ ] Button widget with touch handling
- [ ] Label widget
- [ ] Simple layout engine
- [ ] Event dispatch

### Phase 6: Integration
- [ ] Combine with rpi4-graphics HDMI demo
- [ ] Dual-display support
- [ ] TPM attestation on SPI display
- [ ] Performance optimization (DMA)

## Performance Expectations

| Operation | Estimated Time | Notes |
|-----------|---------------|-------|
| Full refresh (320×240) | ~50ms @ 32MHz | 153,600 bytes |
| Partial update (100×100) | ~6ms | 20,000 bytes |
| Touch read | ~100μs | 3 SPI transactions |
| Button response | <10ms | Touch to visual feedback |

With DMA: Full refresh drops to ~15ms (background transfer).

## Dependencies

```toml
[dependencies]
sel4-microkit = { git = "https://github.com/seL4/rust-sel4" }
verus_builtin_macros = "0.0.0-2025-12-07-0054"
verus_builtin = "0.0.0-2025-12-07-0054"

# No external display/touch crates - all verified from scratch
```

## References

- [ILI9341 Datasheet](https://cdn-shop.adafruit.com/datasheets/ILI9341.pdf)
- [XPT2046 Datasheet](https://www.waveshare.com/w/upload/5/55/XPT2046_Datasheet.pdf)
- [BCM2711 ARM Peripherals](https://datasheets.raspberrypi.com/bcm2711/bcm2711-peripherals.pdf) (Section 10: SPI)
- [Verus Lang](https://github.com/verus-lang/verus)

## License

MIT
