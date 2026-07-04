//! Crypto backend seam.
//!
//! Per the workplan (WP-8), the crate consumes *formally verified* crypto
//! rather than hand-rolling any: SHA-256 and ed25519 come from libcrux's
//! HACL* extractions (`libcrux-sha2`, `libcrux-ed25519`), which build
//! `no_std` on the pinned nightly. If they ever stop building, swap the
//! two thin wrappers below for another implementation (e.g.
//! `sha2`/`ed25519-dalek` with `default-features = false`) and document
//! the change in the README — nothing outside this module names the
//! backend.
//!
//! Length caveat: the HACL* entry points take `u32` lengths. The parser
//! enforces `payload_len <= PAYLOAD_LEN_MAX` (= `u32::MAX - HEADER_LEN`)
//! before any of these functions run, so the casts below cannot truncate
//! for buffers that passed parsing.

/// SHA-256 of `data`. `data.len()` must fit in `u32` (guaranteed for
/// parsed capsules; see module docs).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    libcrux_sha2::sha256(data)
}

/// Verify an ed25519 signature over `msg`.
pub fn ed25519_verify(msg: &[u8], public_key: &[u8; 32], signature: &[u8; 64]) -> bool {
    libcrux_ed25519::verify(msg, public_key, signature).is_ok()
}

/// Constant-time equality for digests. Always inspects every byte — no
/// early exit — so the comparison leaks nothing about *where* two
/// digests differ. Lengths are public; a length mismatch returns early.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc: u8 = 0;
    for i in 0..a.len() {
        acc |= a[i] ^ b[i];
    }
    // black_box keeps the compiler from turning the fold back into a
    // short-circuiting compare.
    core::hint::black_box(acc) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_answer() {
        // SHA-256("abc") — FIPS 180-2 test vector.
        let digest = sha256(b"abc");
        let expected: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        assert_eq!(digest, expected);
    }

    #[test]
    fn ct_eq_behaves() {
        let a = [0u8; 32];
        let mut b = [0u8; 32];
        assert!(ct_eq(&a, &b));
        b[31] = 1;
        assert!(!ct_eq(&a, &b));
        assert!(!ct_eq(&a[..16], &b));
        assert!(ct_eq(&[], &[]));
    }
}
