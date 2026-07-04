//! Shared golden fixture for the integration tests.
//!
//! The signing key is the RFC 8032 Ed25519 TEST 1 secret key — a
//! published test vector, deliberately unusable as a production secret.
//! Signing is deterministic (RFC 8032), so the fixture mints
//! byte-identical capsules on every run; `tests/vectors/golden.capsule`
//! is the committed result and `golden.rs` proves it reproduces.

use update_capsule::header::{PAYLOAD_TYPE_PD_CODE, PLATFORM_QEMU_AARCH64, SIGNED_PREFIX_LEN};
use update_capsule::mint::{derive_public_key, mint, CapsuleSpec};
use update_capsule::verify::{RollbackStore, SlotPolicy, SystemProfile, TrustedKey};

/// RFC 8032 §7.1 TEST 1 secret key.
pub const TEST_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

pub const GOLDEN_PAYLOAD: &[u8] =
    b"WP-8 golden capsule payload: deterministic bytes for the verified update pipeline.\n";

pub const GOLDEN_PLATFORM: u16 = PLATFORM_QEMU_AARCH64;
pub const GOLDEN_SLOT: u8 = 3;
pub const GOLDEN_ABI: u32 = 1;
pub const GOLDEN_VERSION: u64 = 5;
pub const GOLDEN_KEY_ID: u32 = 1;
pub const GOLDEN_KEY_EPOCH: u32 = 2;
pub const GOLDEN_REGION_BASE: u64 = 0x4000_0000;
pub const GOLDEN_REGION_SIZE: u64 = 0x1_0000;
pub const GOLDEN_SLOT_GENERATION: u32 = 7;
pub const GOLDEN_ENTRY_OFFSET: u64 = 0x40;

pub fn golden_spec() -> CapsuleSpec {
    CapsuleSpec {
        payload_type: PAYLOAD_TYPE_PD_CODE,
        target_slot: GOLDEN_SLOT,
        target_platform: GOLDEN_PLATFORM,
        abi_version: GOLDEN_ABI,
        monotonic_version: GOLDEN_VERSION,
        load_vaddr: GOLDEN_REGION_BASE,
        entry_offset: GOLDEN_ENTRY_OFFSET,
        not_after: 0,
        signer_key_id: GOLDEN_KEY_ID,
        key_epoch: GOLDEN_KEY_EPOCH,
        deps_sha256: [0u8; 32],
    }
}

pub fn mint_golden() -> Vec<u8> {
    mint(&golden_spec(), GOLDEN_PAYLOAD, &TEST_SEED).expect("golden capsule mints")
}

/// A profile that accepts the golden capsule.
pub struct Fixture {
    pub keys: Vec<TrustedKey>,
    pub slots: Vec<SlotPolicy>,
}

impl Fixture {
    pub fn new() -> Self {
        Fixture {
            keys: vec![TrustedKey {
                key_id: GOLDEN_KEY_ID,
                key_epoch: GOLDEN_KEY_EPOCH,
                public_key: derive_public_key(&TEST_SEED),
            }],
            slots: vec![SlotPolicy {
                slot: GOLDEN_SLOT,
                payload_type: PAYLOAD_TYPE_PD_CODE,
                abi_version: GOLDEN_ABI,
                region_base: GOLDEN_REGION_BASE,
                region_size: GOLDEN_REGION_SIZE,
                slot_generation: GOLDEN_SLOT_GENERATION,
                deps_sha256: [0u8; 32],
            }],
        }
    }

    pub fn profile(&self) -> SystemProfile<'_> {
        SystemProfile {
            platform: GOLDEN_PLATFORM,
            keys: &self.keys,
            slots: &self.slots,
        }
    }
}

/// Rollback store with one flat counter (or none provisioned).
pub struct Counter(pub Option<u64>);

impl RollbackStore for Counter {
    fn current(&self, _slot: u8, _payload_type: u8) -> Option<u64> {
        self.0
    }
}

pub fn scratch_for(capsule: &[u8]) -> Vec<u8> {
    vec![0u8; SIGNED_PREFIX_LEN + capsule.len()]
}
