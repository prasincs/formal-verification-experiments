//! Canonical signed execution receipts for deterministic local inference.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::{generate_into, token_ids_to_le_bytes, RunBuffers, VocabEntry};

pub const RECEIPT_VERSION: u32 = 1;
pub const RECEIPT_LEN: usize = 128;
pub const TEST_NONCE: [u8; 16] = [0xa5; 16];
pub const TEST_SIGNING_KEY: [u8; 32] = [7; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub version: u32,
    pub verifier_nonce: [u8; 16],
    pub input_digest: [u8; 32],
    pub weights_digest: [u8; 32],
    pub requested_steps: u32,
    pub token_count: u32,
    pub temperature_milli: u32,
    pub output_digest: [u8; 32],
}

impl Receipt {
    pub fn new(
        nonce: [u8; 16],
        prompt: &[u8],
        model: &[u8],
        requested_steps: u32,
        token_count: u32,
        output_token_bytes: &[u8],
    ) -> Self {
        Self {
            version: RECEIPT_VERSION,
            verifier_nonce: nonce,
            input_digest: sha256(prompt),
            weights_digest: sha256(model),
            requested_steps,
            token_count,
            temperature_milli: 0,
            output_digest: sha256(output_token_bytes),
        }
    }

    pub fn encode(&self) -> [u8; RECEIPT_LEN] {
        let mut out = [0u8; RECEIPT_LEN];
        out[0..4].copy_from_slice(&self.version.to_le_bytes());
        out[4..20].copy_from_slice(&self.verifier_nonce);
        out[20..52].copy_from_slice(&self.input_digest);
        out[52..84].copy_from_slice(&self.weights_digest);
        out[84..88].copy_from_slice(&self.requested_steps.to_le_bytes());
        out[88..92].copy_from_slice(&self.token_count.to_le_bytes());
        out[92..96].copy_from_slice(&self.temperature_milli.to_le_bytes());
        out[96..128].copy_from_slice(&self.output_digest);
        out
    }

    pub fn decode(bytes: &[u8; RECEIPT_LEN]) -> Self {
        let mut verifier_nonce = [0u8; 16];
        verifier_nonce.copy_from_slice(&bytes[4..20]);
        let mut input_digest = [0u8; 32];
        input_digest.copy_from_slice(&bytes[20..52]);
        let mut weights_digest = [0u8; 32];
        weights_digest.copy_from_slice(&bytes[52..84]);
        let mut output_digest = [0u8; 32];
        output_digest.copy_from_slice(&bytes[96..128]);
        Self {
            version: u32::from_le_bytes(bytes[0..4].try_into().expect("fixed range")),
            verifier_nonce,
            input_digest,
            weights_digest,
            requested_steps: u32::from_le_bytes(bytes[84..88].try_into().expect("fixed range")),
            token_count: u32::from_le_bytes(bytes[88..92].try_into().expect("fixed range")),
            temperature_milli: u32::from_le_bytes(bytes[92..96].try_into().expect("fixed range")),
            output_digest,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptError {
    Version,
    Nonce,
    InputDigest,
    WeightsDigest,
    Config,
    OutputDigest,
    PublicKey,
    Signature,
    OutputEncoding,
    Reexecution,
    Inference,
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn sign_receipt(receipt: &Receipt, signing_key: &[u8; 32]) -> Result<[u8; 64], ReceiptError> {
    Ok(SigningKey::from_bytes(signing_key)
        .sign(&receipt.encode())
        .to_bytes())
}

pub fn public_key(signing_key: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(signing_key)
        .verifying_key()
        .to_bytes()
}

pub fn verify_signature(
    receipt: &Receipt,
    signature: &[u8; 64],
    public_key: &[u8; 32],
) -> Result<(), ReceiptError> {
    let verifying_key =
        VerifyingKey::from_bytes(public_key).map_err(|_| ReceiptError::PublicKey)?;
    verifying_key
        .verify(&receipt.encode(), &Signature::from_bytes(signature))
        .map_err(|_| ReceiptError::Signature)
}

#[allow(clippy::too_many_arguments)]
pub fn verify_and_reexecute(
    model: &[u8],
    prompt: &[u8],
    expected_nonce: [u8; 16],
    output_token_bytes: &[u8],
    receipt: &Receipt,
    signature: &[u8; 64],
    public_key: &[u8; 32],
    arena: &mut [f32],
    vocab_arena: &mut [VocabEntry],
    prompt_ids: &mut [u32],
    output_ids: &mut [u32],
    output_text: &mut [u8],
    reexec_token_bytes: &mut [u8],
) -> Result<(), ReceiptError> {
    if receipt.version != RECEIPT_VERSION {
        return Err(ReceiptError::Version);
    }
    if receipt.verifier_nonce != expected_nonce {
        return Err(ReceiptError::Nonce);
    }
    if receipt.input_digest != sha256(prompt) {
        return Err(ReceiptError::InputDigest);
    }
    if receipt.weights_digest != sha256(model) {
        return Err(ReceiptError::WeightsDigest);
    }
    if receipt.temperature_milli != 0 {
        return Err(ReceiptError::Config);
    }
    if receipt.output_digest != sha256(output_token_bytes) {
        return Err(ReceiptError::OutputDigest);
    }
    if receipt.token_count as usize * 4 != output_token_bytes.len() {
        return Err(ReceiptError::OutputDigest);
    }
    verify_signature(receipt, signature, public_key)?;

    let generated = generate_into(
        model,
        prompt,
        receipt.requested_steps as usize,
        RunBuffers {
            arena,
            vocab_arena,
            prompt_ids,
            output_ids,
            output_text,
        },
    )
    .map_err(|_| ReceiptError::Inference)?;
    if generated.token_count != receipt.token_count as usize {
        return Err(ReceiptError::Reexecution);
    }
    let len = token_ids_to_le_bytes(&output_ids[..generated.token_count], reexec_token_bytes)
        .map_err(|_| ReceiptError::OutputEncoding)?;
    if &reexec_token_bytes[..len] != output_token_bytes {
        return Err(ReceiptError::Reexecution);
    }
    Ok(())
}
