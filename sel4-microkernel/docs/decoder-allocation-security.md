# Derisking Image Decoder Allocation

## The Problem

Real photo formats (JPEG, PNG) require heap allocation for:
1. **Decompression buffers** - Intermediate decode state
2. **Huffman tables** - JPEG entropy coding
3. **zlib inflate** - PNG decompression
4. **Color conversion** - Working buffers

This introduces two risk categories:
- **Memory exhaustion** - Malicious file causes OOM
- **Parser vulnerabilities** - Buffer overflows, integer overflows, use-after-free

## Historical Parser Vulnerabilities

| CVE | Format | Impact | Root Cause |
|-----|--------|--------|------------|
| CVE-2004-0200 | JPEG (GDI+) | RCE | Integer overflow in buffer size |
| CVE-2016-3714 | ImageMagick | RCE | Command injection via filename |
| CVE-2018-14618 | libpng | DoS/RCE | Heap buffer overflow |
| CVE-2020-13790 | libjpeg-turbo | Info leak | Uninitialized memory read |
| CVE-2021-21897 | libwebp | RCE | Heap buffer overflow |

**Pattern**: Image parsers are consistently high-risk attack surface.

## Defense-in-Depth Strategy

### Layer 1: seL4 Capability Isolation (Runtime)

```
┌──────────────────────────────────────────────────────────────┐
│                      seL4 Microkernel                         │
│                  (Formally Verified TCB)                      │
└──────────────────────────────────────────────────────────────┘
         │                    │                    │
         ▼                    ▼                    ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│   Storage PD    │  │   Decoder PD    │  │   Display PD    │
│                 │  │   (UNTRUSTED)   │  │                 │
│ • SD card       │  │ • JPEG/PNG      │  │ • Framebuffer   │
│ • File read     │  │ • Bounded heap  │  │ • Render        │
│                 │  │ • No FB access  │  │                 │
└────────┬────────┘  └────────┬────────┘  └────────┬────────┘
         │                    │                    │
         │    raw bytes       │    pixels only     │
         └────────────────────┴────────────────────┘
                     Shared Memory Buffers

Even if Decoder PD is fully compromised:
✗ Cannot access framebuffer (no capability)
✗ Cannot access storage (no capability)
✗ Cannot access network (no capability)
✗ Cannot access other PDs (no capability)
✓ Can only write to pixel buffer (validated by Display PD)
```

### Layer 2: Bounded Allocator

Use a **fixed-size memory pool** that cannot grow:

```rust
//! Bounded allocator for Decoder PD
//!
//! Limits total allocation to prevent memory exhaustion attacks.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Maximum heap size for decoder (e.g., 8MB)
const DECODER_HEAP_SIZE: usize = 8 * 1024 * 1024;

/// Bounded allocator that fails when limit reached
pub struct BoundedAllocator {
    /// Fixed memory pool
    pool: UnsafeCell<[u8; DECODER_HEAP_SIZE]>,
    /// Current allocation offset (bump allocator)
    offset: AtomicUsize,
    /// High water mark for debugging
    peak: AtomicUsize,
}

unsafe impl Sync for BoundedAllocator {}

impl BoundedAllocator {
    pub const fn new() -> Self {
        Self {
            pool: UnsafeCell::new([0; DECODER_HEAP_SIZE]),
            offset: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }
    }

    /// Reset allocator (call between photos)
    pub fn reset(&self) {
        self.offset.store(0, Ordering::Release);
    }

    /// Get current usage
    pub fn usage(&self) -> usize {
        self.offset.load(Ordering::Acquire)
    }

    /// Get peak usage
    pub fn peak_usage(&self) -> usize {
        self.peak.load(Ordering::Acquire)
    }
}

unsafe impl GlobalAlloc for BoundedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        loop {
            let current = self.offset.load(Ordering::Acquire);

            // Align up
            let aligned = (current + align - 1) & !(align - 1);
            let new_offset = aligned + size;

            // Check bounds
            if new_offset > DECODER_HEAP_SIZE {
                // Allocation would exceed limit - return null
                return null_mut();
            }

            // Try to claim this space
            match self.offset.compare_exchange_weak(
                current, new_offset,
                Ordering::AcqRel, Ordering::Acquire
            ) {
                Ok(_) => {
                    // Update peak
                    let _ = self.peak.fetch_max(new_offset, Ordering::Relaxed);

                    let pool_ptr = self.pool.get() as *mut u8;
                    return pool_ptr.add(aligned);
                }
                Err(_) => continue,  // Retry
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator - individual deallocs are no-ops
        // Memory is reclaimed on reset()
    }
}

#[global_allocator]
static ALLOCATOR: BoundedAllocator = BoundedAllocator::new();
```

**Key properties:**
- **Fixed 8MB limit** - Cannot allocate more regardless of input
- **Bump allocator** - Simple, no fragmentation exploits
- **Reset between photos** - Fresh heap for each decode
- **Usage tracking** - Detect attempted over-allocation

### Layer 3: Input Validation (Before Parsing)

Validate image headers before full decode:

```rust
/// Maximum dimensions we'll accept
const MAX_WIDTH: u32 = 4096;
const MAX_HEIGHT: u32 = 4096;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;  // 16 megapixels

/// Pre-validate JPEG without full decode
pub fn validate_jpeg_header(data: &[u8]) -> Result<(u32, u32), ValidationError> {
    // Check SOI marker
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err(ValidationError::InvalidMagic);
    }

    // Find SOF0/SOF2 marker for dimensions
    let mut pos = 2;
    while pos + 4 < data.len() {
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }

        let marker = data[pos + 1];
        let length = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;

        // SOF0 (baseline) or SOF2 (progressive)
        if marker == 0xC0 || marker == 0xC2 {
            if pos + 9 > data.len() {
                return Err(ValidationError::TruncatedHeader);
            }
            let height = u16::from_be_bytes([data[pos + 5], data[pos + 6]]) as u32;
            let width = u16::from_be_bytes([data[pos + 7], data[pos + 8]]) as u32;

            // Validate dimensions
            if width == 0 || height == 0 {
                return Err(ValidationError::ZeroDimension);
            }
            if width > MAX_WIDTH || height > MAX_HEIGHT {
                return Err(ValidationError::TooLarge);
            }
            if (width as u64) * (height as u64) > MAX_PIXELS {
                return Err(ValidationError::TooManyPixels);
            }

            return Ok((width, height));
        }

        pos += 2 + length;
    }

    Err(ValidationError::NoFrameHeader)
}

/// Pre-validate PNG without full decode
pub fn validate_png_header(data: &[u8]) -> Result<(u32, u32), ValidationError> {
    // PNG signature
    const PNG_SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if data.len() < 24 || &data[0..8] != PNG_SIG {
        return Err(ValidationError::InvalidMagic);
    }

    // IHDR chunk (must be first)
    let ihdr_len = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    if ihdr_len != 13 || &data[12..16] != b"IHDR" {
        return Err(ValidationError::InvalidHeader);
    }

    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);

    if width == 0 || height == 0 {
        return Err(ValidationError::ZeroDimension);
    }
    if width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(ValidationError::TooLarge);
    }
    if (width as u64) * (height as u64) > MAX_PIXELS {
        return Err(ValidationError::TooManyPixels);
    }

    Ok((width, height))
}
```

### Layer 4: Parser Selection (Reduce Attack Surface)

Choose parsers carefully:

| Crate | Language | Audit Status | Notes |
|-------|----------|--------------|-------|
| `zune-jpeg` | Rust | Partial | Fast, memory-safe |
| `zune-png` | Rust | Partial | Rust rewrite of libpng |
| `image-rs` | Rust | Community reviewed | Kitchen sink, large |
| `jpeg-decoder` | Rust | Mozilla reviewed | Simpler, audited |

**Recommendation**: Use `zune-*` crates - pure Rust, no unsafe in hot paths.

```toml
[dependencies]
# Minimal JPEG decoder (no_std compatible with alloc)
zune-jpeg = { version = "0.4", default-features = false, features = ["std"] }

# Minimal PNG decoder
zune-png = { version = "0.4", default-features = false }
```

### Layer 5: Output Validation (Display PD)

The Display PD validates everything from Decoder:

```rust
/// Validate pixel buffer from untrusted Decoder PD
pub fn validate_pixel_buffer(header: &PixelBufferHeader) -> Result<(), ValidationError> {
    // Dimension bounds
    if header.width == 0 || header.width > MAX_DISPLAY_WIDTH {
        return Err(ValidationError::InvalidWidth);
    }
    if header.height == 0 || header.height > MAX_DISPLAY_HEIGHT {
        return Err(ValidationError::InvalidHeight);
    }

    // Data length matches dimensions
    let expected_len = header.width as usize * header.height as usize * 4;
    if header.data_len as usize != expected_len {
        return Err(ValidationError::DataLengthMismatch);
    }

    // Status is valid
    if header.status != BUFFER_STATUS_READY {
        return Err(ValidationError::NotReady);
    }

    Ok(())
}

/// Render with bounds checking (cannot be bypassed by Decoder)
pub fn render_validated(
    framebuffer: &mut Framebuffer,
    pixel_buffer: &[u32],
    header: &PixelBufferHeader,
) {
    // Even if Decoder sent garbage dimensions, we clamp to display
    let dst_w = framebuffer.width().min(header.width);
    let dst_h = framebuffer.height().min(header.height);

    for y in 0..dst_h {
        for x in 0..dst_w {
            let src_idx = (y * header.width + x) as usize;
            if src_idx < pixel_buffer.len() {
                framebuffer.put_pixel(x, y, pixel_buffer[src_idx]);
            }
        }
    }
}
```

### Layer 6: Watchdog / Timeout

Detect infinite loops or excessive CPU:

```rust
/// Decode with timeout (seL4 timer-based)
pub fn decode_with_timeout(
    data: &[u8],
    output: &mut [u32],
    timeout_ms: u32,
) -> Result<(u32, u32), DecodeError> {
    // Start timer
    let deadline = get_current_time_ms() + timeout_ms;

    // Set watchdog
    set_decode_watchdog(deadline);

    // Attempt decode
    let result = decode_jpeg(data, output);

    // Clear watchdog
    clear_decode_watchdog();

    result
}
```

## Memory Budget

For a 1920×1080 display with JPEG photos:

| Component | Size | Notes |
|-----------|------|-------|
| Output buffer | 8 MB | 1920×1080×4 bytes RGBA |
| JPEG decode buffer | 2-4 MB | Depends on quality |
| Huffman tables | ~1 MB | Per-image |
| Color conversion | ~1 MB | YCbCr → RGB |
| **Total Decoder PD** | **~16 MB** | Conservative |

With 8MB bounded allocator + 8MB output buffer, we're safe.

## Implementation Phases

### Phase 1: Bounded Allocator
- Implement bump allocator with fixed pool
- Add to Decoder PD
- Test with over-allocation attempts

### Phase 2: Header Validation
- JPEG SOF parsing
- PNG IHDR parsing
- Reject before decode

### Phase 3: zune-jpeg Integration
- Add dependency
- Wire up bounded allocator
- Reset between photos

### Phase 4: Full 3-PD Split
- Separate Decoder PD from Display PD
- seL4 enforced isolation
- Shared pixel buffer only

## Summary: Defense Layers

| Layer | What It Catches | Verified By |
|-------|-----------------|-------------|
| seL4 isolation | Capability escape | Isabelle/HOL proofs |
| Bounded allocator | Memory exhaustion | Code review |
| Header validation | Malformed dimensions | Unit tests |
| Rust memory safety | Buffer overflows | Compiler |
| Output validation | Garbage pixels | Display PD code |
| Timeout | Infinite loops | Timer PD |

**Key insight**: Even with allocation, the Decoder PD is **sandboxed by hardware-enforced capabilities**. A fully compromised decoder cannot escape its memory region or access any other resources.
