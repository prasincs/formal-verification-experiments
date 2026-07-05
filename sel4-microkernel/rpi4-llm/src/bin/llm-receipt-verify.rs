use std::error::Error;
use std::fmt;

use rpi4_llm::receipt::{verify_and_reexecute, Receipt, RECEIPT_LEN, TEST_NONCE};
use rpi4_llm::{ArenaPlan, VocabEntry, DEFAULT_PROMPT};

const DEFAULT_MODEL: &[u8] = include_bytes!("../../fixtures/tinystories-260k-f32.gguf");

#[derive(Debug)]
struct VerifyCliError(String);

impl fmt::Display for VerifyCliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for VerifyCliError {}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let log_path = args
        .next()
        .ok_or_else(|| VerifyCliError("usage: llm-receipt-verify LOG [MODEL.gguf]".into()))?;
    let model = match args.next() {
        Some(path) => std::fs::read(path)?,
        None => DEFAULT_MODEL.to_vec(),
    };
    let log = std::fs::read_to_string(log_path)?;

    let receipt_bytes = decode_fixed::<RECEIPT_LEN>(&log, "RECEIPT HEX ")?;
    let signature = decode_fixed::<64>(&log, "SIGNATURE HEX ")?;
    let public_key = decode_fixed::<32>(&log, "PUBLIC KEY HEX ")?;
    let token_bytes = decode_line(&log, "TOKEN IDS HEX ")?;

    let receipt = Receipt::decode(&receipt_bytes);
    let desc = rpi4_llm_loader::parse(&model)
        .map_err(|e| VerifyCliError(format!("model rejected by loader: {e:?}")))?;
    let plan = ArenaPlan::for_model(&desc)
        .map_err(|e| VerifyCliError(format!("arena plan failed: {e:?}")))?;
    let mut arena = vec![0.0f32; plan.f32_len];
    let mut vocab = vec![VocabEntry::default(); plan.vocab_len];
    let mut prompt_ids = vec![0u32; desc.config.seq_len as usize];
    let mut output_ids = vec![0u32; receipt.requested_steps as usize];
    let mut output_text = vec![0u8; receipt.requested_steps as usize * 128];
    let mut reexec_token_bytes = vec![0u8; receipt.requested_steps as usize * 4];

    verify_and_reexecute(
        &model,
        DEFAULT_PROMPT,
        TEST_NONCE,
        &token_bytes,
        &receipt,
        &signature,
        &public_key,
        &mut arena,
        &mut vocab,
        &mut prompt_ids,
        &mut output_ids,
        &mut output_text,
        &mut reexec_token_bytes,
    )
    .map_err(|e| VerifyCliError(format!("receipt verification failed: {e:?}")))?;

    println!("RECEIPT VERIFIED");
    println!("REEXECUTION OK");
    Ok(())
}

fn decode_fixed<const N: usize>(log: &str, prefix: &str) -> Result<[u8; N], Box<dyn Error>> {
    let bytes = decode_line(log, prefix)?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        VerifyCliError(format!(
            "marker {prefix} decoded to {} bytes; expected {N}",
            bytes.len()
        ))
        .into()
    })
}

fn decode_line(log: &str, prefix: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let encoded = log
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .ok_or_else(|| VerifyCliError(format!("missing serial marker {prefix}")))?;
    decode_hex(encoded.trim())
}

fn decode_hex(s: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if s.len() % 2 != 0 {
        return Err(VerifyCliError("hex string has odd length".into()).into());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for pair in bytes.chunks_exact(2) {
        let hi = hex_nibble(pair[0])?;
        let lo = hex_nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8, Box<dyn Error>> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(VerifyCliError(format!("invalid hex byte {byte:#x}")).into()),
    }
}
