//! Capsule construction and signing (feature `mint`).
//!
//! For the host CLI, tests, and other WPs that need to mint test
//! capsules. Verifier PDs must not enable this feature — a verifier
//! carries no signing code. Requires an allocator (hosts have one).

extern crate alloc;

use alloc::vec::Vec;

use crate::crypto;
use crate::header::{
    CAPSULE_FORMAT_VERSION, DEPS_SHA256_OFFSET, HEADER_LEN, PAYLOAD_LEN_MAX, PAYLOAD_SHA256_OFFSET,
    SIGNATURE_OFFSET, SIGNED_PREFIX_LEN,
};

/// Everything the signer chooses about a capsule (the payload digest and
/// length are computed, not chosen).
#[derive(Clone, Copy, Debug)]
pub struct CapsuleSpec {
    pub payload_type: u8,
    pub target_slot: u8,
    pub target_platform: u16,
    pub abi_version: u32,
    pub monotonic_version: u64,
    pub load_vaddr: u64,
    pub entry_offset: u64,
    /// MUST be 0 until a trusted time source is specified (IC-2).
    pub not_after: u64,
    pub signer_key_id: u32,
    pub key_epoch: u32,
    pub deps_sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MintError {
    /// Payload exceeds the format's implementation limit.
    PayloadTooLarge,
    /// The ed25519 backend refused to sign (cannot happen for payloads
    /// within the length limit).
    SigningFailed,
}

/// Serialize the signed prefix (`[0x00..0x80)`) for `spec` + `payload`.
fn write_signed_prefix(spec: &CapsuleSpec, payload: &[u8], out: &mut [u8]) {
    debug_assert!(out.len() == SIGNED_PREFIX_LEN);
    out[0..4].copy_from_slice(b"SAOC");
    out[0x04..0x08].copy_from_slice(&CAPSULE_FORMAT_VERSION.to_le_bytes());
    out[0x08] = spec.payload_type;
    out[0x09] = spec.target_slot;
    out[0x0A..0x0C].copy_from_slice(&spec.target_platform.to_le_bytes());
    out[0x0C..0x10].copy_from_slice(&spec.abi_version.to_le_bytes());
    out[0x10..0x18].copy_from_slice(&spec.monotonic_version.to_le_bytes());
    out[0x18..0x20].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    out[0x20..0x28].copy_from_slice(&spec.load_vaddr.to_le_bytes());
    out[0x28..0x30].copy_from_slice(&spec.entry_offset.to_le_bytes());
    out[0x30..0x38].copy_from_slice(&spec.not_after.to_le_bytes());
    out[0x38..0x3C].copy_from_slice(&spec.signer_key_id.to_le_bytes());
    out[0x3C..0x40].copy_from_slice(&spec.key_epoch.to_le_bytes());
    out[PAYLOAD_SHA256_OFFSET..PAYLOAD_SHA256_OFFSET + 32]
        .copy_from_slice(&crypto::sha256(payload));
    out[DEPS_SHA256_OFFSET..DEPS_SHA256_OFFSET + 32].copy_from_slice(&spec.deps_sha256);
}

/// Build and sign a complete capsule: `prefix ++ signature ++ payload`.
///
/// The signature covers `prefix ++ payload` (IC-2). Signing is
/// deterministic (RFC 8032), so identical inputs mint identical
/// capsules — which is what makes committed golden vectors reproducible.
pub fn mint(
    spec: &CapsuleSpec,
    payload: &[u8],
    signing_key: &[u8; 32],
) -> Result<Vec<u8>, MintError> {
    if payload.len() as u64 > PAYLOAD_LEN_MAX {
        return Err(MintError::PayloadTooLarge);
    }

    // Assemble the signed message: prefix ++ payload.
    let mut msg = alloc::vec![0u8; SIGNED_PREFIX_LEN + payload.len()];
    write_signed_prefix(spec, payload, &mut msg[..SIGNED_PREFIX_LEN]);
    msg[SIGNED_PREFIX_LEN..].copy_from_slice(payload);

    let signature =
        libcrux_ed25519::sign(&msg, signing_key).map_err(|_| MintError::SigningFailed)?;

    // Splice the signature between prefix and payload for the wire form.
    let mut capsule = alloc::vec![0u8; HEADER_LEN + payload.len()];
    capsule[..SIGNED_PREFIX_LEN].copy_from_slice(&msg[..SIGNED_PREFIX_LEN]);
    capsule[SIGNATURE_OFFSET..HEADER_LEN].copy_from_slice(&signature);
    capsule[HEADER_LEN..].copy_from_slice(payload);
    Ok(capsule)
}

/// Derive the ed25519 public key for a 32-byte secret key.
pub fn derive_public_key(signing_key: &[u8; 32]) -> [u8; 32] {
    let mut pk = [0u8; 32];
    libcrux_ed25519::secret_to_public(&mut pk, signing_key);
    pk
}
