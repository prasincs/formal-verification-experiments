//! Golden-file vectors: the committed capsule verifies, and minting the
//! same inputs reproduces it byte-for-byte (format drift breaks this
//! test before it breaks a device).

#![cfg(feature = "mint")]

mod common;

use common::*;
use update_capsule::crypto;
use update_capsule::header::{self, HEADER_LEN};
use update_capsule::verify::verify_capsule;

const GOLDEN_CAPSULE: &[u8] = include_bytes!("vectors/golden.capsule");
const GOLDEN_PUBKEY: &[u8] = include_bytes!("vectors/signer.pub");

#[test]
fn golden_capsule_is_reproducible() {
    assert_eq!(
        mint_golden().as_slice(),
        GOLDEN_CAPSULE,
        "minting the golden spec no longer reproduces the committed vector — \
         the wire format changed"
    );
}

#[test]
fn golden_pubkey_matches_seed() {
    assert_eq!(
        update_capsule::mint::derive_public_key(&TEST_SEED).as_slice(),
        GOLDEN_PUBKEY
    );
}

#[test]
fn golden_capsule_header_fields() {
    let h = header::parse(GOLDEN_CAPSULE).expect("golden capsule parses");
    assert_eq!(h.payload_type, 1);
    assert_eq!(h.target_slot, GOLDEN_SLOT);
    assert_eq!(h.target_platform, GOLDEN_PLATFORM);
    assert_eq!(h.abi_version, GOLDEN_ABI);
    assert_eq!(h.monotonic_version, GOLDEN_VERSION);
    assert_eq!(h.payload_len, GOLDEN_PAYLOAD.len() as u64);
    assert_eq!(h.load_vaddr, GOLDEN_REGION_BASE);
    assert_eq!(h.entry_offset, GOLDEN_ENTRY_OFFSET);
    assert_eq!(h.not_after, 0);
    assert_eq!(h.signer_key_id, GOLDEN_KEY_ID);
    assert_eq!(h.key_epoch, GOLDEN_KEY_EPOCH);
    assert_eq!(header::payload_bytes(GOLDEN_CAPSULE), GOLDEN_PAYLOAD);
}

#[test]
fn golden_capsule_verifies_end_to_end() {
    let fixture = Fixture::new();
    let mut scratch = scratch_for(GOLDEN_CAPSULE);
    let auth = verify_capsule(
        GOLDEN_CAPSULE,
        &fixture.profile(),
        &Counter(Some(GOLDEN_VERSION)),
        0xA11CE,
        &mut scratch,
    )
    .expect("golden capsule is eligible for installation");

    assert_eq!(auth.auth_id, 0xA11CE);
    assert_eq!(auth.target_slot, GOLDEN_SLOT);
    assert_eq!(auth.payload_type, 1);
    assert_eq!(auth.slot_generation, GOLDEN_SLOT_GENERATION);
    assert_eq!(auth.monotonic_version, GOLDEN_VERSION);
    // The digest is the authority: it must be the payload's real hash.
    assert_eq!(auth.payload_sha256, crypto::sha256(GOLDEN_PAYLOAD));
}

#[test]
fn golden_capsule_signature_covers_prefix_and_payload() {
    // Manual reconstruction of the signed message, independent of the
    // pipeline's scratch assembly.
    let mut msg = Vec::new();
    msg.extend_from_slice(&GOLDEN_CAPSULE[..0x80]);
    msg.extend_from_slice(&GOLDEN_CAPSULE[HEADER_LEN..]);
    let mut sig = [0u8; 64];
    sig.copy_from_slice(header::signature_field(GOLDEN_CAPSULE));
    let mut pk = [0u8; 32];
    pk.copy_from_slice(GOLDEN_PUBKEY);
    assert!(crypto::ed25519_verify(&msg, &pk, &sig));
}
