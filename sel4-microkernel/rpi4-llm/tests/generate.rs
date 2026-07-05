//! End-to-end engine tests against the committed fixture: numerics pinned
//! against the independent numpy reference (`fixtures/reference_infer.py`),
//! determinism asserted, arena sizing enforced.

use rpi4_llm::receipt::{
    public_key, sign_receipt, verify_and_reexecute, Receipt, TEST_NONCE, TEST_SIGNING_KEY,
};
use rpi4_llm::{token_ids_to_le_bytes, ArenaPlan, Engine, EngineError, VocabEntry};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/tinystories-260k-f32.gguf"
);

const PROMPT: &[u8] = b"One day, Tom the cat";

/// Greedy continuation produced by `fixtures/reference_infer.py` (numpy,
/// float32) for the same prompt — the engine must agree token-for-token.
const REFERENCE_TEXT: &str = " went to the forest. The funny fox ran to the hill with Ben. The";

/// SHA-256 over the 64 generated token ids (little-endian u32s), as also
/// printed and asserted by the `llmdemo-host --expect` CI run.
const REFERENCE_TOKENS_SHA256: &str =
    "7b2b33323cba78f90b50f6ac02d980f46c7e5920f1d00ba2ef736e2fe64e6dce";

fn generate(buf: &[u8], steps: u32) -> (Vec<u32>, String) {
    let desc = rpi4_llm_loader::parse(buf).expect("fixture parses");
    let plan = ArenaPlan::for_model(&desc).unwrap();
    let mut arena = vec![0.0f32; plan.f32_len];
    let mut vocab = vec![VocabEntry::default(); plan.vocab_len];
    let mut engine = Engine::new(&desc, buf, &mut arena, &mut vocab).unwrap();

    let c = *engine.config();
    let mut ids = vec![0u32; c.seq_len as usize];
    let n_prompt = engine.encode(PROMPT, &mut ids).unwrap();

    let total = (n_prompt.saturating_sub(1) as u32 + steps).min(c.seq_len);
    let mut out_ids = Vec::new();
    let mut text = Vec::new();
    let mut piece = [0u8; 128];
    let mut next = ids[0];
    for pos in 0..total {
        engine.forward(next, pos).unwrap();
        next = if (pos as usize + 1) < n_prompt {
            ids[pos as usize + 1]
        } else {
            let id = engine.argmax_logits();
            if id == c.eos_id {
                break;
            }
            out_ids.push(id);
            let n = engine.decode(id, &mut piece);
            text.extend_from_slice(&piece[..n]);
            id
        };
    }
    (out_ids, String::from_utf8(text).unwrap())
}

#[test]
fn matches_reference_implementation() {
    let buf = std::fs::read(FIXTURE).unwrap();
    let (ids, text) = generate(&buf, 64);
    assert_eq!(text, REFERENCE_TEXT);
    assert!(ids.len() >= 32, "must generate at least 32 tokens");

    // hash over the token id stream, same encoding as llmdemo-host
    let id_bytes: Vec<u8> = ids.iter().flat_map(|t| t.to_le_bytes()).collect();
    let hex: String = rpi4_llm::receipt::sha256(&id_bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, REFERENCE_TOKENS_SHA256);
}

#[test]
fn receipt_signature_and_reexecution_verify() {
    let buf = std::fs::read(FIXTURE).unwrap();
    let (ids, _) = generate(&buf, 64);
    let mut token_bytes = vec![0u8; ids.len() * 4];
    let token_len = token_ids_to_le_bytes(&ids, &mut token_bytes).unwrap();
    let token_bytes = &token_bytes[..token_len];
    let receipt = Receipt::new(TEST_NONCE, PROMPT, &buf, 64, ids.len() as u32, token_bytes);
    let signature = sign_receipt(&receipt, &TEST_SIGNING_KEY).unwrap();
    let public_key = public_key(&TEST_SIGNING_KEY);

    let desc = rpi4_llm_loader::parse(&buf).unwrap();
    let plan = ArenaPlan::for_model(&desc).unwrap();
    let mut arena = vec![0.0f32; plan.f32_len];
    let mut vocab = vec![VocabEntry::default(); plan.vocab_len];
    let mut prompt_ids = vec![0u32; desc.config.seq_len as usize];
    let mut output_ids = vec![0u32; 64];
    let mut output_text = vec![0u8; 64 * 128];
    let mut reexec_token_bytes = vec![0u8; 64 * 4];

    verify_and_reexecute(
        &buf,
        PROMPT,
        TEST_NONCE,
        token_bytes,
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
    .unwrap();
}

#[test]
fn generation_is_deterministic() {
    let buf = std::fs::read(FIXTURE).unwrap();
    let a = generate(&buf, 48);
    let b = generate(&buf, 48);
    assert_eq!(a, b);
}

#[test]
fn tokenizer_roundtrip() {
    let buf = std::fs::read(FIXTURE).unwrap();
    let desc = rpi4_llm_loader::parse(&buf).unwrap();
    let plan = ArenaPlan::for_model(&desc).unwrap();
    let mut arena = vec![0.0f32; plan.f32_len];
    let mut vocab = vec![VocabEntry::default(); plan.vocab_len];
    let engine = Engine::new(&desc, &buf, &mut arena, &mut vocab).unwrap();

    let text = b"Hello, world!";
    let mut ids = vec![0u32; 64];
    let n = engine.encode(text, &mut ids).unwrap();
    // byte-level vocab: BOS + one token per byte
    assert_eq!(n, text.len() + 1);
    assert_eq!(ids[0], engine.config().bos_id);

    let mut piece = [0u8; 128];
    let mut round = Vec::new();
    for &id in &ids[1..n] {
        let k = engine.decode(id, &mut piece);
        round.extend_from_slice(&piece[..k]);
    }
    assert_eq!(round, text);
}

#[test]
fn undersized_arenas_fail_closed() {
    let buf = std::fs::read(FIXTURE).unwrap();
    let desc = rpi4_llm_loader::parse(&buf).unwrap();
    let plan = ArenaPlan::for_model(&desc).unwrap();

    let mut small = vec![0.0f32; plan.f32_len - 1];
    let mut vocab = vec![VocabEntry::default(); plan.vocab_len];
    assert_eq!(
        Engine::new(&desc, &buf, &mut small, &mut vocab).err(),
        Some(EngineError::ArenaTooSmall)
    );

    let mut arena = vec![0.0f32; plan.f32_len];
    let mut small_vocab = vec![VocabEntry::default(); plan.vocab_len - 1];
    assert_eq!(
        Engine::new(&desc, &buf, &mut arena, &mut small_vocab).err(),
        Some(EngineError::VocabArenaTooSmall)
    );
}

#[test]
fn context_overflow_fails_closed() {
    let buf = std::fs::read(FIXTURE).unwrap();
    let desc = rpi4_llm_loader::parse(&buf).unwrap();
    let plan = ArenaPlan::for_model(&desc).unwrap();
    let mut arena = vec![0.0f32; plan.f32_len];
    let mut vocab = vec![VocabEntry::default(); plan.vocab_len];
    let mut engine = Engine::new(&desc, &buf, &mut arena, &mut vocab).unwrap();
    let seq = engine.config().seq_len;
    assert_eq!(
        engine.forward(0, seq).err(),
        Some(EngineError::ContextOverflow)
    );
}
