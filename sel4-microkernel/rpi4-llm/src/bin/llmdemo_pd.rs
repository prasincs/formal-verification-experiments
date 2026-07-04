#![no_std]
#![no_main]

use core::fmt;

use rpi4_llm::{
    build_demo_gguf, execute, test_public_key, FIXED_PROMPT, MODEL_CAPACITY, TEST_NONCE,
    TEST_SIGNING_KEY,
};
use sel4_microkit::{debug_println, protection_domain, NullHandler};

static mut MODEL: [u8; MODEL_CAPACITY] = [0; MODEL_CAPACITY];

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
    let model = unsafe { &mut *core::ptr::addr_of_mut!(MODEL) };
    let length = build_demo_gguf(model).expect("fixed demo model buffer must fit");
    let execution = execute(
        &model[..length],
        FIXED_PROMPT,
        0,
        TEST_NONCE,
        TEST_SIGNING_KEY,
    )
    .expect("verified demo model must execute");
    let receipt = execution.receipt.encode();
    let output_text = core::str::from_utf8(&execution.output).expect("demo tokens are ASCII");

    debug_println!("TOKENS {}", output_text);
    debug_println!("TOKENS HEX {}", Hex(&execution.output));
    debug_println!("TOKENS SHA256 {}", Hex(&execution.receipt.output_digest));
    debug_println!("RECEIPT HEX {}", Hex(&receipt));
    debug_println!("SIGNATURE HEX {}", Hex(&execution.signature));
    debug_println!("PUBLIC KEY HEX {}", Hex(&test_public_key()));
    debug_println!("LLMDEMO PASS");
    NullHandler::new()
}
