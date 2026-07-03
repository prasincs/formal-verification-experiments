use std::{env, fs};

use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use update_capsule::{encode_signed_prefix, Header, HEADER_LEN, SIGNED_PREFIX_LEN};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("keygen") if args.len() == 3 => generate_keypair(&args[1], &args[2]),
        Some("sign") if args.len() == 14 => sign_capsule(&args[1..]),
        _ => Err("usage: update-capsule-cli keygen <signing-key.hex> <verify-key.hex> | sign <signing-key.hex> <payload> <capsule> <type> <slot> <platform> <abi> <version> <load-vaddr> <entry-offset> <key-id> <key-epoch>".into()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

fn generate_keypair(signing_path: &str, verifying_path: &str) -> Result<(), String> {
    let key = SigningKey::generate(&mut OsRng);
    fs::write(signing_path, hex::encode(key.to_bytes()) + "\n").map_err(err)?;
    fs::write(verifying_path, hex::encode(key.verifying_key().to_bytes()) + "\n").map_err(err)
}

fn sign_capsule(args: &[String]) -> Result<(), String> {
    let key_bytes = decode_array::<32>(fs::read_to_string(&args[0]).map_err(err)?.trim())?;
    let payload = fs::read(&args[1]).map_err(err)?;
    let header = Header {
        payload_type: number(&args[3])?,
        target_slot: number(&args[4])?,
        target_platform: number(&args[5])?,
        abi_version: number(&args[6])?,
        monotonic_version: number(&args[7])?,
        payload_len: payload.len().try_into().map_err(|_| "payload too large")?,
        load_vaddr: number(&args[8])?,
        entry_offset: number(&args[9])?,
        not_after: 0,
        signer_key_id: number(&args[10])?,
        key_epoch: number(&args[11])?,
        payload_sha256: Sha256::digest(&payload).into(),
        deps_sha256: [0; 32],
    };
    header.validate_payload_type().map_err(|e| e.to_string())?;

    let mut prefix = [0u8; SIGNED_PREFIX_LEN];
    encode_signed_prefix(&header, &mut prefix);
    let mut signed = Vec::with_capacity(SIGNED_PREFIX_LEN + payload.len());
    signed.extend_from_slice(&prefix);
    signed.extend_from_slice(&payload);
    let signature = SigningKey::from_bytes(&key_bytes).sign(&signed).to_bytes();

    let mut capsule = Vec::with_capacity(HEADER_LEN + payload.len());
    capsule.extend_from_slice(&prefix);
    capsule.extend_from_slice(&signature);
    capsule.extend_from_slice(&payload);
    fs::write(&args[2], capsule).map_err(err)
}

fn number<T: TryFrom<u64>>(text: &str) -> Result<T, String> {
    let text = text.replace('_', "");
    let value = if let Some(hex) = text.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        text.parse()
    }
    .map_err(|_| format!("invalid integer {text}"))?;
    T::try_from(value).map_err(|_| "integer out of range".into())
}

fn decode_array<const N: usize>(text: &str) -> Result<[u8; N], String> {
    hex::decode(text)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| format!("expected {N} bytes"))
}

fn err(error: std::io::Error) -> String {
    error.to_string()
}
