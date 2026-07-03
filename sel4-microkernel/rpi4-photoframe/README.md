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

The slideshow mixes procedural patterns with **real encoded images** decoded at
runtime through the secure pipeline:

| # | Name | Source | Decode path |
|---|------|--------|-------------|
| 1 | GRADIENT | procedural | none |
| 2 | QOI PHOTO | `photos/sample_gradient.qoi` | inline QOI (no alloc) |
| 3 | BMP PHOTO | `photos/sample_gradient.bmp` | tinybmp (no alloc) |
| 4 | CIRCLES | procedural | none |
| 5 | CHECKERBOARD | procedural | none |
| 6 | SUNSET | procedural | none |
| 7 | MOUNTAINS | procedural | none |

## Image Formats & Secure Decode Pipeline

Encoded photos are decoded through `secure_decode_into()`, which enforces
defense-in-depth before and during decode:

```
raw bytes
   │
   ├─ 1. validate header   (validate.rs)      reject bad magic / oversized / zero-dim
   │                                          BEFORE any allocation
   ├─ 2. budget check       (estimate ≤ heap cap)   reject memory bombs
   ├─ 3. reset bounded heap (bounded_alloc.rs)      clean slate per photo
   ├─ 4. decode             (decoder.rs)            against fixed 16 MB pool
   └─ 5. verify no OOM      (HeapControl)           detect over-allocation
   │
   ▼
ARGB32 pixels → blitted centered to framebuffer
```

Supported formats:

| Format | Decoder | Allocation | Notes |
|--------|---------|------------|-------|
| BMP | tinybmp | none | uncompressed |
| QOI | inline (~100 LoC) | none | lossless, ~30% of BMP |
| PNG | zune-png | bounded heap | inflate under 16 MB cap |
| JPEG | zune-jpeg | bounded heap | baseline + progressive |

JPEG/PNG decoders allocate **only** through the
`BoundedBumpAllocator` wired as `#[global_allocator]`, so a malicious image
cannot exhaust memory — once the pool is full, allocation fails and the pipeline
returns `OutOfMemory` instead of growing unbounded. See
[`docs/decoder-allocation-security.md`](../docs/decoder-allocation-security.md).

### Adding your own photo

```bash
# Drop a file in photos/ and reference it in src/main.rs:
#   const MY_PHOTO: &[u8] = include_bytes!("../photos/my_photo.jpg");
#   Photo { name: "MY PHOTO", source: PhotoSource::Encoded(MY_PHOTO) },
# JPEG/PNG/BMP/QOI all work; the format is auto-detected from magic bytes.
```

The on-screen info overlay shows the secure-decode result for each encoded
photo: `SECURE DECODE: <FMT> OK  HEAP PEAK <N> KB`, or `REJECTED <reason>` if the
pipeline refuses the image.

### Future: SD Card Photo Loading

Add Storage PD for runtime photo loading:
- FAT32 filesystem support
- Photo enumeration
- Streaming transfer protocol
