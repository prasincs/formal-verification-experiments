# Secure Photo Frame Architecture

## Overview

A photo frame demo leveraging seL4's capability-based isolation to create a **defense-in-depth** architecture where a malicious image file cannot compromise the system.

## Threat Model

**Primary Threat:** Malicious image files designed to exploit parser vulnerabilities.

Image decoders (JPEG, PNG, BMP) have historically been a major attack vector:
- CVE-2004-0200 (GDI+ JPEG buffer overflow)
- CVE-2016-3714 (ImageMagick RCE)
- Countless browser image parser exploits

**Our Defense:** Even if a malicious image exploits the decoder, seL4's capability system ensures the compromised PD cannot:
- Access the SD card to exfiltrate data
- Write to the display without going through verified protocol
- Access network or input devices
- Escape to other PDs

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            seL4 Microkernel                                   │
│                       (Formally Verified TCB)                                 │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
    ┌───────────────┬───────────────┼───────────────┬───────────────┐
    │               │               │               │               │
    ▼               ▼               ▼               ▼               ▼
┌─────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌─────────┐
│ Input   │   │ Storage  │   │ Decoder  │   │ Display  │   │ Timer   │
│ PD      │   │ PD       │   │ PD       │   │ PD       │   │ PD      │
│         │   │          │   │          │   │          │   │         │
│ UART    │   │ SD Card  │   │ Image    │   │ Frame-   │   │ Slide-  │
│ polling │   │ access   │   │ parsing  │   │ buffer   │   │ show    │
└────┬────┘   └────┬─────┘   └────┬─────┘   └────┬─────┘   └────┬────┘
     │             │              │              │              │
     │        raw bytes      pixels only    render only    timeout
     │             │              │              │              │
     └─────────────┴──────────────┴──────────────┴──────────────┘
                              Shared Memory Regions
                           (Verified Ring Buffers)
```

## Protection Domain Responsibilities

### Input PD (existing)
- Polls UART for keyboard input
- Sends navigation commands (next/prev/pause)
- **Memory:** UART registers + input ring buffer
- **Capabilities:** Notify Display PD

### Storage PD (new)
- Reads photo files from embedded storage / SD card
- Enumerates available photos
- Provides raw file bytes (no interpretation)
- **Memory:** SD card controller registers + file data buffer
- **Capabilities:** Notify Decoder PD
- **Security:** Cannot access framebuffer or network

### Decoder PD (new) - UNTRUSTED
- Receives raw image bytes from Storage PD
- Decodes BMP/TGA/PNG to raw pixels
- Validates output dimensions
- **Memory:** Input buffer (from Storage) + Output buffer (to Display)
- **Capabilities:** Notify Display PD only
- **Security:**
  - CANNOT access SD card (no exfiltration)
  - CANNOT access framebuffer directly (only through verified buffer)
  - Runs at lowest priority
  - Memory quota limited

### Display PD (new)
- Receives validated pixel data from Decoder PD
- Renders to framebuffer with bounds checking
- Handles transitions, UI chrome
- **Memory:** Framebuffer + GPU mailbox + pixel ring buffer
- **Capabilities:** Full display access

### Timer PD (new)
- Manages slideshow timing
- Sends periodic notifications
- **Memory:** Timer registers only
- **Capabilities:** Notify Display PD

## Memory Regions

| Region | Size | Storage | Decoder | Display | Input | Timer |
|--------|------|---------|---------|---------|-------|-------|
| SD Controller | 4KB | RW | - | - | - | - |
| File Buffer | 1MB | RW | R | - | - | - |
| Pixel Buffer | 4MB | - | RW | R | - | - |
| Framebuffer | 8MB | - | - | RW | - | - |
| GPU Mailbox | 4KB | - | - | RW | - | - |
| Command Ring | 4KB | R | R | RW | RW | RW |
| UART | 4KB | - | - | - | RW | - |
| Timer | 4KB | - | - | - | - | RW |

## IPC Protocols

### Photo Command Protocol
```rust
#[repr(C)]
pub struct PhotoCommand {
    command_type: u8,    // 0=None, 1=Next, 2=Prev, 3=Pause, 4=Resume, 5=Load
    photo_index: u16,    // For direct load
    _reserved: u8,
}
```

### File Transfer Protocol (Storage → Decoder)
```rust
#[repr(C)]
pub struct FileChunk {
    chunk_id: u32,       // Sequence number
    total_chunks: u32,   // For progress tracking
    data_len: u32,       // Actual bytes in this chunk (≤ 4096)
    file_type: u8,       // 0=BMP, 1=TGA, 2=PNG
    flags: u8,           // 1=FIRST, 2=LAST
    _reserved: u16,
    // Followed by data in shared buffer
}
```

### Pixel Transfer Protocol (Decoder → Display)
```rust
#[repr(C)]
pub struct PixelBuffer {
    width: u32,          // Validated ≤ MAX_WIDTH
    height: u32,         // Validated ≤ MAX_HEIGHT
    format: u8,          // 0=RGB24, 1=RGBA32
    status: u8,          // 0=Pending, 1=Ready, 2=Error
    _reserved: u16,
    // Followed by pixel data
}
```

## Verus Verification

### Bounds Verification
```rust
verus! {
    // Pixel buffer dimensions are within display bounds
    pub open spec fn valid_pixel_buffer(buf: &PixelBuffer, max_w: u32, max_h: u32) -> bool {
        buf.width <= max_w &&
        buf.height <= max_h &&
        buf.width > 0 &&
        buf.height > 0
    }

    // Pixel index calculation cannot overflow
    pub open spec fn pixel_index_safe(x: u32, y: u32, width: u32, height: u32) -> bool {
        x < width && y < height &&
        (y as u64) * (width as u64) + (x as u64) < u32::MAX as u64
    }
}
```

### Isolation Specification
```rust
verus! {
    // Decoder PD has no access to storage regions
    pub open spec fn decoder_cannot_access_storage() -> bool {
        forall|addr: usize|
            decoder_pd_can_access(addr) ==> !in_sd_controller_region(addr)
    }

    // Decoder PD has no direct framebuffer access
    pub open spec fn decoder_cannot_access_framebuffer() -> bool {
        forall|addr: usize|
            decoder_pd_can_access(addr) ==> !in_framebuffer_region(addr)
    }
}
```

## Security Analysis

### Attack: Malicious BMP with crafted header
**Scenario:** Attacker creates BMP with invalid dimensions to cause buffer overflow in decoder.
**Mitigation:**
1. Decoder runs in isolated PD with limited memory quota
2. Display PD validates dimensions before blitting
3. Verus proves bounds checks in pixel operations

### Attack: Decoder compromise attempts exfiltration
**Scenario:** Compromised decoder tries to read other photos to find sensitive data.
**Mitigation:** Decoder has NO capability to Storage PD. seL4 enforces this at runtime.

### Attack: Decoder writes malicious pixels to cause GPU exploit
**Scenario:** Crafted pixel data designed to exploit GPU firmware.
**Mitigation:**
1. Pixel format is validated (only RGB24/RGBA32)
2. Dimensions bounded to display size
3. No GPU command access from Decoder PD

## Photo Format Support

### Phase 1: BMP (Simplest)
- No compression (direct pixel copy)
- Simple header parsing
- Minimal attack surface
- `tinybmp` crate (no_std, no alloc)

### Phase 2: TGA
- RLE compression only
- Small parser
- `tinytga` crate

### Phase 3: PNG (if needed)
- Requires `zune-png` or similar
- More complex, larger attack surface
- Only add if BMP/TGA insufficient

## Implementation Plan

1. **Photo Protocol Library** (`rpi4-photo-protocol/`)
   - Verus-verified data structures
   - Command and pixel buffer definitions
   - Bounds checking proofs

2. **Storage PD** (`rpi4-storage-pd/`)
   - Embedded photo data (compile-time)
   - Or: SD card FAT32 reader
   - File enumeration

3. **Decoder PD** (`rpi4-decoder-pd/`)
   - BMP decoder (tinybmp)
   - Pixel format conversion
   - Error handling (malformed images)

4. **Display PD** (`rpi4-display-pd/`)
   - Framebuffer management
   - Photo rendering with scaling
   - UI chrome (photo index, controls)
   - Slideshow state machine

5. **System Integration** (`photoframe.system`)
   - Full 5-PD configuration
   - Memory region definitions
   - Channel setup

## Simplified 3-PD Version

For initial demo, we can simplify to 3 PDs:

```
┌───────────┐     ┌───────────┐     ┌───────────┐
│  Input    │────▶│  Decoder  │────▶│  Display  │
│    PD     │     │    PD     │     │    PD     │
└───────────┘     └───────────┘     └───────────┘
    UART         Photos embedded     Framebuffer
                  in binary
```

- Photos compiled into Decoder PD binary
- No runtime file loading
- Still demonstrates isolation of untrusted decoder

## Build Configuration

```makefile
# New product: photoframe
PRODUCT=photoframe
PLATFORM=rpi4
ISOLATED=1

# Builds:
# - input_pd.elf
# - decoder_pd.elf
# - display_pd.elf
```

## Future Enhancements

1. **SD Card Support**: Full FAT32 filesystem in Storage PD
2. **Network Loading**: WiFi PD for downloading photos (highly isolated)
3. **Encryption**: Photos encrypted at rest, decrypted in secure PD
4. **Remote Control**: IR receiver in Input PD
5. **Multiple Displays**: SPI LCD preview + HDMI main display
