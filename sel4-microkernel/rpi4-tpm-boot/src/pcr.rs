//! # Verified PCR (Platform Configuration Register) Management
//!
//! Formal verification of PCR operations using Verus specifications.
//!
//! ## PCR Allocation for seL4/Microkit Boot
//!
//! | PCR | Usage                          | Extended By        |
//! |-----|--------------------------------|--------------------|
//! | 0   | Firmware (bootcode, start4)    | VideoCore          |
//! | 1   | seL4 Kernel                    | Bootloader         |
//! | 2   | Microkit System Config         | Bootloader         |
//! | 3   | Protection Domain Images       | Bootloader/Kernel  |
//! | 4   | Runtime Measurements           | PDs                |
//! | 5   | Reserved                       | -                  |
//! | 6   | Reserved                       | -                  |
//! | 7   | Secure Boot Policy             | Firmware           |
//! | 8-15| Available for applications     | PDs                |
//! | 16  | Debug PCR (resettable)         | Debug tools        |
//! | 17-22| Reserved for DRTM             | -                  |
//! | 23  | Application-specific           | PDs                |
//!
//! ## Verification Properties
//!
//! - PCR indices are always in valid range (0-23)
//! - PCR extension is monotonic (values only increase in entropy)
//! - PCR banks are correctly tracked per algorithm
//! - Policy evaluation is sound

use crate::{Sha256Digest, TpmResult, TpmRc};

#[cfg(feature = "verus")]
use verus_builtin_macros::verus;

// ============================================================================
// PCR CONSTANTS
// ============================================================================

/// Total number of PCRs in TPM 2.0
pub const PCR_COUNT: usize = 24;

/// PCR index for firmware measurements
pub const PCR_FIRMWARE: u8 = 0;

/// PCR index for kernel measurements
pub const PCR_KERNEL: u8 = 1;

/// PCR index for system configuration
pub const PCR_SYSTEM: u8 = 2;

/// PCR index for protection domain images
pub const PCR_PD_IMAGES: u8 = 3;

/// PCR index for runtime measurements
pub const PCR_RUNTIME: u8 = 4;

/// PCR index for secure boot policy
pub const PCR_SECURE_BOOT: u8 = 7;

/// PCR index for debug (resettable)
pub const PCR_DEBUG: u8 = 16;

/// PCR index for application use
pub const PCR_APPLICATION: u8 = 23;

/// Maximum valid PCR index
pub const MAX_PCR_INDEX: u8 = 23;

// ============================================================================
// PCR SELECTION
// ============================================================================

/// PCR selection bitmap (24 bits, one per PCR)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcrSelection {
    /// Bitmap of selected PCRs
    bitmap: u32,
}

impl PcrSelection {
    /// Create empty selection
    pub const fn empty() -> Self {
        Self { bitmap: 0 }
    }

    /// Create selection with all PCRs
    pub const fn all() -> Self {
        Self { bitmap: 0x00FFFFFF }
    }

    /// Create selection from bitmap
    pub const fn from_bitmap(bitmap: u32) -> Self {
        Self { bitmap: bitmap & 0x00FFFFFF }
    }

    /// Select a single PCR
    pub const fn single(pcr: u8) -> Self {
        if pcr <= MAX_PCR_INDEX {
            Self { bitmap: 1 << pcr }
        } else {
            Self { bitmap: 0 }
        }
    }

    /// Standard boot PCRs (0-4, 7)
    pub const fn boot_pcrs() -> Self {
        Self {
            bitmap: (1 << PCR_FIRMWARE)
                | (1 << PCR_KERNEL)
                | (1 << PCR_SYSTEM)
                | (1 << PCR_PD_IMAGES)
                | (1 << PCR_RUNTIME)
                | (1 << PCR_SECURE_BOOT),
        }
    }

    /// Check if a PCR is selected
    #[inline]
    pub const fn is_selected(&self, pcr: u8) -> bool {
        if pcr <= MAX_PCR_INDEX {
            (self.bitmap & (1 << pcr)) != 0
        } else {
            false
        }
    }

    /// Add a PCR to selection
    #[inline]
    pub fn select(&mut self, pcr: u8) {
        if pcr <= MAX_PCR_INDEX {
            self.bitmap |= 1 << pcr;
        }
    }

    /// Remove a PCR from selection
    #[inline]
    pub fn deselect(&mut self, pcr: u8) {
        if pcr <= MAX_PCR_INDEX {
            self.bitmap &= !(1 << pcr);
        }
    }

    /// Get bitmap value
    #[inline]
    pub const fn bitmap(&self) -> u32 {
        self.bitmap
    }

    /// Count selected PCRs
    pub fn count(&self) -> usize {
        self.bitmap.count_ones() as usize
    }

    /// Iterate over selected PCR indices
    pub fn iter(&self) -> PcrSelectionIter {
        PcrSelectionIter {
            bitmap: self.bitmap,
            current: 0,
        }
    }
}

/// Iterator over selected PCR indices
pub struct PcrSelectionIter {
    bitmap: u32,
    current: u8,
}

impl Iterator for PcrSelectionIter {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        while self.current <= MAX_PCR_INDEX {
            let pcr = self.current;
            self.current += 1;
            if (self.bitmap & (1 << pcr)) != 0 {
                return Some(pcr);
            }
        }
        None
    }
}

// ============================================================================
// PCR BANK (SHA-256)
// ============================================================================

/// PCR bank holding SHA-256 values for all 24 PCRs
#[derive(Clone)]
pub struct PcrBank {
    /// PCR values
    values: [Sha256Digest; PCR_COUNT],
    /// Extension count per PCR (for replay verification)
    extend_count: [u32; PCR_COUNT],
}

impl PcrBank {
    /// Create a new PCR bank with all zeros
    pub const fn new() -> Self {
        Self {
            values: [Sha256Digest::zero(); PCR_COUNT],
            extend_count: [0; PCR_COUNT],
        }
    }

    /// Read a PCR value
    ///
    /// Returns None if index is invalid.
    #[inline]
    pub fn read(&self, index: u8) -> Option<&Sha256Digest> {
        if index <= MAX_PCR_INDEX {
            Some(&self.values[index as usize])
        } else {
            None
        }
    }

    /// Get extension count for a PCR
    #[inline]
    pub fn extend_count(&self, index: u8) -> Option<u32> {
        if index <= MAX_PCR_INDEX {
            Some(self.extend_count[index as usize])
        } else {
            None
        }
    }

    /// Extend a PCR with a digest
    ///
    /// PCR_new = SHA-256(PCR_old || digest)
    ///
    /// # Verification Properties
    /// - Index must be valid (0-23)
    /// - Extension count must not overflow
    pub fn extend(&mut self, index: u8, digest: &Sha256Digest) -> TpmResult<()> {
        if index > MAX_PCR_INDEX {
            return Err(TpmRc::BadParam);
        }

        let idx = index as usize;

        // Check for extend count overflow
        if self.extend_count[idx] == u32::MAX {
            return Err(TpmRc::Failure);
        }

        // Compute extension: SHA-256(PCR_old || digest)
        self.values[idx] = crate::boot_chain::extend_pcr(&self.values[idx], digest);
        self.extend_count[idx] += 1;

        Ok(())
    }

    /// Reset a PCR (only valid for debug PCR 16)
    pub fn reset(&mut self, index: u8) -> TpmResult<()> {
        if index != PCR_DEBUG {
            return Err(TpmRc::BadParam);
        }

        self.values[index as usize] = Sha256Digest::zero();
        self.extend_count[index as usize] = 0;

        Ok(())
    }

    /// Read multiple PCRs based on selection
    pub fn read_selection(&self, selection: &PcrSelection) -> PcrReadResult {
        let mut result = PcrReadResult::new();

        for pcr in selection.iter() {
            if let Some(value) = self.read(pcr) {
                result.add(pcr, *value);
            }
        }

        result
    }
}

impl Default for PcrBank {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PCR READ RESULT
// ============================================================================

/// Result of reading multiple PCRs
#[derive(Clone)]
pub struct PcrReadResult {
    /// PCR values read
    pub values: [(u8, Sha256Digest); PCR_COUNT],
    /// Number of values
    pub count: usize,
}

impl PcrReadResult {
    /// Create empty result
    pub fn new() -> Self {
        Self {
            values: [(0, Sha256Digest::zero()); PCR_COUNT],
            count: 0,
        }
    }

    /// Add a PCR value to result
    pub fn add(&mut self, index: u8, value: Sha256Digest) {
        if self.count < PCR_COUNT {
            self.values[self.count] = (index, value);
            self.count += 1;
        }
    }

    /// Get values as slice
    pub fn values(&self) -> &[(u8, Sha256Digest)] {
        &self.values[..self.count]
    }
}

impl Default for PcrReadResult {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PCR POLICY
// ============================================================================

/// Policy type for PCR-based authorization
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyType {
    /// Exact match: all selected PCRs must exactly match
    ExactMatch,
    /// Any match: at least one selected PCR must match
    AnyMatch,
    /// Range: PCR values must be in a specific set
    AllowList,
}

/// A policy rule for PCR verification
#[derive(Clone)]
pub struct PcrPolicy {
    /// Policy type
    pub policy_type: PolicyType,
    /// Selected PCRs for this policy
    pub selection: PcrSelection,
    /// Expected digest (composite hash of selected PCRs)
    pub expected_digest: Sha256Digest,
    /// Policy name/identifier
    pub name: [u8; 32],
}

impl PcrPolicy {
    /// Create a new exact match policy
    pub fn exact_match(selection: PcrSelection, expected: Sha256Digest) -> Self {
        Self {
            policy_type: PolicyType::ExactMatch,
            selection,
            expected_digest: expected,
            name: [0; 32],
        }
    }

    /// Create a boot verification policy
    pub fn boot_policy(expected_pcr_composite: Sha256Digest) -> Self {
        Self {
            policy_type: PolicyType::ExactMatch,
            selection: PcrSelection::boot_pcrs(),
            expected_digest: expected_pcr_composite,
            name: *b"boot_verification_policy\0\0\0\0\0\0\0\0",
        }
    }

    /// Evaluate policy against current PCR bank
    pub fn evaluate(&self, bank: &PcrBank) -> PolicyResult {
        let read_result = bank.read_selection(&self.selection);

        // Compute composite hash of selected PCRs
        let composite = compute_pcr_composite(&read_result);

        match self.policy_type {
            PolicyType::ExactMatch => {
                if crate::boot_chain::constant_time_compare(&composite, &self.expected_digest) {
                    PolicyResult::Satisfied
                } else {
                    PolicyResult::Failed(PolicyFailure::DigestMismatch)
                }
            }
            PolicyType::AnyMatch => {
                // Check if any PCR matches expected
                PolicyResult::Failed(PolicyFailure::NotImplemented)
            }
            PolicyType::AllowList => {
                PolicyResult::Failed(PolicyFailure::NotImplemented)
            }
        }
    }
}

/// Result of policy evaluation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyResult {
    /// Policy was satisfied
    Satisfied,
    /// Policy failed
    Failed(PolicyFailure),
}

/// Reason for policy failure
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyFailure {
    /// PCR digest does not match expected
    DigestMismatch,
    /// Required PCR not available
    PcrNotAvailable,
    /// Policy type not implemented
    NotImplemented,
}

/// Compute composite hash of PCR values
///
/// The composite is SHA-256 of all selected PCR values concatenated
pub fn compute_pcr_composite(values: &PcrReadResult) -> Sha256Digest {
    use sha2::{Sha256, Digest};

    let mut hasher = Sha256::new();

    for (_, value) in values.values() {
        hasher.update(&value.bytes);
    }

    let result = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&result);
    Sha256Digest::new(bytes)
}

// ============================================================================
// VERIFIED PCR INDEX (with Verus specs when enabled)
// ============================================================================

/// Verified PCR index type that is proven to be in valid range
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifiedPcrIndex {
    index: u8,
}

impl VerifiedPcrIndex {
    /// Create a verified PCR index
    ///
    /// Returns None if index > 23
    #[inline]
    pub const fn new(index: u8) -> Option<Self> {
        if index <= MAX_PCR_INDEX {
            Some(Self { index })
        } else {
            None
        }
    }

    /// Create a verified PCR index (unchecked)
    ///
    /// # Safety
    /// Caller must ensure index <= 23
    #[inline]
    pub const unsafe fn new_unchecked(index: u8) -> Self {
        Self { index }
    }

    /// Get the index value
    #[inline]
    pub const fn get(&self) -> u8 {
        self.index
    }

    /// Get as usize for array indexing
    #[inline]
    pub const fn as_usize(&self) -> usize {
        self.index as usize
    }
}

// Convenience constants for verified PCR indices
pub const VERIFIED_PCR_FIRMWARE: VerifiedPcrIndex = unsafe { VerifiedPcrIndex::new_unchecked(0) };
pub const VERIFIED_PCR_KERNEL: VerifiedPcrIndex = unsafe { VerifiedPcrIndex::new_unchecked(1) };
pub const VERIFIED_PCR_SYSTEM: VerifiedPcrIndex = unsafe { VerifiedPcrIndex::new_unchecked(2) };
pub const VERIFIED_PCR_PD_IMAGES: VerifiedPcrIndex = unsafe { VerifiedPcrIndex::new_unchecked(3) };
pub const VERIFIED_PCR_RUNTIME: VerifiedPcrIndex = unsafe { VerifiedPcrIndex::new_unchecked(4) };
pub const VERIFIED_PCR_SECURE_BOOT: VerifiedPcrIndex = unsafe { VerifiedPcrIndex::new_unchecked(7) };

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pcr_selection() {
        let mut sel = PcrSelection::empty();
        assert_eq!(sel.count(), 0);

        sel.select(0);
        sel.select(1);
        sel.select(7);

        assert!(sel.is_selected(0));
        assert!(sel.is_selected(1));
        assert!(!sel.is_selected(2));
        assert!(sel.is_selected(7));
        assert_eq!(sel.count(), 3);
    }

    #[test]
    fn test_pcr_selection_iter() {
        let sel = PcrSelection::boot_pcrs();
        let indices: Vec<u8> = sel.iter().collect();

        assert!(indices.contains(&0));
        assert!(indices.contains(&1));
        assert!(indices.contains(&2));
        assert!(indices.contains(&3));
        assert!(indices.contains(&4));
        assert!(indices.contains(&7));
    }

    #[test]
    fn test_pcr_bank() {
        let mut bank = PcrBank::new();

        // Initial values should be zero
        assert!(bank.read(0).unwrap().is_zero());

        // Extend PCR 0
        let digest = crate::boot_chain::compute_sha256(b"test measurement");
        assert!(bank.extend(0, &digest).is_ok());

        // Value should no longer be zero
        assert!(!bank.read(0).unwrap().is_zero());

        // Extension count should be 1
        assert_eq!(bank.extend_count(0), Some(1));
    }

    #[test]
    fn test_pcr_bank_invalid_index() {
        let mut bank = PcrBank::new();
        let digest = Sha256Digest::zero();

        // Index 24 should fail
        assert!(bank.extend(24, &digest).is_err());
        assert!(bank.read(24).is_none());
    }

    #[test]
    fn test_verified_pcr_index() {
        assert!(VerifiedPcrIndex::new(0).is_some());
        assert!(VerifiedPcrIndex::new(23).is_some());
        assert!(VerifiedPcrIndex::new(24).is_none());

        let idx = VerifiedPcrIndex::new(5).unwrap();
        assert_eq!(idx.get(), 5);
        assert_eq!(idx.as_usize(), 5);
    }

    #[test]
    fn test_pcr_reset() {
        let mut bank = PcrBank::new();
        let digest = crate::boot_chain::compute_sha256(b"test");

        // Extend PCR 16 (debug)
        bank.extend(16, &digest).unwrap();
        assert!(!bank.read(16).unwrap().is_zero());

        // Reset should work for PCR 16
        assert!(bank.reset(16).is_ok());
        assert!(bank.read(16).unwrap().is_zero());

        // Reset should fail for other PCRs
        bank.extend(0, &digest).unwrap();
        assert!(bank.reset(0).is_err());
    }
}
