//! Deterministic malformed-input sweep (CI-friendly complement to the
//! `cargo fuzz` targets in `fuzz/`): random buffers, exhaustive
//! single-byte corruption, and exhaustive truncation. The parser's
//! totality is *proven* in Verus; this keeps the whole pipeline honest
//! on the same inputs.

#![cfg(feature = "mint")]

mod common;

use common::*;
use update_capsule::header;
use update_capsule::verify::verify_capsule;

/// xorshift64* — deterministic, seedable, no dependencies.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
}

fn verify(
    capsule: &[u8],
) -> Result<update_capsule::InstallAuthorization, update_capsule::VerifyError> {
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

#[test]
fn random_buffers_never_panic() {
    let mut rng = Rng(0x57505F38); // "WP_8"
    for _ in 0..20_000 {
        let len = (rng.next() % 1024) as usize;
        let buf: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        let _ = header::parse(&buf);
        let _ = verify(&buf);
    }
}

#[test]
fn random_header_mutations_never_panic() {
    // Start from a valid capsule and splatter random bytes over random
    // offsets: much deeper pipeline coverage than pure noise.
    let golden = mint_golden();
    let mut rng = Rng(0xC0FFEE);
    for _ in 0..20_000 {
        let mut c = golden.clone();
        for _ in 0..(rng.next() % 8 + 1) {
            let off = (rng.next() as usize) % c.len();
            c[off] = rng.next() as u8;
        }
        let _ = verify(&c);
    }
}

#[test]
fn no_single_byte_corruption_survives() {
    // Flipping any single byte anywhere in the capsule must be rejected:
    // header fields are either checked directly or covered by the
    // signature; the payload is covered by the digest.
    let golden = mint_golden();
    for off in 0..golden.len() {
        let mut c = golden.clone();
        c[off] ^= 0xFF;
        assert!(verify(&c).is_err(), "byte flip at {off:#x} was accepted");
    }
}

#[test]
fn every_truncation_is_rejected() {
    let golden = mint_golden();
    for len in 0..golden.len() {
        assert!(
            verify(&golden[..len]).is_err(),
            "truncation to {len} was accepted"
        );
    }
}
