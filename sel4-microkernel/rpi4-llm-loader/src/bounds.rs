//! Verified bounds surface for the GGUF loader.
//!
//! Every byte the parser reads and every size it computes goes through the
//! *total* functions in this module: no preconditions, an `Option`/`bool`
//! result, and a Verus postcondition tying the result to a specification
//! decode. Because nothing here has a `requires` clause, the (unverified)
//! parse loop in `gguf.rs` cannot misuse these primitives — a bad offset
//! yields `None`, never a panic, an out-of-bounds read, or an overflow.
//!
//! This module is deliberately self-contained (constants included) so the
//! Verus harness can check it as a standalone crate root, same as
//! `update-capsule/src/header.rs`:
//!
//! ```text
//! verus --crate-type lib src/bounds.rs
//! ```

// Verus reasons about the plain `%` / `-` operators, not the std helper
// methods clippy prefers; keep the verified code in operator form.
#![allow(clippy::manual_is_multiple_of)]

use verus_builtin_macros::verus;
// Used only by ghost code, which `cargo build` strips.
#[allow(unused_imports)]
use vstd::prelude::*;
use vstd::slice::{slice_index_get, slice_subrange};

verus! {

// ============================================================================
// GGML TENSOR TYPES (the closed set this loader accepts)
// ============================================================================

pub const GGML_TYPE_F32: u32 = 0;
pub const GGML_TYPE_F16: u32 = 1;
pub const GGML_TYPE_Q4_0: u32 = 2;
pub const GGML_TYPE_Q8_0: u32 = 8;

/// Elements per quantization block for the block-quantized types.
pub const QUANT_BLOCK_ELEMS: u64 = 32;
/// Bytes per Q4_0 block: 2-byte f16 scale + 16 nibble bytes.
pub const Q4_0_BLOCK_BYTES: u64 = 18;
/// Bytes per Q8_0 block: 2-byte f16 scale + 32 i8 quants.
pub const Q8_0_BLOCK_BYTES: u64 = 34;

/// Specification: is `ty` in the closed set of accepted tensor types?
pub open spec fn valid_tensor_type(ty: u32) -> bool {
    ty == GGML_TYPE_F32 || ty == GGML_TYPE_F16 || ty == GGML_TYPE_Q4_0 || ty == GGML_TYPE_Q8_0
}

/// Specification: exact byte size of a tensor of `n` elements and type `ty`
/// (only meaningful when `valid_tensor_type(ty)` and, for block types,
/// `n % QUANT_BLOCK_ELEMS == 0`).
pub open spec fn spec_tensor_byte_size(ty: u32, n: int) -> int {
    if ty == GGML_TYPE_F32 {
        n * 4
    } else if ty == GGML_TYPE_F16 {
        n * 2
    } else if ty == GGML_TYPE_Q4_0 {
        (n / QUANT_BLOCK_ELEMS as int) * Q4_0_BLOCK_BYTES as int
    } else {
        (n / QUANT_BLOCK_ELEMS as int) * Q8_0_BLOCK_BYTES as int
    }
}

// ============================================================================
// LITTLE-ENDIAN DECODE SPECIFICATIONS
// ============================================================================

/// Specification: little-endian u32 at byte offset `off`.
pub open spec fn spec_u32_le(buf: Seq<u8>, off: int) -> u32 {
    (buf[off] as u32)
        | ((buf[off + 1] as u32) << 8)
        | ((buf[off + 2] as u32) << 16)
        | ((buf[off + 3] as u32) << 24)
}

/// Specification: little-endian u64 at byte offset `off`.
pub open spec fn spec_u64_le(buf: Seq<u8>, off: int) -> u64 {
    (buf[off] as u64)
        | ((buf[off + 1] as u64) << 8)
        | ((buf[off + 2] as u64) << 16)
        | ((buf[off + 3] as u64) << 24)
        | ((buf[off + 4] as u64) << 32)
        | ((buf[off + 5] as u64) << 40)
        | ((buf[off + 6] as u64) << 48)
        | ((buf[off + 7] as u64) << 56)
}

// ============================================================================
// TOTAL READERS — no preconditions, cannot be misused
// ============================================================================

/// Read one byte, totally: `None` iff `off` is out of bounds.
pub fn try_u8(buf: &[u8], off: usize) -> (r: Option<u8>)
    ensures
        match r {
            Some(v) => off < buf@.len() && v == buf@[off as int],
            None => off >= buf@.len(),
        },
{
    if off < buf.len() {
        Some(*slice_index_get(buf, off))
    } else {
        None
    }
}

/// Read a little-endian u32, totally: `None` iff the 4 bytes don't fit.
pub fn try_u32_le(buf: &[u8], off: usize) -> (r: Option<u32>)
    ensures
        match r {
            Some(v) => off + 4 <= buf@.len() && v == spec_u32_le(buf@, off as int),
            None => off + 4 > buf@.len(),
        },
{
    if buf.len() >= 4 && off <= buf.len() - 4 {
        Some(
            (*slice_index_get(buf, off) as u32)
                | ((*slice_index_get(buf, off + 1) as u32) << 8)
                | ((*slice_index_get(buf, off + 2) as u32) << 16)
                | ((*slice_index_get(buf, off + 3) as u32) << 24),
        )
    } else {
        None
    }
}

/// Read a little-endian u64, totally: `None` iff the 8 bytes don't fit.
pub fn try_u64_le(buf: &[u8], off: usize) -> (r: Option<u64>)
    ensures
        match r {
            Some(v) => off + 8 <= buf@.len() && v == spec_u64_le(buf@, off as int),
            None => off + 8 > buf@.len(),
        },
{
    if buf.len() >= 8 && off <= buf.len() - 8 {
        Some(
            (*slice_index_get(buf, off) as u64)
                | ((*slice_index_get(buf, off + 1) as u64) << 8)
                | ((*slice_index_get(buf, off + 2) as u64) << 16)
                | ((*slice_index_get(buf, off + 3) as u64) << 24)
                | ((*slice_index_get(buf, off + 4) as u64) << 32)
                | ((*slice_index_get(buf, off + 5) as u64) << 40)
                | ((*slice_index_get(buf, off + 6) as u64) << 48)
                | ((*slice_index_get(buf, off + 7) as u64) << 56),
        )
    } else {
        None
    }
}

/// Take the subslice `[off, off + len)`, totally: `None` iff it doesn't fit.
pub fn try_subslice(buf: &[u8], off: usize, len: usize) -> (r: Option<&[u8]>)
    ensures
        match r {
            Some(s) => {
                &&& off + len <= buf@.len()
                &&& s@ == buf@.subrange(off as int, off as int + len as int)
                &&& s@.len() == len
            },
            None => off + len > buf@.len(),
        },
{
    if len <= buf.len() && off <= buf.len() - len {
        Some(slice_subrange(buf, off, off + len))
    } else {
        None
    }
}

// ============================================================================
// TOTAL SIZE ARITHMETIC — overflow is a rejection, never a wrap
// ============================================================================

/// Byte size of a tensor, totally: `None` on an unknown type, a block-size
/// violation, or arithmetic that would overflow u64. The returned size is
/// proven equal to the specification formula — the parser cannot be tricked
/// into a smaller-than-real buffer by crafted element counts.
pub fn tensor_byte_size(ty: u32, nelems: u64) -> (r: Option<u64>)
    ensures
        match r {
            Some(sz) => {
                &&& valid_tensor_type(ty)
                &&& sz == spec_tensor_byte_size(ty, nelems as int)
                &&& ((ty == GGML_TYPE_Q4_0 || ty == GGML_TYPE_Q8_0) ==> nelems as int
                    % QUANT_BLOCK_ELEMS as int == 0)
            },
            None => true,
        },
        !valid_tensor_type(ty) ==> matches!(r, None),
{
    if ty == GGML_TYPE_F32 {
        if nelems <= u64::MAX / 4 {
            Some(nelems * 4)
        } else {
            None
        }
    } else if ty == GGML_TYPE_F16 {
        if nelems <= u64::MAX / 2 {
            Some(nelems * 2)
        } else {
            None
        }
    } else if ty == GGML_TYPE_Q4_0 {
        if nelems % QUANT_BLOCK_ELEMS == 0 {
            let blocks = nelems / QUANT_BLOCK_ELEMS;
            if blocks <= u64::MAX / Q4_0_BLOCK_BYTES {
                Some(blocks * Q4_0_BLOCK_BYTES)
            } else {
                None
            }
        } else {
            None
        }
    } else if ty == GGML_TYPE_Q8_0 {
        if nelems % QUANT_BLOCK_ELEMS == 0 {
            let blocks = nelems / QUANT_BLOCK_ELEMS;
            if blocks <= u64::MAX / Q8_0_BLOCK_BYTES {
                Some(blocks * Q8_0_BLOCK_BYTES)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    }
}

/// Checked u64 multiply, totally: `None` iff the product overflows. The
/// product is computed in u128, where u64 × u64 cannot overflow.
pub fn try_mul_u64(a: u64, b: u64) -> (r: Option<u64>)
    ensures
        match r {
            Some(v) => v == a as int * b as int,
            None => a as int * b as int > u64::MAX as int,
        },
{
    proof {
        vstd::arithmetic::mul::lemma_mul_upper_bound(
            a as int,
            u64::MAX as int,
            b as int,
            u64::MAX as int,
        );
    }
    let wide = (a as u128) * (b as u128);
    if wide <= u64::MAX as u128 {
        Some(wide as u64)
    } else {
        None
    }
}

/// Checked u64 add, totally: `None` iff the sum overflows.
pub fn try_add_u64(a: u64, b: u64) -> (r: Option<u64>)
    ensures
        match r {
            Some(v) => v == a as int + b as int,
            None => a as int + b as int > u64::MAX,
        },
{
    if b <= u64::MAX - a {
        Some(a + b)
    } else {
        None
    }
}

/// Does the half-open range `[off, off + size)` fit inside a region of
/// `region_len` bytes? Total: the addition is performed in `int`, so no
/// crafted `off`/`size` pair can wrap its way inside.
pub fn region_fits(off: u64, size: u64, region_len: u64) -> (r: bool)
    ensures
        r <==> off as int + size as int <= region_len as int,
{
    off <= region_len && size <= region_len - off
}

/// Is `off` a multiple of `align`? Total: `align == 0` is `false`, not UB.
pub fn is_aligned(off: u64, align: u64) -> (r: bool)
    ensures
        r <==> (align != 0 && off as int % align as int == 0),
{
    align != 0 && off % align == 0
}

} // verus!

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_are_total() {
        let buf = [1u8, 2, 3, 4, 5, 6, 7, 8, 9];
        assert_eq!(try_u8(&buf, 0), Some(1));
        assert_eq!(try_u8(&buf, 8), Some(9));
        assert_eq!(try_u8(&buf, 9), None);
        assert_eq!(try_u8(&buf, usize::MAX), None);
        assert_eq!(try_u32_le(&buf, 0), Some(0x04030201));
        assert_eq!(try_u32_le(&buf, 5), Some(0x09080706));
        assert_eq!(try_u32_le(&buf, 6), None);
        assert_eq!(try_u32_le(&buf, usize::MAX), None);
        assert_eq!(try_u64_le(&buf, 1), Some(0x0908070605040302));
        assert_eq!(try_u64_le(&buf, 2), None);
        assert_eq!(try_u64_le(&[], 0), None);
        assert_eq!(try_subslice(&buf, 2, 3).unwrap(), &[3, 4, 5]);
        assert_eq!(try_subslice(&buf, 9, 0).unwrap(), &[]);
        assert!(try_subslice(&buf, 9, 1).is_none());
        assert!(try_subslice(&buf, usize::MAX, 2).is_none());
        assert!(try_subslice(&buf, 2, usize::MAX).is_none());
    }

    #[test]
    fn tensor_sizes() {
        assert_eq!(tensor_byte_size(GGML_TYPE_F32, 10), Some(40));
        assert_eq!(tensor_byte_size(GGML_TYPE_F16, 10), Some(20));
        assert_eq!(tensor_byte_size(GGML_TYPE_Q4_0, 64), Some(36));
        assert_eq!(tensor_byte_size(GGML_TYPE_Q8_0, 64), Some(68));
        // block misalignment rejected
        assert_eq!(tensor_byte_size(GGML_TYPE_Q4_0, 63), None);
        assert_eq!(tensor_byte_size(GGML_TYPE_Q8_0, 1), None);
        // unknown types rejected
        assert_eq!(tensor_byte_size(3, 64), None);
        assert_eq!(tensor_byte_size(u32::MAX, 64), None);
        // overflow rejected, not wrapped
        assert_eq!(tensor_byte_size(GGML_TYPE_F32, u64::MAX / 4 + 1), None);
        assert_eq!(
            tensor_byte_size(GGML_TYPE_F32, u64::MAX / 4),
            Some(u64::MAX / 4 * 4)
        );
    }

    #[test]
    fn checked_arithmetic() {
        assert_eq!(try_mul_u64(0, u64::MAX), Some(0));
        assert_eq!(try_mul_u64(u64::MAX, 1), Some(u64::MAX));
        assert_eq!(try_mul_u64(u64::MAX, 2), None);
        assert_eq!(try_mul_u64(1 << 32, 1 << 32), None);
        assert_eq!(try_add_u64(u64::MAX, 0), Some(u64::MAX));
        assert_eq!(try_add_u64(u64::MAX, 1), None);
        assert!(region_fits(0, 10, 10));
        assert!(!region_fits(1, 10, 10));
        assert!(!region_fits(u64::MAX, u64::MAX, u64::MAX));
        assert!(is_aligned(64, 32));
        assert!(!is_aligned(48, 32));
        assert!(!is_aligned(0, 0));
        assert!(is_aligned(0, 1));
    }
}
