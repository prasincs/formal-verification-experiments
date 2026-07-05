//! Verification-pipeline semantics beyond the corruption matrix:
//! rollback scoping, slot policy, scratch discipline, and payload-type
//! rules.

#![cfg(feature = "mint")]

mod common;

use common::*;
use update_capsule::header::{
    PAYLOAD_TYPE_CONFIG, PAYLOAD_TYPE_MODEL_WEIGHTS, PAYLOAD_TYPE_PD_CODE, SIGNED_PREFIX_LEN,
};
use update_capsule::mint::mint;
use update_capsule::verify::{verify_capsule, VerifyError};

fn verify_with_counter(
    capsule: &[u8],
    counter: Counter,
) -> Result<update_capsule::InstallAuthorization, VerifyError> {
    let fixture = Fixture::new();
    let mut scratch = scratch_for(capsule);
    verify_capsule(capsule, &fixture.profile(), &counter, 1, &mut scratch)
}

#[test]
fn rollback_equal_version_is_reinstallable() {
    let capsule = mint_golden();
    assert!(verify_with_counter(&capsule, Counter(Some(GOLDEN_VERSION))).is_ok());
}

#[test]
fn rollback_newer_version_accepted() {
    let capsule = mint_golden();
    assert!(verify_with_counter(&capsule, Counter(Some(GOLDEN_VERSION - 3))).is_ok());
}

#[test]
fn rollback_missing_counter_fails_closed() {
    let capsule = mint_golden();
    assert_eq!(
        verify_with_counter(&capsule, Counter(None)),
        Err(VerifyError::NoRollbackCounter)
    );
}

#[test]
fn scratch_too_small_is_rejected_not_panicked() {
    let capsule = mint_golden();
    let fixture = Fixture::new();
    let needed = SIGNED_PREFIX_LEN + GOLDEN_PAYLOAD.len();
    for len in [0, 1, SIGNED_PREFIX_LEN, needed - 1] {
        let mut scratch = vec![0u8; len];
        assert_eq!(
            verify_capsule(
                &capsule,
                &fixture.profile(),
                &Counter(Some(GOLDEN_VERSION)),
                1,
                &mut scratch
            ),
            Err(VerifyError::ScratchTooSmall),
            "scratch len {len}"
        );
    }
    // Exactly-sized scratch succeeds.
    let mut scratch = vec![0u8; needed];
    assert!(verify_capsule(
        &capsule,
        &fixture.profile(),
        &Counter(Some(GOLDEN_VERSION)),
        1,
        &mut scratch
    )
    .is_ok());
}

#[test]
fn payload_must_fit_slot_region() {
    let mut fixture = Fixture::new();
    fixture.slots[0].region_size = (GOLDEN_PAYLOAD.len() - 1) as u64;
    let capsule = mint_golden();
    let mut scratch = scratch_for(&capsule);
    assert_eq!(
        verify_capsule(
            &capsule,
            &fixture.profile(),
            &Counter(Some(GOLDEN_VERSION)),
            1,
            &mut scratch
        ),
        Err(VerifyError::PayloadDoesNotFit)
    );
}

#[test]
fn pic_capsule_accepted_for_any_slot_base() {
    // load_vaddr == 0 means position-independent: no base check applies.
    let mut spec = golden_spec();
    spec.load_vaddr = 0;
    let capsule = mint(&spec, GOLDEN_PAYLOAD, &TEST_SEED).unwrap();
    assert!(verify_with_counter(&capsule, Counter(Some(GOLDEN_VERSION))).is_ok());
}

#[test]
fn non_code_payload_types_take_no_load_address_or_entry() {
    for t in [PAYLOAD_TYPE_MODEL_WEIGHTS, PAYLOAD_TYPE_CONFIG] {
        let mut fixture = Fixture::new();
        fixture.slots[0].payload_type = t;

        // Clean weights/config capsule: fine.
        let mut spec = golden_spec();
        spec.payload_type = t;
        spec.load_vaddr = 0;
        spec.entry_offset = 0;
        let capsule = mint(&spec, GOLDEN_PAYLOAD, &TEST_SEED).unwrap();
        let mut scratch = scratch_for(&capsule);
        assert!(verify_capsule(
            &capsule,
            &fixture.profile(),
            &Counter(Some(GOLDEN_VERSION)),
            1,
            &mut scratch
        )
        .is_ok());

        // Nonzero load_vaddr on a non-code payload: reserved-must-be-zero.
        spec.load_vaddr = GOLDEN_REGION_BASE;
        let capsule = mint(&spec, GOLDEN_PAYLOAD, &TEST_SEED).unwrap();
        let mut scratch = scratch_for(&capsule);
        assert_eq!(
            verify_capsule(
                &capsule,
                &fixture.profile(),
                &Counter(Some(GOLDEN_VERSION)),
                1,
                &mut scratch
            ),
            Err(VerifyError::LoadAddressMismatch)
        );

        // Nonzero entry_offset likewise.
        spec.load_vaddr = 0;
        spec.entry_offset = 8;
        let capsule = mint(&spec, GOLDEN_PAYLOAD, &TEST_SEED).unwrap();
        let mut scratch = scratch_for(&capsule);
        assert_eq!(
            verify_capsule(
                &capsule,
                &fixture.profile(),
                &Counter(Some(GOLDEN_VERSION)),
                1,
                &mut scratch
            ),
            Err(VerifyError::EntryInvalid)
        );
    }
}

#[test]
fn code_entry_must_be_inside_payload() {
    let mut spec = golden_spec();
    spec.entry_offset = GOLDEN_PAYLOAD.len() as u64; // one past the end
    let capsule = mint(&spec, GOLDEN_PAYLOAD, &TEST_SEED).unwrap();
    assert_eq!(
        verify_with_counter(&capsule, Counter(Some(GOLDEN_VERSION))),
        Err(VerifyError::EntryInvalid)
    );
    // Empty code payloads can therefore never verify: no valid entry.
    let mut spec = golden_spec();
    spec.entry_offset = 0;
    let capsule = mint(&spec, b"", &TEST_SEED).unwrap();
    assert_eq!(
        verify_with_counter(&capsule, Counter(Some(GOLDEN_VERSION))),
        Err(VerifyError::EntryInvalid)
    );
}

#[test]
fn slot_scope_is_slot_and_payload_type() {
    // Same slot id, different payload_type: no policy row → UnknownSlot.
    // (Rollback scope is per (slot, type); so is the policy table.)
    let mut fixture = Fixture::new();
    fixture.slots[0].payload_type = PAYLOAD_TYPE_MODEL_WEIGHTS;
    let capsule = mint_golden(); // type = pd-code
    let mut scratch = scratch_for(&capsule);
    assert_eq!(
        verify_capsule(
            &capsule,
            &fixture.profile(),
            &Counter(Some(GOLDEN_VERSION)),
            1,
            &mut scratch
        ),
        Err(VerifyError::UnknownSlot)
    );
}

#[test]
fn deps_digest_must_match_slot_declaration() {
    // Capsule declares a manifest digest, slot expects none.
    let mut spec = golden_spec();
    spec.deps_sha256 = [0x11; 32];
    let capsule = mint(&spec, GOLDEN_PAYLOAD, &TEST_SEED).unwrap();
    assert_eq!(
        verify_with_counter(&capsule, Counter(Some(GOLDEN_VERSION))),
        Err(VerifyError::DepsDigestMismatch)
    );

    // Slot expects the digest the capsule declares: accepted.
    let mut fixture = Fixture::new();
    fixture.slots[0].deps_sha256 = [0x11; 32];
    let mut scratch = scratch_for(&capsule);
    assert!(verify_capsule(
        &capsule,
        &fixture.profile(),
        &Counter(Some(GOLDEN_VERSION)),
        1,
        &mut scratch
    )
    .is_ok());
}

#[test]
fn wrong_signer_key_is_bad_signature() {
    // Sign with a different (also well-formed) key while claiming the
    // pinned key_id: every field checks out, the signature does not.
    let other_seed = [0x42u8; 32];
    let capsule = mint(&golden_spec(), GOLDEN_PAYLOAD, &other_seed).unwrap();
    assert_eq!(
        verify_with_counter(&capsule, Counter(Some(GOLDEN_VERSION))),
        Err(VerifyError::BadSignature)
    );
}

#[test]
fn payload_type_1_and_4_differ_only_in_entry_rules() {
    // wasm-tool payloads are position-independent by nature: entry rules
    // are the non-code ones (must be zero) since no native entry exists.
    let mut fixture = Fixture::new();
    fixture.slots[0].payload_type = update_capsule::header::PAYLOAD_TYPE_WASM_TOOL;
    let mut spec = golden_spec();
    spec.payload_type = update_capsule::header::PAYLOAD_TYPE_WASM_TOOL;
    spec.load_vaddr = 0;
    spec.entry_offset = 0;
    let capsule = mint(&spec, GOLDEN_PAYLOAD, &TEST_SEED).unwrap();
    let mut scratch = scratch_for(&capsule);
    assert!(verify_capsule(
        &capsule,
        &fixture.profile(),
        &Counter(Some(GOLDEN_VERSION)),
        1,
        &mut scratch
    )
    .is_ok());
    assert_eq!(fixture.slots[0].payload_type, PAYLOAD_TYPE_PD_CODE + 3);
}
