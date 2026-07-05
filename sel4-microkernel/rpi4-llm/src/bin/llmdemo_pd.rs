#![no_std]
#![no_main]

use core::fmt;

use rpi4_llm::receipt::{public_key, sign_receipt, Receipt, TEST_NONCE, TEST_SIGNING_KEY};
use rpi4_llm::{
    generate_into, receipt, token_ids_to_le_bytes, RunBuffers, VocabEntry, DEFAULT_PROMPT,
    DEFAULT_STEPS,
};
use sel4_microkit::{debug_println, protection_domain, NullHandler};

const MODEL: &[u8] = include_bytes!("../../fixtures/tinystories-260k-f32.gguf");
const FIXTURE_F32_ARENA_LEN: usize = 84_835;
const FIXTURE_VOCAB_LEN: usize = 259;
const FIXTURE_SEQ_LEN: usize = 256;
const OUTPUT_TEXT_CAPACITY: usize = DEFAULT_STEPS * 128;

const VOCAB_ENTRY_ZERO: VocabEntry = VocabEntry {
    off: 0,
    len: 0,
    byte: 0,
    score: 0.0,
};

static mut ARENA: [f32; FIXTURE_F32_ARENA_LEN] = [0.0; FIXTURE_F32_ARENA_LEN];
static mut VOCAB: [VocabEntry; FIXTURE_VOCAB_LEN] = [VOCAB_ENTRY_ZERO; FIXTURE_VOCAB_LEN];
static mut PROMPT_IDS: [u32; FIXTURE_SEQ_LEN] = [0; FIXTURE_SEQ_LEN];
static mut OUTPUT_IDS: [u32; DEFAULT_STEPS] = [0; DEFAULT_STEPS];
static mut OUTPUT_TEXT: [u8; OUTPUT_TEXT_CAPACITY] = [0; OUTPUT_TEXT_CAPACITY];
static mut TOKEN_BYTES: [u8; DEFAULT_STEPS * 4] = [0; DEFAULT_STEPS * 4];

struct Hex<'a>(&'a [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[protection_domain]
fn init() -> NullHandler {
    debug_println!("LLMDEMO START");

    let arena = unsafe { &mut *core::ptr::addr_of_mut!(ARENA) };
    let vocab = unsafe { &mut *core::ptr::addr_of_mut!(VOCAB) };
    let prompt_ids = unsafe { &mut *core::ptr::addr_of_mut!(PROMPT_IDS) };
    let output_ids = unsafe { &mut *core::ptr::addr_of_mut!(OUTPUT_IDS) };
    let output_text = unsafe { &mut *core::ptr::addr_of_mut!(OUTPUT_TEXT) };
    let token_bytes = unsafe { &mut *core::ptr::addr_of_mut!(TOKEN_BYTES) };

    let generated = generate_into(
        MODEL,
        DEFAULT_PROMPT,
        DEFAULT_STEPS,
        RunBuffers {
            arena,
            vocab_arena: vocab,
            prompt_ids,
            output_ids,
            output_text,
        },
    )
    .expect("committed fixture must generate");
    let token_byte_len = token_ids_to_le_bytes(&output_ids[..generated.token_count], token_bytes)
        .expect("token byte buffer fits committed fixture");
    let token_bytes = &token_bytes[..token_byte_len];
    let output_digest = receipt::sha256(token_bytes);
    let output_text = &output_text[..generated.text_len];
    let output = core::str::from_utf8(output_text).unwrap_or("<non-utf8>");

    let receipt = Receipt::new(
        TEST_NONCE,
        DEFAULT_PROMPT,
        MODEL,
        DEFAULT_STEPS as u32,
        generated.token_count as u32,
        token_bytes,
    );
    let receipt_bytes = receipt.encode();
    let signature = sign_receipt(&receipt, &TEST_SIGNING_KEY).expect("test key signs receipts");
    let public_key = public_key(&TEST_SIGNING_KEY);

    debug_println!("MODEL SHA256 {}", Hex(&receipt.weights_digest));
    debug_println!("TOKENS {}", output);
    debug_println!("generated {} tokens", generated.token_count);
    debug_println!("TOKEN IDS HEX {}", Hex(token_bytes));
    debug_println!("TOKENS SHA256 {}", Hex(&output_digest));
    debug_println!("RECEIPT HEX {}", Hex(&receipt_bytes));
    debug_println!("SIGNATURE HEX {}", Hex(&signature));
    debug_println!("PUBLIC KEY HEX {}", Hex(&public_key));
    debug_println!("LLMDEMO PASS");
    NullHandler::new()
}
