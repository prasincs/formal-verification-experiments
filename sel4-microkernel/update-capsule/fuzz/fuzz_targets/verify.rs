//! Fuzz the full verification pipeline against a fixed profile: no
//! input may panic it (acceptance would require forging ed25519).
//! Run: `cargo +nightly fuzz run verify -- -max_total_time=60`

#![no_main]

use libfuzzer_sys::fuzz_target;
use update_capsule::verify::{verify_capsule, RollbackStore, SlotPolicy, SystemProfile, TrustedKey};

/// The golden test signer's public key (RFC 8032 TEST 1).
const PUBLIC_KEY: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07,
    0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07,
    0x51, 0x1a,
];

struct Counter;

impl RollbackStore for Counter {
    fn current(&self, _slot: u8, _payload_type: u8) -> Option<u64> {
        Some(5)
    }
}

fuzz_target!(|data: &[u8]| {
    let keys = [TrustedKey { key_id: 1, key_epoch: 2, public_key: PUBLIC_KEY }];
    let slots = [SlotPolicy {
        slot: 3,
        payload_type: 1,
        abi_version: 1,
        region_base: 0x4000_0000,
        region_size: 0x1_0000,
        slot_generation: 7,
        deps_sha256: [0u8; 32],
    }];
    let profile = SystemProfile { platform: 1, keys: &keys, slots: &slots };
    let mut scratch = vec![0u8; 0x80 + data.len()];
    let _ = verify_capsule(data, &profile, &Counter, 1, &mut scratch);
});
