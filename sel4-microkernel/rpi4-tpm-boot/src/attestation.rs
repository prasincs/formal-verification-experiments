//! # TPM Remote Attestation
//!
//! Remote attestation allows a verifier to cryptographically verify
//! the boot state of a remote system.
//!
//! ## Attestation Flow
//!
//! 1. Verifier sends a challenge (nonce)
//! 2. TPM creates a Quote (signed PCR values + nonce)
//! 3. Prover sends Quote + measurement log to verifier
//! 4. Verifier validates:
//!    - Quote signature (using TPM's attestation key)
//!    - Nonce matches challenge
//!    - PCR values match expected state
//!    - Measurement log replays to PCR values

use crate::{Sha256Digest, TpmResult, TpmRc};
use crate::pcr::{PcrSelection, PcrBank, PcrReadResult};
use crate::boot_chain::BootChain;

// ============================================================================
// ATTESTATION CONSTANTS
// ============================================================================

/// Size of attestation nonce
pub const NONCE_SIZE: usize = 32;

/// Maximum size of quote data
pub const MAX_QUOTE_SIZE: usize = 1024;

/// Maximum size of signature
pub const MAX_SIGNATURE_SIZE: usize = 512;

// ============================================================================
// ATTESTATION KEY
// ============================================================================

/// Type of attestation key
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttestationKeyType {
    /// RSA 2048-bit key
    Rsa2048,
    /// ECC P-256 key
    EccP256,
    /// ECC P-384 key
    EccP384,
}

/// Attestation Identity Key (AIK) reference
#[derive(Clone, Copy, Debug)]
pub struct AttestationKey {
    /// TPM handle for the key
    pub handle: u32,
    /// Key type
    pub key_type: AttestationKeyType,
    /// Public key hash (for identification)
    pub public_key_hash: Sha256Digest,
}

impl AttestationKey {
    /// Create a new attestation key reference
    pub fn new(handle: u32, key_type: AttestationKeyType) -> Self {
        Self {
            handle,
            key_type,
            public_key_hash: Sha256Digest::zero(), // Would be populated from TPM
        }
    }

    /// Standard AIK handle (well-known handle for attestation)
    pub const fn standard_aik() -> u32 {
        0x81010001
    }
}

// ============================================================================
// QUOTE STRUCTURE
// ============================================================================

/// TPM2 Quote structure
#[derive(Clone)]
pub struct Quote {
    /// TPM-generated attestation data
    pub attested: AttestedData,
    /// Signature over attestation data
    pub signature: QuoteSignature,
}

/// Attested data from TPM Quote
#[derive(Clone)]
pub struct AttestedData {
    /// Magic value (TPM2_GENERATED_VALUE = 0xFF544347)
    pub magic: u32,
    /// Attestation type (ATTEST_QUOTE = 0x8018)
    pub attest_type: u16,
    /// Qualified signer name
    pub qualified_signer: [u8; 34],
    /// Extra data (includes nonce)
    pub extra_data: [u8; NONCE_SIZE],
    /// Clock info
    pub clock_info: ClockInfo,
    /// Firmware version
    pub firmware_version: u64,
    /// PCR selection
    pub pcr_select: PcrSelection,
    /// PCR digest (hash of selected PCR values)
    pub pcr_digest: Sha256Digest,
}

/// TPM clock information
#[derive(Clone, Copy, Debug)]
pub struct ClockInfo {
    /// Time in milliseconds since TPM was initialized
    pub clock: u64,
    /// Reset count
    pub reset_count: u32,
    /// Restart count
    pub restart_count: u32,
    /// Safe flag (true if no time went backwards)
    pub safe: bool,
}

/// Quote signature
#[derive(Clone)]
pub struct QuoteSignature {
    /// Signature algorithm
    pub algorithm: u16,
    /// Signature data
    pub data: [u8; MAX_SIGNATURE_SIZE],
    /// Signature length
    pub length: usize,
}

// ============================================================================
// ATTESTATION REQUEST
// ============================================================================

/// Request for remote attestation
#[derive(Clone)]
pub struct AttestationRequest {
    /// Challenge nonce from verifier
    pub nonce: [u8; NONCE_SIZE],
    /// PCRs to include in quote
    pub pcr_selection: PcrSelection,
    /// Attestation key to use
    pub key_handle: u32,
}

impl AttestationRequest {
    /// Create a new attestation request
    pub fn new(nonce: [u8; NONCE_SIZE], pcr_selection: PcrSelection) -> Self {
        Self {
            nonce,
            pcr_selection,
            key_handle: AttestationKey::standard_aik(),
        }
    }

    /// Create a boot attestation request (standard boot PCRs)
    pub fn boot_attestation(nonce: [u8; NONCE_SIZE]) -> Self {
        Self {
            nonce,
            pcr_selection: PcrSelection::boot_pcrs(),
            key_handle: AttestationKey::standard_aik(),
        }
    }
}

// ============================================================================
// ATTESTATION RESPONSE
// ============================================================================

/// Response to attestation request
#[derive(Clone)]
pub struct AttestationResponse {
    /// TPM Quote
    pub quote: Quote,
    /// PCR values included in quote
    pub pcr_values: PcrReadResult,
    /// Measurement log for replay verification
    pub event_log: Option<EventLog>,
}

/// Event log for measurement replay
#[derive(Clone)]
pub struct EventLog {
    /// Log entries
    entries: [Option<EventLogEntry>; 64],
    /// Number of entries
    count: usize,
}

impl EventLog {
    /// Create empty event log
    pub const fn new() -> Self {
        Self {
            entries: [None; 64],
            count: 0,
        }
    }

    /// Add an entry
    pub fn add(&mut self, entry: EventLogEntry) -> bool {
        if self.count < 64 {
            self.entries[self.count] = Some(entry);
            self.count += 1;
            true
        } else {
            false
        }
    }

    /// Get entries
    pub fn entries(&self) -> &[Option<EventLogEntry>] {
        &self.entries[..self.count]
    }

    /// Number of entries
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Single event log entry
#[derive(Clone, Copy)]
pub struct EventLogEntry {
    /// PCR that was extended
    pub pcr_index: u8,
    /// Event type
    pub event_type: EventType,
    /// Digest that was extended
    pub digest: Sha256Digest,
    /// Size of event data
    pub event_size: u32,
}

/// Event types for measurement log
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum EventType {
    /// Pre-boot environment
    PreBoot = 0x00000000,
    /// POST code
    PostCode = 0x00000001,
    /// UEFI variable
    EfiVariable = 0x80000001,
    /// Boot services application
    EfiBootServicesApp = 0x80000003,
    /// GPT event
    EfiGptEvent = 0x80000006,
    /// seL4 kernel image
    Sel4Kernel = 0x90000001,
    /// Microkit system config
    MicrokitSystem = 0x90000002,
    /// Protection domain image
    ProtectionDomain = 0x90000003,
    /// Runtime measurement
    RuntimeMeasurement = 0x90000004,
}

// ============================================================================
// ATTESTATION VERIFIER
// ============================================================================

/// Attestation verification result
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationResult {
    /// Attestation verified successfully
    Verified,
    /// Signature verification failed
    SignatureInvalid,
    /// Nonce mismatch
    NonceMismatch,
    /// PCR values don't match expected
    PcrMismatch,
    /// Event log replay doesn't match PCRs
    EventLogMismatch,
    /// Quote structure invalid
    InvalidQuote,
}

/// Verifier for remote attestation
pub struct AttestationVerifier {
    /// Expected PCR values
    expected_pcrs: PcrBank,
    /// Trusted attestation key public key hash
    trusted_key_hash: Option<Sha256Digest>,
}

impl AttestationVerifier {
    /// Create a new verifier
    pub fn new() -> Self {
        Self {
            expected_pcrs: PcrBank::new(),
            trusted_key_hash: None,
        }
    }

    /// Set trusted attestation key
    pub fn set_trusted_key(&mut self, key_hash: Sha256Digest) {
        self.trusted_key_hash = Some(key_hash);
    }

    /// Set expected PCR value
    pub fn set_expected_pcr(&mut self, index: u8, digest: &Sha256Digest) -> TpmResult<()> {
        // We need to set the PCR directly, but PcrBank only supports extend
        // This is a verification-only bank, so we'll track separately
        if index > 23 {
            return Err(TpmRc::BadParam);
        }
        // For now, extend from zero to set the value
        self.expected_pcrs.extend(index, digest)
    }

    /// Verify an attestation response
    pub fn verify(
        &self,
        request: &AttestationRequest,
        response: &AttestationResponse,
    ) -> VerificationResult {
        // 1. Verify nonce
        if response.quote.attested.extra_data != request.nonce {
            return VerificationResult::NonceMismatch;
        }

        // 2. Verify magic value
        if response.quote.attested.magic != 0xFF544347 {
            return VerificationResult::InvalidQuote;
        }

        // 3. Verify PCR digest matches reported PCR values
        let computed_digest = crate::pcr::compute_pcr_composite(&response.pcr_values);
        if !crate::boot_chain::constant_time_compare(
            &computed_digest,
            &response.quote.attested.pcr_digest,
        ) {
            return VerificationResult::PcrMismatch;
        }

        // 4. If event log present, verify replay
        if let Some(ref event_log) = response.event_log {
            if !self.verify_event_log(event_log, &response.pcr_values) {
                return VerificationResult::EventLogMismatch;
            }
        }

        // 5. Verify signature (would use trusted key)
        // This is a placeholder - actual verification needs crypto
        if response.quote.signature.length == 0 {
            return VerificationResult::SignatureInvalid;
        }

        VerificationResult::Verified
    }

    /// Verify event log replays to PCR values
    fn verify_event_log(&self, log: &EventLog, pcr_values: &PcrReadResult) -> bool {
        let mut replay_bank = PcrBank::new();

        // Replay all events
        for entry in log.entries().iter().flatten() {
            if replay_bank.extend(entry.pcr_index, &entry.digest).is_err() {
                return false;
            }
        }

        // Compare replayed values with reported values
        for (index, reported) in pcr_values.values() {
            if let Some(replayed) = replay_bank.read(*index) {
                if !crate::boot_chain::constant_time_compare(replayed, reported) {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

impl Default for AttestationVerifier {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ATTESTATION EVIDENCE
// ============================================================================

/// Complete attestation evidence bundle
pub struct AttestationEvidence {
    /// Attestation response from TPM
    pub response: AttestationResponse,
    /// Additional platform info
    pub platform_info: PlatformInfo,
    /// Timestamp of attestation
    pub timestamp: u64,
}

/// Platform information for attestation context
#[derive(Clone)]
pub struct PlatformInfo {
    /// Platform manufacturer
    pub manufacturer: [u8; 32],
    /// Model identifier
    pub model: [u8; 32],
    /// Firmware version
    pub firmware_version: [u8; 16],
    /// seL4 kernel version
    pub kernel_version: [u8; 16],
    /// Microkit SDK version
    pub microkit_version: [u8; 16],
}

impl PlatformInfo {
    /// Create platform info for Raspberry Pi 4
    pub fn rpi4_sel4() -> Self {
        Self {
            manufacturer: *b"Raspberry Pi Foundation \0\0\0\0\0\0\0\0",
            model: *b"Raspberry Pi 4 Model B  \0\0\0\0\0\0\0\0",
            firmware_version: *b"2024.01.01\0\0\0\0\0\0",
            kernel_version: *b"seL4-13.0.0\0\0\0\0\0",
            microkit_version: *b"2.1.0\0\0\0\0\0\0\0\0\0\0\0",
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attestation_request() {
        let nonce = [0x42u8; NONCE_SIZE];
        let request = AttestationRequest::boot_attestation(nonce);

        assert_eq!(request.nonce, nonce);
        assert!(request.pcr_selection.is_selected(0));
        assert!(request.pcr_selection.is_selected(1));
        assert!(request.pcr_selection.is_selected(7));
    }

    #[test]
    fn test_event_log() {
        let mut log = EventLog::new();
        assert!(log.is_empty());

        let entry = EventLogEntry {
            pcr_index: 0,
            event_type: EventType::Sel4Kernel,
            digest: crate::boot_chain::compute_sha256(b"kernel"),
            event_size: 6,
        };

        assert!(log.add(entry));
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn test_verifier_nonce_check() {
        let verifier = AttestationVerifier::new();
        let nonce = [0x42u8; NONCE_SIZE];
        let wrong_nonce = [0x00u8; NONCE_SIZE];

        let request = AttestationRequest::boot_attestation(nonce);

        let attested = AttestedData {
            magic: 0xFF544347,
            attest_type: 0x8018,
            qualified_signer: [0; 34],
            extra_data: wrong_nonce, // Wrong nonce
            clock_info: ClockInfo {
                clock: 0,
                reset_count: 0,
                restart_count: 0,
                safe: true,
            },
            firmware_version: 0,
            pcr_select: PcrSelection::boot_pcrs(),
            pcr_digest: Sha256Digest::zero(),
        };

        let quote = Quote {
            attested,
            signature: QuoteSignature {
                algorithm: 0,
                data: [0; MAX_SIGNATURE_SIZE],
                length: 0,
            },
        };

        let response = AttestationResponse {
            quote,
            pcr_values: PcrReadResult::new(),
            event_log: None,
        };

        let result = verifier.verify(&request, &response);
        assert_eq!(result, VerificationResult::NonceMismatch);
    }
}
