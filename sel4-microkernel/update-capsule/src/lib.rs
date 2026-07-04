//! # Update Capsule (workplan IC-2)
//!
//! `no_std` library implementing the signed update capsule format for the
//! high-assurance agent appliance (WP-8):
//!
//! - [`header`] — the fixed-offset wire format and a **Verus-verified
//!   totality parser**: for every input, either a validated
//!   [`CapsuleHeader`] or a distinct [`ParseError`]; no panic, no
//!   out-of-bounds read, no overflow (proven, not tested).
//! - [`verify`] — the verification pipeline in IC-2's normative order,
//!   ending in a one-shot [`InstallAuthorization`] (the digest is the
//!   authority, not the buffer).
//! - [`crypto`] — thin seam over formally verified SHA-256/ed25519
//!   (libcrux HACL* extractions), plus constant-time digest comparison.
//! - [`mint`] (feature `mint`) — capsule construction + signing for the
//!   host CLI and tests. Verifier PDs must not enable it.
//!
//! Non-goals (per WP-8): applying capsules (the supervisor's job, Wave
//! 2), transport, key management/provisioning policy.
//!
//! See `README.md` for the format, threat notes, and how to run the
//! Verus proofs.

#![no_std]
#![forbid(unsafe_code)]
// The payload_type range check lives inside `verus!` and is written as
// explicit comparisons because Verus verifies those directly;
// `RangeInclusive::contains` has no spec.
#![allow(clippy::manual_range_contains)]

pub mod crypto;
pub mod header;
#[cfg(feature = "mint")]
pub mod mint;
pub mod verify;

pub use header::{CapsuleHeader, ParseError};
pub use verify::{
    InstallAuthorization, RollbackStore, SlotPolicy, SystemProfile, TrustedKey, VerifyError,
};
