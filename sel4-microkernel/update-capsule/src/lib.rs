#![no_std]

#[cfg(feature = "alloc-crypto")]
extern crate alloc;

use core::convert::TryInto;
use core::fmt;

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use verus_builtin_macros::verus;

pub const MAGIC: [u8; 4] = *b"SAOC";
pub const FORMAT_VERSION: u32 = 2;
pub const SIGNED_PREFIX_LEN: usize = 0x80;
pub const SIGNATURE_LEN: usize = 64;
pub const HEADER_LEN: usize = 0xc0;

pub mod payload_type {
    pub const PD_CODE: u8 = 1;
    pub const MODEL_WEIGHTS: u8 = 2;
    pub const CONFIG: u8 = 3;
    pub const WASM_TOOL: u8 = 4;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub payload_type: u8,
    pub target_slot: u8,
    pub target_platform: u16,
    pub abi_version: u32,
    pub monotonic_version: u64,
    pub payload_len: u64,
    pub load_vaddr: u64,
    pub entry_offset: u64,
    pub not_after: u64,
    pub signer_key_id: u32,
    pub key_epoch: u32,
    pub payload_sha256: [u8; 32],
    pub deps_sha256: [u8; 32],
}

impl Header {
    pub fn validate_payload_type(&self) -> Result<(), ParseError> {
        if (payload_type::PD_CODE..=payload_type::WASM_TOOL).contains(&self.payload_type) {
            Ok(())
        } else {
            Err(ParseError::PayloadType(self.payload_type))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    TooShort,
    Magic,
    FormatVersion(u32),
    PayloadType(u8),
    LengthOverflow,
    LengthMismatch { declared: usize, actual: usize },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "capsule is shorter than the fixed header"),
            Self::Magic => write!(f, "invalid capsule magic"),
            Self::FormatVersion(value) => write!(f, "unsupported capsule version {value}"),
            Self::PayloadType(value) => write!(f, "unsupported payload type {value}"),
            Self::LengthOverflow => write!(f, "payload length overflows the address space"),
            Self::LengthMismatch { declared, actual } => {
                write!(f, "capsule length is {actual}, expected {declared}")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError<E> {
    Parse(ParseError),
    Platform { expected: u16, actual: u16 },
    Slot { expected: u8, actual: u8 },
    PayloadType { expected: u8, actual: u8 },
    Abi { expected: u32, actual: u32 },
    SignerKey { expected: u32, actual: u32 },
    KeyEpoch { expected: u32, actual: u32 },
    LoadAddress { expected: u64, actual: u64 },
    TrustedTimeUnavailable,
    Expired { not_after: u64, now: u64 },
    PayloadDigest,
    Signature(E),
    Rollback { current: u64, candidate: u64 },
}

impl<E: fmt::Display> fmt::Display for VerifyError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "{error}"),
            Self::Platform { expected, actual } => {
                write!(f, "platform {actual} does not match {expected}")
            }
            Self::Slot { expected, actual } => write!(f, "slot {actual} does not match {expected}"),
            Self::PayloadType { expected, actual } => {
                write!(f, "payload type {actual} does not match {expected}")
            }
            Self::Abi { expected, actual } => write!(f, "ABI {actual} does not match {expected}"),
            Self::SignerKey { expected, actual } => {
                write!(f, "signer key {actual} does not match {expected}")
            }
            Self::KeyEpoch { expected, actual } => {
                write!(f, "key epoch {actual} does not match {expected}")
            }
            Self::LoadAddress { expected, actual } => {
                write!(f, "load address {actual:#x} does not match {expected:#x}")
            }
            Self::TrustedTimeUnavailable => write!(f, "capsule expiry requires trusted wall time"),
            Self::Expired { not_after, now } => {
                write!(f, "capsule expired at {not_after}; trusted time is {now}")
            }
            Self::PayloadDigest => write!(f, "payload digest mismatch"),
            Self::Signature(error) => write!(f, "signature verification failed: {error}"),
            Self::Rollback { current, candidate } => {
                write!(f, "version {candidate} is not newer than {current}")
            }
        }
    }
}

pub struct ParsedCapsule<'a> {
    pub header: Header,
    pub signed_prefix: &'a [u8; SIGNED_PREFIX_LEN],
    pub signature: &'a [u8; SIGNATURE_LEN],
    pub payload: &'a [u8],
}

pub struct VerifiedCapsule<'a> {
    pub header: Header,
    pub payload: &'a [u8],
}

/// Runtime expectations. Rollback state is supplied per `(target_slot,
/// payload_type)` by the caller; the library deliberately cannot fall back to
/// a global counter.
#[derive(Clone, Debug)]
pub struct VerificationContext {
    pub target_platform: u16,
    pub target_slot: u8,
    pub payload_type: u8,
    pub abi_version: u32,
    pub signer_key_id: u32,
    pub key_epoch: u32,
    pub slot_base: u64,
    pub trusted_unix_time: Option<u64>,
    pub scoped_rollback_version: u64,
}

/// A scatter/gather signature interface avoids requiring the no_std parser to
/// allocate a contiguous copy of `prefix || payload`.
pub trait SignatureVerifier {
    type Error;

    fn verify(
        &self,
        signed_prefix: &[u8; SIGNED_PREFIX_LEN],
        payload: &[u8],
        signature: &[u8; SIGNATURE_LEN],
    ) -> Result<(), Self::Error>;
}

pub fn parse_capsule(bytes: &[u8]) -> Result<ParsedCapsule<'_>, ParseError> {
    if bytes.len() < HEADER_LEN {
        return Err(ParseError::TooShort);
    }
    if bytes[0..4] != MAGIC {
        return Err(ParseError::Magic);
    }
    let version = read_u32(bytes, 0x04);
    if version != FORMAT_VERSION {
        return Err(ParseError::FormatVersion(version));
    }

    let header = Header {
        payload_type: bytes[0x08],
        target_slot: bytes[0x09],
        target_platform: read_u16(bytes, 0x0a),
        abi_version: read_u32(bytes, 0x0c),
        monotonic_version: read_u64(bytes, 0x10),
        payload_len: read_u64(bytes, 0x18),
        load_vaddr: read_u64(bytes, 0x20),
        entry_offset: read_u64(bytes, 0x28),
        not_after: read_u64(bytes, 0x30),
        signer_key_id: read_u32(bytes, 0x38),
        key_epoch: read_u32(bytes, 0x3c),
        payload_sha256: bytes[0x40..0x60].try_into().expect("fixed range"),
        deps_sha256: bytes[0x60..0x80].try_into().expect("fixed range"),
    };
    header.validate_payload_type()?;

    let payload_len: usize = header
        .payload_len
        .try_into()
        .map_err(|_| ParseError::LengthOverflow)?;
    let expected = HEADER_LEN
        .checked_add(payload_len)
        .ok_or(ParseError::LengthOverflow)?;
    if bytes.len() != expected {
        return Err(ParseError::LengthMismatch {
            declared: expected,
            actual: bytes.len(),
        });
    }

    Ok(ParsedCapsule {
        header,
        signed_prefix: bytes[0..SIGNED_PREFIX_LEN].try_into().expect("fixed range"),
        signature: bytes[SIGNED_PREFIX_LEN..HEADER_LEN]
            .try_into()
            .expect("fixed range"),
        payload: &bytes[HEADER_LEN..expected],
    })
}

pub fn verify_capsule<'a, V: SignatureVerifier>(
    bytes: &'a [u8],
    context: &VerificationContext,
    verifier: &V,
) -> Result<VerifiedCapsule<'a>, VerifyError<V::Error>> {
    // IC-2 order is security-relevant: parse/bounds, bindings, time, digest,
    // signature, then scoped rollback state.
    let parsed = parse_capsule(bytes).map_err(VerifyError::Parse)?;
    let header = &parsed.header;

    if header.target_platform != context.target_platform {
        return Err(VerifyError::Platform {
            expected: context.target_platform,
            actual: header.target_platform,
        });
    }
    if header.target_slot != context.target_slot {
        return Err(VerifyError::Slot {
            expected: context.target_slot,
            actual: header.target_slot,
        });
    }
    if header.payload_type != context.payload_type {
        return Err(VerifyError::PayloadType {
            expected: context.payload_type,
            actual: header.payload_type,
        });
    }
    if header.abi_version != context.abi_version {
        return Err(VerifyError::Abi {
            expected: context.abi_version,
            actual: header.abi_version,
        });
    }
    if header.signer_key_id != context.signer_key_id {
        return Err(VerifyError::SignerKey {
            expected: context.signer_key_id,
            actual: header.signer_key_id,
        });
    }
    if header.key_epoch != context.key_epoch {
        return Err(VerifyError::KeyEpoch {
            expected: context.key_epoch,
            actual: header.key_epoch,
        });
    }
    if header.load_vaddr != 0 && header.load_vaddr != context.slot_base {
        return Err(VerifyError::LoadAddress {
            expected: context.slot_base,
            actual: header.load_vaddr,
        });
    }

    if header.not_after != 0 {
        let now = context
            .trusted_unix_time
            .ok_or(VerifyError::TrustedTimeUnavailable)?;
        if now > header.not_after {
            return Err(VerifyError::Expired {
                not_after: header.not_after,
                now,
            });
        }
    }

    let actual_digest: [u8; 32] = Sha256::digest(parsed.payload).into();
    if !bool::from(actual_digest.ct_eq(&header.payload_sha256)) {
        return Err(VerifyError::PayloadDigest);
    }

    verifier
        .verify(parsed.signed_prefix, parsed.payload, parsed.signature)
        .map_err(VerifyError::Signature)?;

    if header.monotonic_version <= context.scoped_rollback_version {
        return Err(VerifyError::Rollback {
            current: context.scoped_rollback_version,
            candidate: header.monotonic_version,
        });
    }

    Ok(VerifiedCapsule {
        header: parsed.header,
        payload: parsed.payload,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("fixed range"))
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("fixed range"))
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("fixed range"))
}

pub fn encode_signed_prefix(header: &Header, out: &mut [u8; SIGNED_PREFIX_LEN]) {
    out.fill(0);
    out[0..4].copy_from_slice(&MAGIC);
    out[0x04..0x08].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    out[0x08] = header.payload_type;
    out[0x09] = header.target_slot;
    out[0x0a..0x0c].copy_from_slice(&header.target_platform.to_le_bytes());
    out[0x0c..0x10].copy_from_slice(&header.abi_version.to_le_bytes());
    out[0x10..0x18].copy_from_slice(&header.monotonic_version.to_le_bytes());
    out[0x18..0x20].copy_from_slice(&header.payload_len.to_le_bytes());
    out[0x20..0x28].copy_from_slice(&header.load_vaddr.to_le_bytes());
    out[0x28..0x30].copy_from_slice(&header.entry_offset.to_le_bytes());
    out[0x30..0x38].copy_from_slice(&header.not_after.to_le_bytes());
    out[0x38..0x3c].copy_from_slice(&header.signer_key_id.to_le_bytes());
    out[0x3c..0x40].copy_from_slice(&header.key_epoch.to_le_bytes());
    out[0x40..0x60].copy_from_slice(&header.payload_sha256);
    out[0x60..0x80].copy_from_slice(&header.deps_sha256);
}

#[cfg(feature = "alloc-crypto")]
pub mod dalek {
    use alloc::vec::Vec;
    use core::fmt;

    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    use super::{SignatureVerifier, SIGNATURE_LEN, SIGNED_PREFIX_LEN};

    pub struct DalekVerifier(pub VerifyingKey);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DalekError;

    impl fmt::Display for DalekError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "invalid Ed25519 signature")
        }
    }

    impl SignatureVerifier for DalekVerifier {
        type Error = DalekError;

        fn verify(
            &self,
            signed_prefix: &[u8; SIGNED_PREFIX_LEN],
            payload: &[u8],
            signature: &[u8; SIGNATURE_LEN],
        ) -> Result<(), Self::Error> {
            let mut message = Vec::with_capacity(SIGNED_PREFIX_LEN + payload.len());
            message.extend_from_slice(signed_prefix);
            message.extend_from_slice(payload);
            let signature = Signature::from_bytes(signature);
            self.0
                .verify_strict(&message, &signature)
                .map_err(|_| DalekError)
        }
    }
}

verus! {

pub open spec fn payload_end_fits(payload_len: usize) -> bool {
    payload_len <= usize::MAX - HEADER_LEN
}

pub fn checked_payload_end(payload_len: usize) -> (result: Option<usize>)
    ensures
        result.is_some() ==> payload_end_fits(payload_len),
        result.is_some() ==> result.unwrap() == HEADER_LEN + payload_len,
        result.is_none() ==> !payload_end_fits(payload_len),
{
    HEADER_LEN.checked_add(payload_len)
}

} // verus!

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec::Vec;

    use ed25519_dalek::{Signer, SigningKey};

    use super::dalek::DalekVerifier;
    use super::*;

    fn fixture() -> (Vec<u8>, VerificationContext, DalekVerifier) {
        let payload = b"worker-v2";
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let header = Header {
            payload_type: payload_type::PD_CODE,
            target_slot: 3,
            target_platform: 1,
            abi_version: 4,
            monotonic_version: 9,
            payload_len: payload.len() as u64,
            load_vaddr: 0x5000_0000,
            entry_offset: 0x40,
            not_after: 0,
            signer_key_id: 12,
            key_epoch: 2,
            payload_sha256: Sha256::digest(payload).into(),
            deps_sha256: [0; 32],
        };
        let mut prefix = [0u8; SIGNED_PREFIX_LEN];
        encode_signed_prefix(&header, &mut prefix);
        let mut message = Vec::from(prefix);
        message.extend_from_slice(payload);
        let signature = signing.sign(&message).to_bytes();
        let mut capsule = message[..SIGNED_PREFIX_LEN].to_vec();
        capsule.extend_from_slice(&signature);
        capsule.extend_from_slice(payload);
        let context = VerificationContext {
            target_platform: 1,
            target_slot: 3,
            payload_type: payload_type::PD_CODE,
            abi_version: 4,
            signer_key_id: 12,
            key_epoch: 2,
            slot_base: 0x5000_0000,
            trusted_unix_time: None,
            scoped_rollback_version: 8,
        };
        (capsule, context, DalekVerifier(signing.verifying_key()))
    }

    #[test]
    fn valid_capsule_is_accepted() {
        let (capsule, context, verifier) = fixture();
        let verified = verify_capsule(&capsule, &context, &verifier).unwrap();
        assert_eq!(verified.payload, b"worker-v2");
    }

    #[test]
    fn each_security_field_has_a_distinct_failure() {
        let (capsule, context, verifier) = fixture();

        let mut value = capsule.clone();
        value[0] ^= 1;
        assert!(matches!(verify_capsule(&value, &context, &verifier), Err(VerifyError::Parse(ParseError::Magic))));

        let mut value = capsule.clone();
        value[4] = 3;
        assert!(matches!(verify_capsule(&value, &context, &verifier), Err(VerifyError::Parse(ParseError::FormatVersion(_)))));

        let mut value = capsule.clone();
        value[0x18..0x20].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(verify_capsule(&value, &context, &verifier), Err(VerifyError::Parse(ParseError::LengthOverflow | ParseError::LengthMismatch { .. }))));

        let mut value = capsule.clone();
        *value.last_mut().unwrap() ^= 1;
        assert!(matches!(verify_capsule(&value, &context, &verifier), Err(VerifyError::PayloadDigest)));

        let mut value = capsule.clone();
        value[0x80] ^= 1;
        assert!(matches!(verify_capsule(&value, &context, &verifier), Err(VerifyError::Signature(_))));

        let mut rollback = context.clone();
        rollback.scoped_rollback_version = 9;
        assert!(matches!(verify_capsule(&capsule, &rollback, &verifier), Err(VerifyError::Rollback { .. })));
    }

    #[test]
    fn nonzero_expiry_requires_trusted_time() {
        let (mut capsule, context, verifier) = fixture();
        capsule[0x30..0x38].copy_from_slice(&100u64.to_le_bytes());
        // The signed bytes changed, but time policy is deliberately checked
        // before signature verification in the normative pipeline.
        assert!(matches!(verify_capsule(&capsule, &context, &verifier), Err(VerifyError::TrustedTimeUnavailable)));
    }

    #[test]
    fn trailing_data_is_rejected() {
        let (mut capsule, _, _) = fixture();
        capsule.push(0);
        assert!(matches!(parse_capsule(&capsule), Err(ParseError::LengthMismatch { .. })));
    }
}
