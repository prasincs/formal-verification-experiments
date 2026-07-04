#![no_std]

#[cfg(feature = "std")]
extern crate std;

use core::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rpi4_llm_loader::{self as loader, ModelDescriptor, GGML_TYPE_F32, GGUF_MAGIC, GGUF_VERSION};
use sha2::{Digest, Sha256};

pub const VOCAB_SIZE: usize = 16;
pub const OUTPUT_TOKENS: usize = 32;
pub const MODEL_CAPACITY: usize = 1152;
pub const RECEIPT_VERSION: u32 = 1;
pub const RECEIPT_LEN: usize = 128;
pub const TEST_SIGNING_KEY: [u8; 32] = [7; 32];
pub const TEST_NONCE: [u8; 16] = [0xa5; 16];
pub const FIXED_PROMPT: &[u8] = b"cycle from token zero";
pub const EXPECTED_OUTPUT: &[u8; OUTPUT_TOKENS] = b"123456789abcdef0123456789abcdef0";
pub const TOKEN_BYTES: &[u8; VOCAB_SIZE] = b"0123456789abcdef";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelBuildError {
    BufferTooSmall,
}

impl fmt::Display for ModelBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "model buffer is too small")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ModelBuildError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InferenceError {
    Loader(loader::LoadError),
    MissingTransitionTensor,
    InvalidShape,
    NonFiniteWeight,
}

impl fmt::Display for InferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Loader(error) => write!(f, "{error}"),
            Self::MissingTransitionTensor => write!(f, "transition tensor is missing"),
            Self::InvalidShape => write!(f, "transition tensor must be 16x16 F32"),
            Self::NonFiniteWeight => write!(f, "transition tensor contains a non-finite weight"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for InferenceError {}

impl From<loader::LoadError> for InferenceError {
    fn from(value: loader::LoadError) -> Self {
        Self::Loader(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub version: u32,
    pub verifier_nonce: [u8; 16],
    pub input_digest: [u8; 32],
    pub weights_digest: [u8; 32],
    pub seed_token: u32,
    pub token_count: u32,
    pub temperature_milli: u32,
    pub output_digest: [u8; 32],
}

impl Receipt {
    pub fn encode(&self) -> [u8; RECEIPT_LEN] {
        let mut out = [0u8; RECEIPT_LEN];
        out[0..4].copy_from_slice(&self.version.to_le_bytes());
        out[4..20].copy_from_slice(&self.verifier_nonce);
        out[20..52].copy_from_slice(&self.input_digest);
        out[52..84].copy_from_slice(&self.weights_digest);
        out[84..88].copy_from_slice(&self.seed_token.to_le_bytes());
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
            seed_token: u32::from_le_bytes(bytes[84..88].try_into().expect("fixed range")),
            token_count: u32::from_le_bytes(bytes[88..92].try_into().expect("fixed range")),
            temperature_milli: u32::from_le_bytes(
                bytes[92..96].try_into().expect("fixed range"),
            ),
            output_digest,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Execution {
    pub output: [u8; OUTPUT_TOKENS],
    pub receipt: Receipt,
    pub signature: [u8; 64],
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
    Reexecution,
    Inference,
}

impl fmt::Display for ReceiptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "receipt verification failed: {self:?}")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ReceiptError {}

/// Build a compact, standards-shaped GGUF v3 file containing one 16x16 F32
/// transition tensor. Row `i` has its unique maximum at `(i + 1) mod 16`.
pub fn build_demo_gguf(buffer: &mut [u8]) -> Result<usize, ModelBuildError> {
    let header_and_info = 24 + 8 + b"transition".len() + 4 + 16 + 4 + 8;
    let data_start = align32(header_and_info);
    let matrix_bytes = VOCAB_SIZE * VOCAB_SIZE * core::mem::size_of::<f32>();
    let total = data_start + matrix_bytes;
    if buffer.len() < total {
        return Err(ModelBuildError::BufferTooSmall);
    }
    buffer[..total].fill(0);

    let mut cursor = 0;
    put(buffer, &mut cursor, &GGUF_MAGIC);
    put(buffer, &mut cursor, &GGUF_VERSION.to_le_bytes());
    put(buffer, &mut cursor, &1u64.to_le_bytes());
    put(buffer, &mut cursor, &0u64.to_le_bytes());
    put(buffer, &mut cursor, &(b"transition".len() as u64).to_le_bytes());
    put(buffer, &mut cursor, b"transition");
    put(buffer, &mut cursor, &2u32.to_le_bytes());
    put(buffer, &mut cursor, &(VOCAB_SIZE as u64).to_le_bytes());
    put(buffer, &mut cursor, &(VOCAB_SIZE as u64).to_le_bytes());
    put(buffer, &mut cursor, &GGML_TYPE_F32.to_le_bytes());
    put(buffer, &mut cursor, &0u64.to_le_bytes());

    for row in 0..VOCAB_SIZE {
        for column in 0..VOCAB_SIZE {
            let weight = if column == (row + 1) % VOCAB_SIZE {
                1.0f32
            } else {
                0.0f32
            };
            let offset = data_start + (row * VOCAB_SIZE + column) * 4;
            buffer[offset..offset + 4].copy_from_slice(&weight.to_le_bytes());
        }
    }

    Ok(total)
}

fn put(buffer: &mut [u8], cursor: &mut usize, bytes: &[u8]) {
    let end = *cursor + bytes.len();
    buffer[*cursor..end].copy_from_slice(bytes);
    *cursor = end;
}

const fn align32(value: usize) -> usize {
    (value + 31) & !31
}

pub fn parse_model(model: &[u8]) -> Result<ModelDescriptor, InferenceError> {
    Ok(loader::parse(model)?)
}

pub fn generate(
    model: &[u8],
    descriptor: &ModelDescriptor,
    seed_token: u32,
) -> Result<[u8; OUTPUT_TOKENS], InferenceError> {
    let tensor = descriptor
        .find(b"transition")
        .ok_or(InferenceError::MissingTransitionTensor)?;
    if tensor.ggml_type != GGML_TYPE_F32
        || tensor.dimension_count != 2
        || tensor.dimensions[0] != VOCAB_SIZE as u64
        || tensor.dimensions[1] != VOCAB_SIZE as u64
        || tensor.byte_len != VOCAB_SIZE * VOCAB_SIZE * 4
    {
        return Err(InferenceError::InvalidShape);
    }
    let weights = loader::tensor_bytes(model, tensor)?;
    let mut current = seed_token as usize % VOCAB_SIZE;
    let mut output = [0u8; OUTPUT_TOKENS];

    for token in &mut output {
        let mut best_index = 0usize;
        let mut best_weight = f32::NEG_INFINITY;
        for column in 0..VOCAB_SIZE {
            let offset = (current * VOCAB_SIZE + column) * 4;
            let weight = f32::from_le_bytes(
                weights[offset..offset + 4]
                    .try_into()
                    .expect("validated tensor range"),
            );
            if !weight.is_finite() {
                return Err(InferenceError::NonFiniteWeight);
            }
            if weight > best_weight {
                best_weight = weight;
                best_index = column;
            }
        }
        current = best_index;
        *token = TOKEN_BYTES[current];
    }
    Ok(output)
}

pub fn execute(
    model: &[u8],
    prompt: &[u8],
    seed_token: u32,
    nonce: [u8; 16],
    signing_key: [u8; 32],
) -> Result<Execution, InferenceError> {
    let descriptor = parse_model(model)?;
    let output = generate(model, &descriptor, seed_token)?;
    let receipt = Receipt {
        version: RECEIPT_VERSION,
        verifier_nonce: nonce,
        input_digest: Sha256::digest(prompt).into(),
        weights_digest: Sha256::digest(model).into(),
        seed_token,
        token_count: OUTPUT_TOKENS as u32,
        temperature_milli: 0,
        output_digest: Sha256::digest(output).into(),
    };
    let signature = SigningKey::from_bytes(&signing_key)
        .sign(&receipt.encode())
        .to_bytes();
    Ok(Execution {
        output,
        receipt,
        signature,
    })
}

pub fn verify_and_reexecute(
    model: &[u8],
    prompt: &[u8],
    expected_nonce: [u8; 16],
    output: &[u8; OUTPUT_TOKENS],
    receipt: &Receipt,
    signature: &[u8; 64],
    public_key: &[u8; 32],
) -> Result<(), ReceiptError> {
    if receipt.version != RECEIPT_VERSION {
        return Err(ReceiptError::Version);
    }
    if receipt.verifier_nonce != expected_nonce {
        return Err(ReceiptError::Nonce);
    }
    if receipt.input_digest != <[u8; 32]>::from(Sha256::digest(prompt)) {
        return Err(ReceiptError::InputDigest);
    }
    if receipt.weights_digest != <[u8; 32]>::from(Sha256::digest(model)) {
        return Err(ReceiptError::WeightsDigest);
    }
    if receipt.token_count != OUTPUT_TOKENS as u32 || receipt.temperature_milli != 0 {
        return Err(ReceiptError::Config);
    }
    if receipt.output_digest != <[u8; 32]>::from(Sha256::digest(output)) {
        return Err(ReceiptError::OutputDigest);
    }

    let verifying_key = VerifyingKey::from_bytes(public_key).map_err(|_| ReceiptError::PublicKey)?;
    let signature = Signature::from_bytes(signature);
    verifying_key
        .verify(&receipt.encode(), &signature)
        .map_err(|_| ReceiptError::Signature)?;

    let descriptor = parse_model(model).map_err(|_| ReceiptError::Inference)?;
    let reexecuted =
        generate(model, &descriptor, receipt.seed_token).map_err(|_| ReceiptError::Inference)?;
    if &reexecuted != output {
        return Err(ReceiptError::Reexecution);
    }
    Ok(())
}

pub fn test_public_key() -> [u8; 32] {
    SigningKey::from_bytes(&TEST_SIGNING_KEY)
        .verifying_key()
        .to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_loads_and_generates_expected_tokens() {
        let mut bytes = [0u8; MODEL_CAPACITY];
        let len = build_demo_gguf(&mut bytes).unwrap();
        let descriptor = parse_model(&bytes[..len]).unwrap();
        let output = generate(&bytes[..len], &descriptor, 0).unwrap();
        assert_eq!(&output, EXPECTED_OUTPUT);
    }

    #[test]
    fn receipt_signature_and_reexecution_verify() {
        let mut bytes = [0u8; MODEL_CAPACITY];
        let len = build_demo_gguf(&mut bytes).unwrap();
        let execution = execute(
            &bytes[..len],
            FIXED_PROMPT,
            0,
            TEST_NONCE,
            TEST_SIGNING_KEY,
        )
        .unwrap();
        verify_and_reexecute(
            &bytes[..len],
            FIXED_PROMPT,
            TEST_NONCE,
            &execution.output,
            &execution.receipt,
            &execution.signature,
            &test_public_key(),
        )
        .unwrap();
    }

    #[test]
    fn changed_output_fails_before_reexecution() {
        let mut bytes = [0u8; MODEL_CAPACITY];
        let len = build_demo_gguf(&mut bytes).unwrap();
        let execution = execute(
            &bytes[..len],
            FIXED_PROMPT,
            0,
            TEST_NONCE,
            TEST_SIGNING_KEY,
        )
        .unwrap();
        let mut output = execution.output;
        output[0] ^= 1;
        assert_eq!(
            verify_and_reexecute(
                &bytes[..len],
                FIXED_PROMPT,
                TEST_NONCE,
                &output,
                &execution.receipt,
                &execution.signature,
                &test_public_key(),
            ),
            Err(ReceiptError::OutputDigest)
        );
    }
}
