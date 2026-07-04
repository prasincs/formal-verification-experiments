use std::env;
use std::error::Error;
use std::fs;

use rpi4_llm::{
    build_demo_gguf, test_public_key, verify_and_reexecute, Receipt, EXPECTED_OUTPUT, FIXED_PROMPT,
    MODEL_CAPACITY, OUTPUT_TOKENS, RECEIPT_LEN, TEST_NONCE,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("llm-receipt-verify: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let path = env::args()
        .nth(1)
        .ok_or("usage: llm-receipt-verify <serial-log>")?;
    let log = fs::read_to_string(path)?;

    let output = decode_line::<OUTPUT_TOKENS>(&log, "TOKENS HEX ")?;
    let receipt_bytes = decode_line::<RECEIPT_LEN>(&log, "RECEIPT HEX ")?;
    let signature = decode_line::<64>(&log, "SIGNATURE HEX ")?;
    let public_key = decode_line::<32>(&log, "PUBLIC KEY HEX ")?;
    if public_key != test_public_key() {
        return Err("serial transcript contains an unexpected test public key".into());
    }

    let receipt = Receipt::decode(&receipt_bytes);
    let mut model = [0u8; MODEL_CAPACITY];
    let model_len = build_demo_gguf(&mut model)?;
    verify_and_reexecute(
        &model[..model_len],
        FIXED_PROMPT,
        TEST_NONCE,
        &output,
        &receipt,
        &signature,
        &public_key,
    )?;

    if &output != EXPECTED_OUTPUT {
        return Err("re-executed output does not match the pinned golden output".into());
    }

    println!("RECEIPT VERIFIED");
    println!("REEXECUTION OK");
    Ok(())
}

fn decode_line<const N: usize>(log: &str, prefix: &str) -> Result<[u8; N], Box<dyn Error>> {
    let encoded = log
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .ok_or_else(|| format!("missing serial marker {prefix}"))?;
    let bytes = hex::decode(encoded.trim())?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        format!(
            "marker {prefix} decoded to {} bytes; expected {N}",
            bytes.len()
        )
        .into()
    })
}
