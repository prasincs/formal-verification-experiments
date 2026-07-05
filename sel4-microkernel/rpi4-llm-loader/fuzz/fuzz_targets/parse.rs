//! Fuzz the GGUF totality parser: no input may panic it.
//! Run: `cargo +nightly fuzz run parse -- -max_total_time=60`

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(desc) = rpi4_llm_loader::parse(data) {
        // On acceptance, exercise the accessors the engine relies on:
        // they must stay in bounds for whatever the parser let through.
        for t in desc.tensors() {
            let _ = desc.tensor_data(data, t);
        }
        for piece in desc.tokens(data) {
            assert!(piece.len() <= rpi4_llm_loader::gguf::MAX_TOKEN_BYTES);
        }
        for id in 0..desc.tokenizer.count {
            let _ = desc.token_score(data, id);
        }
    }
});
