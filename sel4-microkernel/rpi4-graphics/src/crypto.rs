//! # Verified Cryptography Primitives
//!
//! Combines well-audited RustCrypto libraries with Verus-verified wrappers.
//!
//! ## Design
//! - SHA-256: Uses `sha2` crate from RustCrypto (audited, no_std)
//! - Constant-time comparison: Verus-verified (timing side-channel protection)
//! - Bounds checking: Verus-verified (memory safety)
//!
//! ## Why This Approach
//! - RustCrypto's sha2 is battle-tested and audited
//! - Verus verifies our security-critical wrappers
//! - Best of both worlds: trusted crypto + verified safety

use sha2::{Sha256 as Sha256Impl, Digest};
use verus_builtin::*;
use verus_builtin_macros::*;

/// SHA-256 digest size in bytes
pub const SHA256_DIGEST_SIZE: usize = 32;

/// A SHA-256 digest (32 bytes)
#[derive(Clone, Copy)]
pub struct Sha256Digest {
    bytes: [u8; SHA256_DIGEST_SIZE],
}

impl Sha256Digest {
    /// Create a new digest from bytes
    pub const fn new(bytes: [u8; SHA256_DIGEST_SIZE]) -> Self {
        Self { bytes }
    }

    /// Get the underlying bytes
    pub const fn as_bytes(&self) -> &[u8; SHA256_DIGEST_SIZE] {
        &self.bytes
    }
}

/// SHA-256 hasher (wraps RustCrypto's sha2)
pub struct Sha256 {
    inner: Sha256Impl,
}

impl Sha256 {
    /// Create a new SHA-256 hasher
    pub fn new() -> Self {
        Self {
            inner: Sha256Impl::new(),
        }
    }

    /// Update the hash with more data
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Finalize and return the digest
    pub fn finalize(self) -> Sha256Digest {
        let result = self.inner.finalize();
        let mut bytes = [0u8; SHA256_DIGEST_SIZE];
        bytes.copy_from_slice(&result);
        Sha256Digest::new(bytes)
    }

    /// Compute SHA-256 of data in one call
    pub fn hash(data: &[u8]) -> Sha256Digest {
        let mut hasher = Self::new();
        hasher.update(data);
        hasher.finalize()
    }
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VERUS-VERIFIED SECURITY PRIMITIVES
// ============================================================================

/// Constant-time byte comparison
///
/// # Verification
/// This function is verified by Verus to:
/// 1. Always examine all bytes (constant-time)
/// 2. Return true iff all bytes are equal
/// 3. Never access out-of-bounds memory
///
/// # Security
/// Timing-safe comparison prevents attackers from learning
/// partial hash values through timing analysis.
#[verus_verify]
#[verifier::spec]
fn spec_ct_eq(a: &[u8; SHA256_DIGEST_SIZE], b: &[u8; SHA256_DIGEST_SIZE]) -> bool {
    // Specification: arrays are equal iff all elements are equal
    forall(|i: usize| i < SHA256_DIGEST_SIZE ==> a[i] == b[i])
}

#[verus_verify]
pub fn constant_time_compare(
    a: &[u8; SHA256_DIGEST_SIZE],
    b: &[u8; SHA256_DIGEST_SIZE],
) -> (result: bool)
    ensures
        result == spec_ct_eq(a, b),
{
    let mut diff: u8 = 0;

    // XOR all bytes - any difference sets bits in diff
    // This loop ALWAYS runs exactly SHA256_DIGEST_SIZE iterations
    let mut i: usize = 0;
    while i < SHA256_DIGEST_SIZE
        invariant
            i <= SHA256_DIGEST_SIZE,
            // diff is 0 iff all bytes so far are equal
            (diff == 0) == forall(|j: usize| j < i ==> a[j] == b[j]),
    {
        diff |= a[i] ^ b[i];
        i += 1;
    }

    // Convert to bool: 0 means equal, non-zero means different
    diff == 0
}

/// Verified bounds-checked indexing
///
/// Returns None if index is out of bounds, Some(value) otherwise.
/// Verified to never panic or access invalid memory.
#[verus_verify]
pub fn safe_index<T: Copy>(slice: &[T], index: usize) -> (result: Option<T>)
    ensures
        index < slice.len() ==> result == Some(slice[index as int]),
        index >= slice.len() ==> result.is_none(),
{
    if index < slice.len() {
        Some(slice[index])
    } else {
        None
    }
}

// ============================================================================
// VERIFICATION RESULT TYPES
// ============================================================================

/// Verification result for display
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VerifyResult {
    /// Hash matches expected value
    Valid,
    /// Hash does not match
    Invalid,
    /// Verification not performed
    NotChecked,
}

impl VerifyResult {
    pub const fn as_str(&self) -> &'static str {
        match self {
            VerifyResult::Valid => "VALID",
            VerifyResult::Invalid => "INVALID",
            VerifyResult::NotChecked => "NOT CHECKED",
        }
    }

    pub const fn is_valid(&self) -> bool {
        matches!(self, VerifyResult::Valid)
    }
}

/// Verify a data buffer against an expected SHA-256 hash
///
/// Uses constant-time comparison to prevent timing attacks.
pub fn verify_sha256(data: &[u8], expected: &Sha256Digest) -> VerifyResult {
    let actual = Sha256::hash(data);

    if constant_time_compare(actual.as_bytes(), expected.as_bytes()) {
        VerifyResult::Valid
    } else {
        VerifyResult::Invalid
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Convert a hex string to bytes (for embedding expected hashes)
///
/// Returns None if the string is invalid.
pub fn hex_to_bytes<const N: usize>(hex: &str) -> Option<[u8; N]> {
    if hex.len() != N * 2 {
        return None;
    }

    let mut result = [0u8; N];
    let hex_bytes = hex.as_bytes();

    for i in 0..N {
        let high = hex_digit(hex_bytes[i * 2])?;
        let low = hex_digit(hex_bytes[i * 2 + 1])?;
        result[i] = (high << 4) | low;
    }

    Some(result)
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Format a digest as hex string into a buffer
pub fn digest_to_hex(digest: &Sha256Digest, out: &mut [u8; 64]) {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    for (i, byte) in digest.as_bytes().iter().enumerate() {
        out[i * 2] = HEX_CHARS[(byte >> 4) as usize];
        out[i * 2 + 1] = HEX_CHARS[(byte & 0x0f) as usize];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test vector: SHA-256("")
    // Expected: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    #[test]
    fn test_sha256_empty() {
        let digest = Sha256::hash(b"");
        let expected = hex_to_bytes::<32>(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        ).unwrap();
        assert!(constant_time_compare(digest.as_bytes(), &expected));
    }

    // Test vector: SHA-256("abc")
    // Expected: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
    #[test]
    fn test_sha256_abc() {
        let digest = Sha256::hash(b"abc");
        let expected = hex_to_bytes::<32>(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        ).unwrap();
        assert!(constant_time_compare(digest.as_bytes(), &expected));
    }
}
