//! Single-field corruption matrix (WP-8 acceptance): every corrupted
//! field is rejected with its own distinct error, and field checks that
//! precede the signature check in the IC-2 normative order surface
//! *before* `BadSignature` even though the corruption also breaks the
//! signature.

#![cfg(feature = "mint")]

mod common;

use common::*;
use update_capsule::header::ParseError;
use update_capsule::mint::mint;
use update_capsule::verify::{verify_capsule, VerifyError};

/// Verify a (possibly corrupted) capsule against the golden profile.
fn verify(capsule: &[u8]) -> Result<update_capsule::InstallAuthorization, VerifyError> {
    let fixture = Fixture::new();
    let mut scratch = scratch_for(capsule);
    verify_capsule(
        capsule,
        &fixture.profile(),
        &Counter(Some(GOLDEN_VERSION)),
        1,
        &mut scratch,
    )
}

/// The golden capsule with one byte XOR-flipped at `offset`.
fn flipped(offset: usize) -> Vec<u8> {
    let mut c = mint_golden();
    c[offset] ^= 0xFF;
    c
}

#[test]
fn baseline_is_accepted() {
    assert!(verify(&mint_golden()).is_ok());
}

// --- structural fields (rejected by the verified parser) -----------------

#[test]
fn corrupt_magic() {
    assert_eq!(
        verify(&flipped(0x00)),
        Err(VerifyError::Parse(ParseError::BadMagic))
    );
}

#[test]
fn corrupt_version() {
    assert_eq!(
        verify(&flipped(0x04)),
        Err(VerifyError::Parse(ParseError::UnsupportedVersion))
    );
}

#[test]
fn corrupt_payload_type() {
    assert_eq!(
        verify(&flipped(0x08)),
        Err(VerifyError::Parse(ParseError::BadPayloadType))
    );
}

#[test]
fn corrupt_payload_len() {
    // Bigger than the buffer.
    assert_eq!(
        verify(&flipped(0x18)),
        Err(VerifyError::Parse(ParseError::PayloadOutOfBounds))
    );
    // High byte set: over the u32 implementation limit.
    assert_eq!(
        verify(&flipped(0x1F)),
        Err(VerifyError::Parse(ParseError::PayloadTooLarge))
    );
    // Smaller than the buffer: smuggled trailing bytes.
    let mut c = mint_golden();
    c[0x18..0x20].copy_from_slice(&((GOLDEN_PAYLOAD.len() - 1) as u64).to_le_bytes());
    assert_eq!(
        verify(&c),
        Err(VerifyError::Parse(ParseError::TrailingData))
    );
    // Truncated buffer, header intact: same lie, other direction.
    let mut c = mint_golden();
    c.truncate(c.len() - 1);
    assert_eq!(
        verify(&c),
        Err(VerifyError::Parse(ParseError::PayloadOutOfBounds))
    );
}

// --- system-binding fields (rejected before any crypto) ------------------

#[test]
fn corrupt_platform() {
    assert_eq!(verify(&flipped(0x0A)), Err(VerifyError::PlatformMismatch));
}

#[test]
fn corrupt_slot() {
    assert_eq!(verify(&flipped(0x09)), Err(VerifyError::UnknownSlot));
}

#[test]
fn corrupt_abi() {
    assert_eq!(verify(&flipped(0x0C)), Err(VerifyError::AbiMismatch));
}

#[test]
fn corrupt_key_id() {
    assert_eq!(verify(&flipped(0x38)), Err(VerifyError::UnknownSignerKey));
}

#[test]
fn corrupt_key_epoch() {
    // Below the pinned epoch: revoked.
    let mut c = mint_golden();
    c[0x3C..0x40].copy_from_slice(&(GOLDEN_KEY_EPOCH - 1).to_le_bytes());
    assert_eq!(verify(&c), Err(VerifyError::RevokedKeyEpoch));
    // Above the pinned epoch: unknown.
    let mut c = mint_golden();
    c[0x3C..0x40].copy_from_slice(&(GOLDEN_KEY_EPOCH + 1).to_le_bytes());
    assert_eq!(verify(&c), Err(VerifyError::FutureKeyEpoch));
}

#[test]
fn corrupt_not_after() {
    // Nonzero expiry cannot be checked without a trusted time source and
    // MUST be rejected (IC-2).
    assert_eq!(verify(&flipped(0x30)), Err(VerifyError::UnsupportedExpiry));
}

#[test]
fn corrupt_deps_digest() {
    assert_eq!(verify(&flipped(0x60)), Err(VerifyError::DepsDigestMismatch));
}

#[test]
fn corrupt_load_vaddr() {
    assert_eq!(
        verify(&flipped(0x20)),
        Err(VerifyError::LoadAddressMismatch)
    );
}

#[test]
fn corrupt_entry_offset() {
    // Flip the high byte: far outside the payload.
    assert_eq!(verify(&flipped(0x2F)), Err(VerifyError::EntryInvalid));
}

// --- crypto fields --------------------------------------------------------

#[test]
fn corrupt_payload_hash_field() {
    // Checked (constant-time) before the signature: distinct error.
    assert_eq!(
        verify(&flipped(0x40)),
        Err(VerifyError::PayloadHashMismatch)
    );
}

#[test]
fn corrupt_payload_byte() {
    let capsule = mint_golden();
    assert_eq!(
        verify(&flipped(capsule.len() - 1)),
        Err(VerifyError::PayloadHashMismatch)
    );
}

#[test]
fn corrupt_signature() {
    // First, middle, and last byte of the signature field.
    for off in [0x80usize, 0xA0, 0xBF] {
        assert_eq!(
            verify(&flipped(off)),
            Err(VerifyError::BadSignature),
            "offset {off:#x}"
        );
    }
}

// --- rollback -------------------------------------------------------------

#[test]
fn rollback_rejected() {
    // A validly signed capsule with an older monotonic_version: everything
    // upstream passes, the scoped counter rejects it.
    let mut spec = golden_spec();
    spec.monotonic_version = GOLDEN_VERSION - 1;
    let capsule = mint(&spec, GOLDEN_PAYLOAD, &TEST_SEED).unwrap();
    assert_eq!(verify(&capsule), Err(VerifyError::RollbackRejected));
}

// --- order evidence -------------------------------------------------------

#[test]
fn field_checks_precede_signature_check() {
    // Wrong platform AND garbage signature: the platform mismatch must
    // surface, proving the normative order (system checks before crypto).
    let mut c = mint_golden();
    c[0x0A] ^= 0xFF; // platform
    c[0x80] ^= 0xFF; // signature
    assert_eq!(verify(&c), Err(VerifyError::PlatformMismatch));
}

#[test]
fn hash_check_precedes_signature_check() {
    let mut c = mint_golden();
    c[0x40] ^= 0xFF; // payload_sha256 field
    c[0x80] ^= 0xFF; // signature
    assert_eq!(verify(&c), Err(VerifyError::PayloadHashMismatch));
}

#[test]
fn signature_check_precedes_rollback_check() {
    // Validly *structured* rollback violation plus a broken signature:
    // BadSignature must win — rollback state is only consulted for
    // authentic capsules.
    let mut spec = golden_spec();
    spec.monotonic_version = GOLDEN_VERSION - 1;
    let mut c = mint(&spec, GOLDEN_PAYLOAD, &TEST_SEED).unwrap();
    c[0x80] ^= 0xFF;
    assert_eq!(verify(&c), Err(VerifyError::BadSignature));
}
