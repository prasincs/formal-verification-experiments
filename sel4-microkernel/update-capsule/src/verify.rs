//! Capsule verification pipeline (workplan IC-2, normative order).
//!
//! ```text
//! parse header (totality — reject, never trust)
//!   → check payload_len bounds against buffer            (in parse)
//!   → check target_platform,
//!           target_slot + payload_type,
//!           abi_version,
//!           signer_key_id + key_epoch  against the running system
//!   → check not_after (MUST be 0 — no trusted time source)
//!   → hash payload → compare payload_sha256 (constant-time)
//!   → verify signature
//!   → check monotonic_version against the scoped NV counter
//! ```
//!
//! Only then is the payload **eligible for installation**, and the sole
//! output is a **one-shot install authorization** — the digest is the
//! authority, not the buffer. The installer (WP-12, Wave 2) re-hashes its
//! own private staging copy against `payload_sha256` before any write and
//! tracks consumed `auth_id`s.
//!
//! This module is deliberately plain `no_std` Rust (tested, not
//! Verus-verified): every untrusted-input hazard lives in the verified
//! parser, and the crypto is consumed from formally verified
//! implementations. The pipeline itself only sequences checks over
//! already-validated data.

use crate::crypto;
use crate::header::{self, CapsuleHeader, ParseError, PAYLOAD_TYPE_PD_CODE, SIGNED_PREFIX_LEN};

/// A pinned signing key the running system trusts.
pub struct TrustedKey {
    /// Matches the capsule's `signer_key_id`.
    pub key_id: u32,
    /// Current rotation epoch for this key. Capsules signed under an
    /// older epoch are revoked; newer epochs are unknown. Both are
    /// rejected — only an exact match verifies.
    pub key_epoch: u32,
    pub public_key: [u8; 32],
}

/// The running system's declaration of one updatable slot, scoped by
/// `(slot, payload_type)` exactly like the rollback state (IC-2: a model
/// update must not burn the code-slot rollback counter).
pub struct SlotPolicy {
    pub slot: u8,
    pub payload_type: u8,
    /// Slot protocol/ABI version the running system provides.
    pub abi_version: u32,
    /// Declared base address of the slot's executable/data region. A
    /// non-PIC capsule (`load_vaddr != 0`) must be linked for exactly
    /// this address.
    pub region_base: u64,
    /// Size of the slot region; the payload must fit.
    pub region_size: u64,
    /// Current generation of the slot (bound into the authorization so a
    /// stale authorization cannot apply after the slot was restarted).
    pub slot_generation: u32,
    /// Expected dependency/config manifest digest (zero = none declared).
    pub deps_sha256: [u8; 32],
}

/// Everything about the running system a capsule is checked against.
pub struct SystemProfile<'a> {
    pub platform: u16,
    pub keys: &'a [TrustedKey],
    pub slots: &'a [SlotPolicy],
}

/// Scoped anti-rollback state: one monotonic counter per
/// `(target_slot, payload_type)`. In the real system this is a TPM NV
/// counter owned by the verifier PD; hosts and tests supply a mock.
pub trait RollbackStore {
    /// Current counter for the scope, or `None` if no counter is
    /// provisioned (which rejects the capsule — absence of rollback
    /// state must fail closed).
    fn current(&self, slot: u8, payload_type: u8) -> Option<u64>;
}

/// The verifier's sole output: a one-shot authorization naming exactly
/// what may be installed where. Delivered (in the real system) over the
/// verifier→installer private channel — channel identity is the
/// authenticity mechanism, so this carries no signature of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstallAuthorization {
    /// Fresh nonce chosen by the caller. Freshness is the caller's
    /// obligation (the verifier PD owns nonce generation); the installer
    /// tracks consumed ids and rejects reuse.
    pub auth_id: u64,
    /// The digest that *is* the authority: the installer re-hashes its
    /// private staging copy against this before any write.
    pub payload_sha256: [u8; 32],
    pub target_slot: u8,
    pub payload_type: u8,
    pub slot_generation: u32,
    pub monotonic_version: u64,
}

/// Rejection reasons, one per check — corruption of any single field
/// surfaces as its own distinct error (and does so *before* the
/// signature check when the field check precedes it in the normative
/// order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// Structural rejection from the verified parser.
    Parse(ParseError),
    /// Capsule targets a different platform than this system.
    PlatformMismatch,
    /// No declared slot accepts this `(target_slot, payload_type)`.
    UnknownSlot,
    /// Payload does not fit the declared slot region.
    PayloadDoesNotFit,
    /// Non-PIC capsule linked for an address that is not the slot base,
    /// or a load address on a payload type that takes none.
    LoadAddressMismatch,
    /// Entry point outside the payload, or nonzero on a payload type
    /// that takes none (reserved-must-be-zero).
    EntryInvalid,
    /// Capsule expects a different slot ABI than the system provides.
    AbiMismatch,
    /// `signer_key_id` names no pinned key.
    UnknownSignerKey,
    /// Capsule signed under a revoked (older) key epoch.
    RevokedKeyEpoch,
    /// Capsule signed under a future (unknown) key epoch.
    FutureKeyEpoch,
    /// `not_after` is nonzero: the device has monotonic counters, not
    /// wall time, so nonzero expiry cannot be checked and MUST be
    /// rejected (IC-2).
    UnsupportedExpiry,
    /// Dependency-manifest digest differs from the slot's declaration.
    DepsDigestMismatch,
    /// Payload hash does not match the signed `payload_sha256`.
    PayloadHashMismatch,
    /// ed25519 signature verification failed.
    BadSignature,
    /// No rollback counter provisioned for this scope (fails closed).
    NoRollbackCounter,
    /// `monotonic_version` is older than the scoped rollback counter.
    RollbackRejected,
    /// Caller-provided scratch buffer is too small to assemble the
    /// signed message (`0x80 + payload_len` bytes needed).
    ScratchTooSmall,
}

impl From<ParseError> for VerifyError {
    fn from(e: ParseError) -> Self {
        VerifyError::Parse(e)
    }
}

fn find_key<'a>(profile: &'a SystemProfile, key_id: u32) -> Option<&'a TrustedKey> {
    profile.keys.iter().find(|k| k.key_id == key_id)
}

fn find_slot<'a>(profile: &'a SystemProfile, slot: u8, payload_type: u8) -> Option<&'a SlotPolicy> {
    profile
        .slots
        .iter()
        .find(|s| s.slot == slot && s.payload_type == payload_type)
}

/// Run the full IC-2 verification pipeline over `capsule`.
///
/// `scratch` must hold at least `SIGNED_PREFIX_LEN + payload_len` bytes;
/// it is used to assemble the signed message `header[0x00..0x80) ++
/// payload` contiguously (the signature field sits between the two in
/// the wire format). `auth_id` is the caller's fresh nonce for the
/// resulting authorization.
///
/// On success the capsule payload is *eligible for installation* — no
/// more, no less: the returned [`InstallAuthorization`] is the only
/// authority this function grants, and it names the payload by digest,
/// not by buffer.
pub fn verify_capsule(
    capsule: &[u8],
    profile: &SystemProfile,
    rollback: &dyn RollbackStore,
    auth_id: u64,
    scratch: &mut [u8],
) -> Result<InstallAuthorization, VerifyError> {
    // 1. Totality-parse the header. Bounds of payload_len against the
    //    buffer (and the u32 implementation limit) are proven inside.
    let h: CapsuleHeader = header::parse(capsule)?;

    // 2. Running-system checks, cheapest first, all before any crypto.
    check_platform(&h, profile)?;
    let slot = check_slot(&h, profile)?;
    check_abi(&h, slot)?;
    let key = check_signer(&h, profile)?;

    // 3. Expiry: no trusted time source exists, so only 0 is acceptable.
    if h.not_after != 0 {
        return Err(VerifyError::UnsupportedExpiry);
    }

    // 4. Dependency manifest digest against the slot's declaration.
    let deps_field = header::deps_sha256_field(capsule);
    if !crypto::ct_eq(deps_field, &slot.deps_sha256) {
        return Err(VerifyError::DepsDigestMismatch);
    }

    // 5. Hash the payload and compare against the signed digest,
    //    constant-time.
    let payload = header::payload_bytes(capsule);
    let payload_digest = crypto::sha256(payload);
    let digest_field = header::payload_sha256_field(capsule);
    if !crypto::ct_eq(digest_field, &payload_digest) {
        return Err(VerifyError::PayloadHashMismatch);
    }

    // 6. Signature over prefix ++ payload (assembled in scratch — the
    //    signature field sits between them on the wire).
    let msg_len = SIGNED_PREFIX_LEN + payload.len();
    if scratch.len() < msg_len {
        return Err(VerifyError::ScratchTooSmall);
    }
    scratch[..SIGNED_PREFIX_LEN].copy_from_slice(header::signed_prefix(capsule));
    scratch[SIGNED_PREFIX_LEN..msg_len].copy_from_slice(payload);
    let mut signature = [0u8; 64];
    signature.copy_from_slice(header::signature_field(capsule));
    if !crypto::ed25519_verify(&scratch[..msg_len], &key.public_key, &signature) {
        return Err(VerifyError::BadSignature);
    }

    // 7. Anti-rollback, scoped per (target_slot, payload_type). Equal is
    //    accepted (re-install of the current version); the consumer bumps
    //    the NV counter to the capsule's version after a successful
    //    install.
    let counter = rollback
        .current(h.target_slot, h.payload_type)
        .ok_or(VerifyError::NoRollbackCounter)?;
    if h.monotonic_version < counter {
        return Err(VerifyError::RollbackRejected);
    }

    let mut payload_sha256 = [0u8; 32];
    payload_sha256.copy_from_slice(digest_field);
    Ok(InstallAuthorization {
        auth_id,
        payload_sha256,
        target_slot: h.target_slot,
        payload_type: h.payload_type,
        slot_generation: slot.slot_generation,
        monotonic_version: h.monotonic_version,
    })
}

fn check_platform(h: &CapsuleHeader, profile: &SystemProfile) -> Result<(), VerifyError> {
    if h.target_platform != profile.platform {
        return Err(VerifyError::PlatformMismatch);
    }
    Ok(())
}

fn check_slot<'a>(
    h: &CapsuleHeader,
    profile: &'a SystemProfile,
) -> Result<&'a SlotPolicy, VerifyError> {
    let slot = find_slot(profile, h.target_slot, h.payload_type).ok_or(VerifyError::UnknownSlot)?;
    if h.payload_len > slot.region_size {
        return Err(VerifyError::PayloadDoesNotFit);
    }
    if h.payload_type == PAYLOAD_TYPE_PD_CODE {
        // PIC (load_vaddr == 0) or linked for exactly the slot base.
        if h.load_vaddr != 0 && h.load_vaddr != slot.region_base {
            return Err(VerifyError::LoadAddressMismatch);
        }
        // Entry point must land inside the payload.
        if h.entry_offset >= h.payload_len {
            return Err(VerifyError::EntryInvalid);
        }
    } else {
        // Non-code payloads take no load address or entry point;
        // reserved fields must be zero (IC-2: unknown-field smuggling).
        if h.load_vaddr != 0 {
            return Err(VerifyError::LoadAddressMismatch);
        }
        if h.entry_offset != 0 {
            return Err(VerifyError::EntryInvalid);
        }
    }
    Ok(slot)
}

fn check_abi(h: &CapsuleHeader, slot: &SlotPolicy) -> Result<(), VerifyError> {
    if h.abi_version != slot.abi_version {
        return Err(VerifyError::AbiMismatch);
    }
    Ok(())
}

fn check_signer<'a>(
    h: &CapsuleHeader,
    profile: &'a SystemProfile,
) -> Result<&'a TrustedKey, VerifyError> {
    let key = find_key(profile, h.signer_key_id).ok_or(VerifyError::UnknownSignerKey)?;
    if h.key_epoch < key.key_epoch {
        return Err(VerifyError::RevokedKeyEpoch);
    }
    if h.key_epoch > key.key_epoch {
        return Err(VerifyError::FutureKeyEpoch);
    }
    Ok(key)
}
