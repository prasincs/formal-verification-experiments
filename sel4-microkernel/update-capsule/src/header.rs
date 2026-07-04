//! IC-2 capsule header: fixed-offset, little-endian, totality-parsed.
//!
//! The fixed layout *is* the canonical serialization (workplan IC-2):
//!
//! ```text
//! 0x00  [u8;4]  magic = "SAOC"
//! 0x04  u32     format_version = 2
//! 0x08  u8      payload_type    (1 = pd-code, 2 = model-weights, 3 = config, 4 = wasm-tool)
//! 0x09  u8      target_slot     (slot PD id; 0 for whole-image)
//! 0x0A  u16     target_platform (1 = qemu-aarch64, 2 = rpi4, 3 = qemu-riscv64, ...)
//! 0x0C  u32     abi_version
//! 0x10  u64     monotonic_version (rollback epoch)
//! 0x18  u64     payload_len
//! 0x20  u64     load_vaddr      (0 = position-independent)
//! 0x28  u64     entry_offset
//! 0x30  u64     not_after       (MUST be 0 until a trusted time source exists)
//! 0x38  u32     signer_key_id
//! 0x3C  u32     key_epoch
//! 0x40  [u8;32] payload_sha256
//! 0x60  [u8;32] deps_sha256     (zero = none)
//! 0x80  [u8;64] ed25519 signature over bytes [0x00..0x80) ++ payload
//! 0xC0  ...     payload
//! ```
//!
//! This module is the crate's Verus surface: parsing is proven total (no
//! panic, no out-of-bounds read, no overflow, for *every* input slice) and
//! every parsed scalar is proven equal to its little-endian specification
//! decode. It is deliberately self-contained (constants included) so the
//! Verus harness can check it as a standalone crate root:
//!
//! ```text
//! verus --crate-type lib src/header.rs
//! ```

use verus_builtin_macros::verus;
// Used only by ghost code, which `cargo build` strips.
#[allow(unused_imports)]
use vstd::prelude::*;
use vstd::slice::{slice_index_get, slice_subrange};

verus! {

// ============================================================================
// CONSTANTS (normative, IC-2)
// ============================================================================

/// Header magic: "SAOC" (Secure Agent OS Capsule).
pub const MAGIC0: u8 = 0x53; // 'S'
pub const MAGIC1: u8 = 0x41; // 'A'
pub const MAGIC2: u8 = 0x4F; // 'O'
pub const MAGIC3: u8 = 0x43; // 'C'

/// The only format version this parser accepts.
pub const CAPSULE_FORMAT_VERSION: u32 = 2;

/// Total header length including the signature (payload starts here).
pub const HEADER_LEN: usize = 0xC0;

/// The signature covers bytes `[0x00..0x80)` of the header, then the payload.
pub const SIGNED_PREFIX_LEN: usize = 0x80;

/// Signature location within the header.
pub const SIGNATURE_OFFSET: usize = 0x80;
pub const SIGNATURE_LEN: usize = 64;

/// Digest field locations within the signed prefix.
pub const PAYLOAD_SHA256_OFFSET: usize = 0x40;
pub const DEPS_SHA256_OFFSET: usize = 0x60;
pub const SHA256_LEN: usize = 32;

/// Implementation limit on `payload_len` (`u32::MAX - HEADER_LEN`): the
/// verified HACL* hash/signature entry points take `u32` lengths, so the
/// whole capsule — and the signed message `prefix ++ payload` — must fit
/// in `u32`. Larger values are rejected during parsing, never trusted.
pub const PAYLOAD_LEN_MAX: u64 = 0xFFFF_FF3F;

/// Payload types (IC-2).
pub const PAYLOAD_TYPE_PD_CODE: u8 = 1;
pub const PAYLOAD_TYPE_MODEL_WEIGHTS: u8 = 2;
pub const PAYLOAD_TYPE_CONFIG: u8 = 3;
pub const PAYLOAD_TYPE_WASM_TOOL: u8 = 4;

/// Target platforms (IC-2). The value space is open-ended; only 0 is
/// structurally invalid. Matching against the *running* platform happens
/// in the verification pipeline.
pub const PLATFORM_QEMU_AARCH64: u16 = 1;
pub const PLATFORM_RPI4: u16 = 2;
pub const PLATFORM_QEMU_RISCV64: u16 = 3;

/// Specification: is a payload type known to format version 2?
pub open spec fn valid_payload_type(t: u8) -> bool {
    PAYLOAD_TYPE_PD_CODE <= t && t <= PAYLOAD_TYPE_WASM_TOOL
}

// ============================================================================
// PARSE ERRORS
// ============================================================================

/// Structural rejection reasons. Every malformed input maps to exactly one
/// of these — parsing is total: reject, never trust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Buffer shorter than the fixed 0xC0-byte header.
    TruncatedHeader,
    /// Magic is not "SAOC".
    BadMagic,
    /// `format_version` is not 2.
    UnsupportedVersion,
    /// `payload_type` is not one of the four defined types.
    BadPayloadType,
    /// `target_platform` is 0 (reserved).
    BadPlatform,
    /// `payload_len` exceeds [`PAYLOAD_LEN_MAX`].
    PayloadTooLarge,
    /// Declared payload extends past the end of the buffer.
    PayloadOutOfBounds,
    /// Buffer extends past the declared payload (unknown-byte smuggling).
    TrailingData,
}

// ============================================================================
// LITTLE-ENDIAN DECODE SPECIFICATIONS
// ============================================================================

/// Specification: little-endian u16 at byte offset `off`.
pub open spec fn spec_u16_le(buf: Seq<u8>, off: int) -> u16 {
    (buf[off] as u16) | ((buf[off + 1] as u16) << 8)
}

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
// VERIFIED LITTLE-ENDIAN READERS
// ============================================================================

/// Read one byte; in-bounds by precondition.
fn read_u8(buf: &[u8], off: usize) -> (v: u8)
    requires
        off < buf@.len(),
    ensures
        v == buf@[off as int],
{
    *slice_index_get(buf, off)
}

/// Read a little-endian u16; all accesses proven in-bounds.
fn read_u16_le(buf: &[u8], off: usize) -> (v: u16)
    requires
        off + 2 <= buf@.len(),
    ensures
        v == spec_u16_le(buf@, off as int),
{
    (*slice_index_get(buf, off) as u16) | ((*slice_index_get(buf, off + 1) as u16) << 8)
}

/// Read a little-endian u32; all accesses proven in-bounds.
fn read_u32_le(buf: &[u8], off: usize) -> (v: u32)
    requires
        off + 4 <= buf@.len(),
    ensures
        v == spec_u32_le(buf@, off as int),
{
    (*slice_index_get(buf, off) as u32)
        | ((*slice_index_get(buf, off + 1) as u32) << 8)
        | ((*slice_index_get(buf, off + 2) as u32) << 16)
        | ((*slice_index_get(buf, off + 3) as u32) << 24)
}

/// Read a little-endian u64; all accesses proven in-bounds.
fn read_u64_le(buf: &[u8], off: usize) -> (v: u64)
    requires
        off + 8 <= buf@.len(),
    ensures
        v == spec_u64_le(buf@, off as int),
{
    (*slice_index_get(buf, off) as u64)
        | ((*slice_index_get(buf, off + 1) as u64) << 8)
        | ((*slice_index_get(buf, off + 2) as u64) << 16)
        | ((*slice_index_get(buf, off + 3) as u64) << 24)
        | ((*slice_index_get(buf, off + 4) as u64) << 32)
        | ((*slice_index_get(buf, off + 5) as u64) << 40)
        | ((*slice_index_get(buf, off + 6) as u64) << 48)
        | ((*slice_index_get(buf, off + 7) as u64) << 56)
}

// ============================================================================
// CAPSULE HEADER
// ============================================================================

/// The scalar fields of a validated capsule header. Digest and signature
/// fields stay in the buffer and are exposed through the verified subslice
/// accessors below — the parser never copies untrusted bytes it has not
/// bounds-checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapsuleHeader {
    pub payload_type: u8,
    pub target_slot: u8,
    pub target_platform: u16,
    pub abi_version: u32,
    pub monotonic_version: u64,
    pub payload_len: u64,
    pub load_vaddr: u64,
    pub entry_offset: u64,
    pub not_after: u64,
    pub signer_key_id: u32,
    pub key_epoch: u32,
}

impl CapsuleHeader {
    /// Specification: every guarantee `parse` gives about a returned header
    /// with respect to the input buffer it was parsed from.
    pub open spec fn parsed_from(&self, buf: Seq<u8>) -> bool {
        &&& buf.len() == HEADER_LEN + self.payload_len
        &&& self.payload_len <= PAYLOAD_LEN_MAX
        &&& buf[0] == MAGIC0 && buf[1] == MAGIC1 && buf[2] == MAGIC2 && buf[3] == MAGIC3
        &&& spec_u32_le(buf, 0x04) == CAPSULE_FORMAT_VERSION
        &&& self.payload_type == buf[0x08]
        &&& valid_payload_type(self.payload_type)
        &&& self.target_slot == buf[0x09]
        &&& self.target_platform == spec_u16_le(buf, 0x0A)
        &&& self.target_platform != 0
        &&& self.abi_version == spec_u32_le(buf, 0x0C)
        &&& self.monotonic_version == spec_u64_le(buf, 0x10)
        &&& self.payload_len == spec_u64_le(buf, 0x18)
        &&& self.load_vaddr == spec_u64_le(buf, 0x20)
        &&& self.entry_offset == spec_u64_le(buf, 0x28)
        &&& self.not_after == spec_u64_le(buf, 0x30)
        &&& self.signer_key_id == spec_u32_le(buf, 0x38)
        &&& self.key_epoch == spec_u32_le(buf, 0x3C)
    }
}

/// Parse and structurally validate a capsule buffer.
///
/// Totality (proven): for **every** input slice this function returns
/// either a validated `CapsuleHeader` or a `ParseError` — it cannot
/// panic, read out of bounds, or overflow. The buffer must be exactly
/// `HEADER_LEN + payload_len` bytes: a shorter buffer is
/// `PayloadOutOfBounds`, a longer one is `TrailingData` (nothing may
/// ride along unvalidated).
pub fn parse(buf: &[u8]) -> (result: Result<CapsuleHeader, ParseError>)
    ensures
        match result {
            Ok(h) => h.parsed_from(buf@),
            Err(_) => true,
        },
{
    if buf.len() < HEADER_LEN {
        return Err(ParseError::TruncatedHeader);
    }

    // Magic
    if read_u8(buf, 0) != MAGIC0 || read_u8(buf, 1) != MAGIC1 || read_u8(buf, 2) != MAGIC2
        || read_u8(buf, 3) != MAGIC3 {
        return Err(ParseError::BadMagic);
    }

    // Format version
    if read_u32_le(buf, 0x04) != CAPSULE_FORMAT_VERSION {
        return Err(ParseError::UnsupportedVersion);
    }

    // Payload type: reject unknown values, never trust them.
    let payload_type = read_u8(buf, 0x08);
    if payload_type < PAYLOAD_TYPE_PD_CODE || payload_type > PAYLOAD_TYPE_WASM_TOOL {
        return Err(ParseError::BadPayloadType);
    }

    // Platform 0 is reserved.
    let target_platform = read_u16_le(buf, 0x0A);
    if target_platform == 0 {
        return Err(ParseError::BadPlatform);
    }

    // Payload length: bounds-check against the implementation limit and
    // the actual buffer before anything downstream may use it.
    let payload_len = read_u64_le(buf, 0x18);
    if payload_len > PAYLOAD_LEN_MAX {
        return Err(ParseError::PayloadTooLarge);
    }
    let avail: u64 = (buf.len() as u64) - (HEADER_LEN as u64);
    if payload_len > avail {
        return Err(ParseError::PayloadOutOfBounds);
    }
    if payload_len < avail {
        return Err(ParseError::TrailingData);
    }

    Ok(CapsuleHeader {
        payload_type,
        target_slot: read_u8(buf, 0x09),
        target_platform,
        abi_version: read_u32_le(buf, 0x0C),
        monotonic_version: read_u64_le(buf, 0x10),
        payload_len,
        load_vaddr: read_u64_le(buf, 0x20),
        entry_offset: read_u64_le(buf, 0x28),
        not_after: read_u64_le(buf, 0x30),
        signer_key_id: read_u32_le(buf, 0x38),
        key_epoch: read_u32_le(buf, 0x3C),
    })
}

// ============================================================================
// VERIFIED FIELD ACCESSORS (zero-copy)
// ============================================================================

/// The signed message prefix: header bytes `[0x00..0x80)`.
pub fn signed_prefix(capsule: &[u8]) -> (out: &[u8])
    requires
        capsule@.len() >= HEADER_LEN,
    ensures
        out@ == capsule@.subrange(0, SIGNED_PREFIX_LEN as int),
        out@.len() == SIGNED_PREFIX_LEN,
{
    slice_subrange(capsule, 0, SIGNED_PREFIX_LEN)
}

/// The claimed payload SHA-256 digest field (header bytes `[0x40..0x60)`).
pub fn payload_sha256_field(capsule: &[u8]) -> (out: &[u8])
    requires
        capsule@.len() >= HEADER_LEN,
    ensures
        out@ == capsule@.subrange(
            PAYLOAD_SHA256_OFFSET as int,
            PAYLOAD_SHA256_OFFSET as int + SHA256_LEN as int,
        ),
        out@.len() == SHA256_LEN,
{
    slice_subrange(capsule, PAYLOAD_SHA256_OFFSET, PAYLOAD_SHA256_OFFSET + SHA256_LEN)
}

/// The dependency-manifest digest field (header bytes `[0x60..0x80)`).
pub fn deps_sha256_field(capsule: &[u8]) -> (out: &[u8])
    requires
        capsule@.len() >= HEADER_LEN,
    ensures
        out@ == capsule@.subrange(
            DEPS_SHA256_OFFSET as int,
            DEPS_SHA256_OFFSET as int + SHA256_LEN as int,
        ),
        out@.len() == SHA256_LEN,
{
    slice_subrange(capsule, DEPS_SHA256_OFFSET, DEPS_SHA256_OFFSET + SHA256_LEN)
}

/// The ed25519 signature field (header bytes `[0x80..0xC0)`).
pub fn signature_field(capsule: &[u8]) -> (out: &[u8])
    requires
        capsule@.len() >= HEADER_LEN,
    ensures
        out@ == capsule@.subrange(
            SIGNATURE_OFFSET as int,
            SIGNATURE_OFFSET as int + SIGNATURE_LEN as int,
        ),
        out@.len() == SIGNATURE_LEN,
{
    slice_subrange(capsule, SIGNATURE_OFFSET, SIGNATURE_OFFSET + SIGNATURE_LEN)
}

/// The payload: everything after the header. For a buffer accepted by
/// [`parse`] this is exactly `payload_len` bytes.
pub fn payload_bytes(capsule: &[u8]) -> (out: &[u8])
    requires
        capsule@.len() >= HEADER_LEN,
    ensures
        out@ == capsule@.subrange(HEADER_LEN as int, capsule@.len() as int),
        out@.len() == capsule@.len() - HEADER_LEN,
{
    slice_subrange(capsule, HEADER_LEN, capsule.len())
}

} // verus!

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec::Vec;

    /// A structurally valid, unsigned capsule buffer for parser tests.
    fn minimal_capsule(payload_len: usize) -> Vec<u8> {
        let mut buf = std::vec![0u8; HEADER_LEN + payload_len];
        buf[0..4].copy_from_slice(b"SAOC");
        buf[0x04..0x08].copy_from_slice(&2u32.to_le_bytes());
        buf[0x08] = PAYLOAD_TYPE_PD_CODE;
        buf[0x09] = 3; // target_slot
        buf[0x0A..0x0C].copy_from_slice(&PLATFORM_QEMU_AARCH64.to_le_bytes());
        buf[0x0C..0x10].copy_from_slice(&1u32.to_le_bytes()); // abi
        buf[0x10..0x18].copy_from_slice(&5u64.to_le_bytes()); // monotonic
        buf[0x18..0x20].copy_from_slice(&(payload_len as u64).to_le_bytes());
        buf
    }

    #[test]
    fn accepts_minimal() {
        let buf = minimal_capsule(16);
        let h = parse(&buf).unwrap();
        assert_eq!(h.payload_type, PAYLOAD_TYPE_PD_CODE);
        assert_eq!(h.target_slot, 3);
        assert_eq!(h.target_platform, PLATFORM_QEMU_AARCH64);
        assert_eq!(h.abi_version, 1);
        assert_eq!(h.monotonic_version, 5);
        assert_eq!(h.payload_len, 16);
        assert_eq!(h.load_vaddr, 0);
        assert_eq!(h.entry_offset, 0);
        assert_eq!(h.not_after, 0);
    }

    #[test]
    fn rejects_short_buffers() {
        for n in 0..HEADER_LEN {
            let buf = std::vec![0u8; n];
            assert_eq!(parse(&buf), Err(ParseError::TruncatedHeader), "len {n}");
        }
    }

    #[test]
    fn rejects_bad_magic() {
        for i in 0..4 {
            let mut buf = minimal_capsule(0);
            buf[i] ^= 0xFF;
            assert_eq!(parse(&buf), Err(ParseError::BadMagic));
        }
    }

    #[test]
    fn rejects_bad_version() {
        for v in [0u32, 1, 3, u32::MAX] {
            let mut buf = minimal_capsule(0);
            buf[0x04..0x08].copy_from_slice(&v.to_le_bytes());
            assert_eq!(parse(&buf), Err(ParseError::UnsupportedVersion));
        }
    }

    #[test]
    fn rejects_bad_payload_type() {
        for t in [0u8, 5, 0xFF] {
            let mut buf = minimal_capsule(0);
            buf[0x08] = t;
            assert_eq!(parse(&buf), Err(ParseError::BadPayloadType));
        }
        for t in [1u8, 2, 3, 4] {
            let mut buf = minimal_capsule(0);
            buf[0x08] = t;
            assert!(parse(&buf).is_ok());
        }
    }

    #[test]
    fn rejects_platform_zero() {
        let mut buf = minimal_capsule(0);
        buf[0x0A..0x0C].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(parse(&buf), Err(ParseError::BadPlatform));
    }

    #[test]
    fn rejects_payload_len_lies() {
        // Declared longer than the buffer.
        let mut buf = minimal_capsule(8);
        buf[0x18..0x20].copy_from_slice(&9u64.to_le_bytes());
        assert_eq!(parse(&buf), Err(ParseError::PayloadOutOfBounds));

        // Declared shorter than the buffer (trailing smuggled bytes).
        let mut buf = minimal_capsule(8);
        buf[0x18..0x20].copy_from_slice(&7u64.to_le_bytes());
        assert_eq!(parse(&buf), Err(ParseError::TrailingData));

        // Absurd length: rejected by the implementation limit, not by
        // attempting arithmetic with it.
        let mut buf = minimal_capsule(8);
        buf[0x18..0x20].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(parse(&buf), Err(ParseError::PayloadTooLarge));
        let mut buf = minimal_capsule(8);
        buf[0x18..0x20].copy_from_slice(&(PAYLOAD_LEN_MAX + 1).to_le_bytes());
        assert_eq!(parse(&buf), Err(ParseError::PayloadTooLarge));
    }

    #[test]
    fn zero_length_payload_is_structurally_valid() {
        let buf = minimal_capsule(0);
        let h = parse(&buf).unwrap();
        assert_eq!(h.payload_len, 0);
        assert_eq!(payload_bytes(&buf).len(), 0);
    }

    #[test]
    fn accessors_slice_the_right_ranges() {
        let mut buf = minimal_capsule(4);
        buf[0x40..0x60].fill(0xAA);
        buf[0x60..0x80].fill(0xBB);
        buf[0x80..0xC0].fill(0xCC);
        buf[0xC0..].fill(0xDD);
        assert!(payload_sha256_field(&buf).iter().all(|&b| b == 0xAA));
        assert!(deps_sha256_field(&buf).iter().all(|&b| b == 0xBB));
        assert!(signature_field(&buf).iter().all(|&b| b == 0xCC));
        assert!(payload_bytes(&buf).iter().all(|&b| b == 0xDD));
        assert_eq!(signed_prefix(&buf).len(), SIGNED_PREFIX_LEN);
        assert_eq!(&signed_prefix(&buf)[0..4], b"SAOC");
    }
}
