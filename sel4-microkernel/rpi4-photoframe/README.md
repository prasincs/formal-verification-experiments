# Secure Photo Frame for seL4 Microkit

A photo frame demo demonstrating **defense-in-depth isolation** using the seL4 microkernel.

## Security Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     seL4 Microkernel                         │
│                  (Formally Verified TCB)                     │
└─────────────────────────────────────────────────────────────┘
                             │
        ┌────────────────────┴────────────────────┐
        │                                         │
        ▼                                         ▼
┌───────────────────┐                   ┌───────────────────┐
│    Input PD       │                   │  Photoframe PD    │
│  (priority 200)   │                   │  (priority 150)   │
├───────────────────┤                   ├───────────────────┤
│ Memory Access:    │    notification   │ Memory Access:    │
│ • UART registers  │ ────────────────▶ │ • Mailbox regs    │
│ • Ring buffer     │                   │ • GPIO registers  │
│                   │    shared mem     │ • Framebuffer     │
│ CANNOT access:    │ ◀───────────────▶ │ • Ring buffer     │
│ • Framebuffer     │   [4KB input]     │                   │
│ • Mailbox         │                   │ CANNOT access:    │
│ • GPIO            │                   │ • UART registers  │
└───────────────────┘                   └───────────────────┘
        │                                         │
        ▼                                         ▼
   ┌─────────┐                            ┌─────────────┐
   │  UART   │                            │    HDMI     │
   │ Console │                            │  Display    │
   └─────────┘                            └─────────────┘
```

## Features

- **Slideshow Mode**: Auto-advances photos every 5 seconds
- **Manual Navigation**: Left/Right arrows to browse
- **Pause/Resume**: Space bar toggles slideshow
- **Info Overlay**: Enter key toggles photo information
- **Isolated Input**: Input PD cannot access display memory

## Controls

| Key | Action |
|-----|--------|
| ← / → | Previous / Next photo |
| ↑ / ↓ | Previous / Next photo |
| Space | Pause / Resume slideshow |
| Enter | Toggle info overlay |
| Escape | Return to first photo |

## Building

```bash
# Build for Raspberry Pi 4
cd sel4-microkernel
make PRODUCT=photoframe PLATFORM=rpi4 sdcard

# Flash to SD card
sudo dd if=build/rpi4/photoframe/rpi4-sel4-photoframe.img of=/dev/sdX bs=4M conv=fsync
```

## Security Properties

### Enforced by seL4 (Runtime)

1. **Input PD cannot access framebuffer** - No capability mapping
2. **Photoframe PD cannot access UART** - No capability mapping
3. **Only ring buffer is shared** - Explicit mapping in system description

### Verified by Verus (Compile-time)

1. **Ring buffer bounds** - Indices never exceed capacity
2. **Key code validation** - Only valid codes transmitted
3. **Protocol correctness** - SPSC discipline maintained

## Future Enhancements

### 3-PD Architecture (Decoder Isolation)

For production use with untrusted image files:

```
Input PD ──▶ Decoder PD ──▶ Display PD
 (UART)      (BMP parse)    (Framebuffer)
```

The Decoder PD would:
- Parse potentially malicious image files
- Have NO access to framebuffer (only pixel buffer)
- Have NO access to storage (only receive file chunks)
- Be isolated so compromise cannot affect display

### SD Card Photo Loading

Add Storage PD for runtime photo loading:
- FAT32 filesystem support
- Photo enumeration
- Streaming transfer protocol

## Demo Photos

Currently uses 5 embedded test patterns:
1. **GRADIENT** - Color gradient
2. **CIRCLES** - Concentric circles
3. **CHECKERBOARD** - Classic pattern
4. **SUNSET** - Sunset with sun
5. **MOUNTAINS** - Mountain silhouette

For real photos, embed BMP files using `include_bytes!()` or add SD card support.
