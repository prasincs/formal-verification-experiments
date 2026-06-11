//! # Secure Decode Pipeline
//!
//! Orchestrates the defense-in-depth flow for decoding untrusted images,
//! combining the pieces documented in `docs/decoder-allocation-security.md`:
//!
//! ```text
//!   raw bytes
//!      │
//!      ▼
//!   1. validate header        (validate::validate_auto)   ── reject malformed/oversized
//!      │                                                      BEFORE any allocation
//!      ▼
//!   2. budget check           (estimated_memory ≤ heap cap) ── reject memory bombs
//!      │
//!      ▼
//!   3. reset bounded heap     (HeapControl::reset)          ── clean slate per photo
//!      │
//!      ▼
//!   4. decode                 (decoder::decode_*)           ── bounded global allocator
//!      │
//!      ▼
//!   5. verify no OOM          (HeapControl::oom_occurred)   ── detect over-allocation
//!      │
//!      ▼
//!   ARGB32 pixels in caller-owned `output`
//! ```
//!
//! The heap referenced here is the process-wide `#[global_allocator]`, a
//! [`crate::bounded_alloc::BoundedBumpAllocator`] with a fixed pool. Even a
//! fully-compromised decoder cannot allocate past that cap, and — under seL4 —
//! cannot reach the framebuffer, storage, or any other PD regardless.

use crate::bounded_alloc::HeapControl;
use crate::decoder::{self, DecodeError};
use crate::validate::{self, ImageType, ValidatedImage, ValidationError};

/// Outcome of a successful secure decode.
#[derive(Debug, Clone, Copy)]
pub struct SecureDecodeResult {
    pub width: u32,
    pub height: u32,
    pub format: ImageType,
    /// Heap bytes in use after decode (0 for no-allocation formats).
    pub heap_used: usize,
    /// Peak heap bytes during decode.
    pub heap_peak: usize,
}

/// Why a secure decode was rejected or failed.
#[derive(Debug, Clone, Copy)]
pub enum SecureDecodeError {
    /// Header validation rejected the image (malformed / oversized / zero dim).
    Validation(ValidationError),
    /// Estimated decode memory exceeds the heap budget.
    ExceedsBudget { needed: usize, budget: usize },
    /// The output pixel buffer is too small for the image.
    OutputTooSmall { needed: usize, have: usize },
    /// The decoder itself failed (corrupt data, unsupported variant).
    Decode(DecodeError),
    /// The bounded heap hit its cap mid-decode (possible attack / underestimate).
    OutOfMemory { peak: usize, budget: usize },
}

impl From<ValidationError> for SecureDecodeError {
    fn from(e: ValidationError) -> Self {
        SecureDecodeError::Validation(e)
    }
}
impl From<DecodeError> for SecureDecodeError {
    fn from(e: DecodeError) -> Self {
        SecureDecodeError::Decode(e)
    }
}

/// Run the full secure pipeline, decoding `data` into `output` (ARGB32).
///
/// `heap` is the bounded global allocator (passed as `&dyn HeapControl` so this
/// stays independent of the heap's compile-time size). `output` must be large
/// enough to hold `width * height` pixels.
pub fn secure_decode_into(
    data: &[u8],
    output: &mut [u32],
    heap: &dyn HeapControl,
) -> Result<SecureDecodeResult, SecureDecodeError> {
    // ---- 1. Validate header (no allocation yet) ----------------------------
    let info: ValidatedImage = validate::validate_auto(data)?;

    // ---- 2. Budget checks --------------------------------------------------
    let budget = heap.capacity();
    if info.estimated_memory > budget {
        return Err(SecureDecodeError::ExceedsBudget {
            needed: info.estimated_memory,
            budget,
        });
    }

    let pixel_count = (info.width as usize).saturating_mul(info.height as usize);
    if output.len() < pixel_count {
        return Err(SecureDecodeError::OutputTooSmall {
            needed: pixel_count,
            have: output.len(),
        });
    }

    // ---- 3. Reset the bounded heap (reclaim previous photo) ----------------
    heap.reset();

    // ---- 4. Decode against the bounded allocator ---------------------------
    let decode_result = match info.format {
        ImageType::Jpeg => decoder::decode_jpeg(data, output),
        ImageType::Png => decoder::decode_png(data, output),
        ImageType::Bmp => decoder::decode_bmp(data, output),
        ImageType::Qoi => decoder::decode_qoi(data, output),
        // TGA has no magic bytes, so validate_auto never yields it; handle for
        // completeness in case a caller validates by extension instead.
        ImageType::Tga => decoder::decode_tga(data, output),
        ImageType::Unknown => Err(DecodeError::UnsupportedFormat),
    };

    // ---- 5. Verify the heap never hit its cap ------------------------------
    // Check OOM even on the error path: a decode failure caused by OOM should
    // be reported as such, not as generic corruption.
    if heap.oom_occurred() {
        return Err(SecureDecodeError::OutOfMemory {
            peak: heap.peak(),
            budget,
        });
    }

    let (width, height) = decode_result?;

    Ok(SecureDecodeResult {
        width,
        height,
        format: info.format,
        heap_used: heap.used(),
        heap_peak: heap.peak(),
    })
}
