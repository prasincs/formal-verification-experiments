//! Host llmdemo: exercises the exact loader + engine a PD would link,
//! backing the arenas with heap memory instead of static regions.
//!
//! Prints the generated text and `TOKENS SHA256 <hex>` over the generated
//! token-id stream — the line CI pins (determinism as a tested property).
//!
//! ```text
//! llmdemo-host MODEL.gguf [--prompt TEXT] [--steps N] [--expect HEX]
//! ```

use std::process::ExitCode;

use rpi4_llm::{
    generate_into, token_ids_to_le_bytes, ArenaPlan, RunBuffers, VocabEntry, DEFAULT_PROMPT,
    DEFAULT_STEPS,
};

fn sha256_hex(data: &[u8]) -> String {
    rpi4_llm::receipt::sha256(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("llmdemo-host: {msg}");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(model_path) = args.next() else {
        return fail("usage: llmdemo-host MODEL.gguf [--prompt TEXT] [--steps N] [--expect HEX]");
    };
    let mut prompt = String::from(std::str::from_utf8(DEFAULT_PROMPT).expect("ASCII prompt"));
    let mut steps: usize = DEFAULT_STEPS;
    let mut expect: Option<String> = None;
    while let Some(flag) = args.next() {
        let val = args.next();
        match (flag.as_str(), val) {
            ("--prompt", Some(v)) => prompt = v,
            ("--steps", Some(v)) => match v.parse() {
                Ok(n) => steps = n,
                Err(_) => return fail("--steps expects a number"),
            },
            ("--expect", Some(v)) => expect = Some(v),
            _ => return fail("unknown or valueless flag"),
        }
    }

    let buf = match std::fs::read(&model_path) {
        Ok(b) => b,
        Err(e) => return fail(&format!("reading {model_path}: {e}")),
    };
    println!("MODEL SHA256 {}", sha256_hex(&buf));

    let desc = match rpi4_llm_loader::parse(&buf) {
        Ok(d) => d,
        Err(e) => return fail(&format!("GGUF rejected: {e:?}")),
    };
    let c = desc.config;
    println!(
        "loaded: dim={} hidden={} layers={} heads={} kv_heads={} vocab={} seq_len={} tensors={}",
        c.dim,
        c.hidden_dim,
        c.n_layers,
        c.n_heads,
        c.n_kv_heads,
        c.vocab_size,
        c.seq_len,
        desc.tensors().len()
    );

    let plan = match ArenaPlan::for_model(&desc) {
        Ok(p) => p,
        Err(e) => return fail(&format!("arena plan: {e:?}")),
    };
    println!(
        "arena: {} f32 slots ({} KiB), {} vocab entries",
        plan.f32_len,
        plan.f32_len * 4 / 1024,
        plan.vocab_len
    );
    let mut arena = vec![0.0f32; plan.f32_len];
    let mut vocab = vec![VocabEntry::default(); plan.vocab_len];

    let mut ids = vec![0u32; c.seq_len as usize];
    let mut generated = vec![0u32; steps];
    let mut text = vec![0u8; steps * 128];
    let out = match generate_into(
        &buf,
        prompt.as_bytes(),
        steps,
        RunBuffers {
            arena: &mut arena,
            vocab_arena: &mut vocab,
            prompt_ids: &mut ids,
            output_ids: &mut generated,
            output_text: &mut text,
        },
    ) {
        Ok(out) => out,
        Err(e) => return fail(&format!("generate: {e:?}")),
    };

    println!("prompt: {prompt:?}");
    println!(
        "output: {:?}",
        String::from_utf8_lossy(&text[..out.text_len])
    );
    println!("generated {} tokens", out.token_count);

    let mut id_bytes = vec![0u8; out.token_count * 4];
    let byte_len = match token_ids_to_le_bytes(&generated[..out.token_count], &mut id_bytes) {
        Ok(len) => len,
        Err(e) => return fail(&format!("token-id encoding: {e:?}")),
    };
    let hash = sha256_hex(&id_bytes[..byte_len]);
    println!("TOKENS SHA256 {hash}");

    if let Some(want) = expect {
        if want != hash {
            return fail(&format!("hash mismatch: expected {want}"));
        }
        println!("hash matches --expect");
    }
    ExitCode::SUCCESS
}
