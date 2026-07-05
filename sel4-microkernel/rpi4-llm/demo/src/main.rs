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

use rpi4_llm::{ArenaPlan, Engine, VocabEntry};

fn sha256_hex(data: &[u8]) -> String {
    libcrux_sha2::sha256(data)
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
    let mut prompt = String::from("One day, Tom the cat");
    let mut steps: u32 = 64;
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
        Ok(d) => Box::new(d),
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

    let mut engine = match Engine::new(&desc, &buf, &mut arena, &mut vocab) {
        Ok(e) => e,
        Err(e) => return fail(&format!("engine bind: {e:?}")),
    };

    let mut ids = vec![0u32; c.seq_len as usize];
    let n_prompt = match engine.encode(prompt.as_bytes(), &mut ids) {
        Ok(n) => n,
        Err(e) => return fail(&format!("encode: {e:?}")),
    };

    let total = (n_prompt as u32 + steps).min(c.seq_len);
    let mut generated: Vec<u32> = Vec::new();
    let mut text = Vec::new();
    let mut piece = [0u8; 128];
    let mut next = ids[0];
    for pos in 0..total {
        if let Err(e) = engine.forward(next, pos) {
            return fail(&format!("forward at {pos}: {e:?}"));
        }
        next = if (pos as usize + 1) < n_prompt {
            ids[pos as usize + 1]
        } else {
            let id = engine.argmax_logits();
            if id == c.eos_id {
                break;
            }
            generated.push(id);
            let n = engine.decode(id, &mut piece);
            text.extend_from_slice(&piece[..n]);
            id
        };
    }

    println!("prompt: {prompt:?}");
    println!("output: {:?}", String::from_utf8_lossy(&text));
    println!("generated {} tokens", generated.len());

    let id_bytes: Vec<u8> = generated.iter().flat_map(|t| t.to_le_bytes()).collect();
    let hash = sha256_hex(&id_bytes);
    println!("TOKENS SHA256 {hash}");

    if let Some(want) = expect {
        if want != hash {
            return fail(&format!("hash mismatch: expected {want}"));
        }
        println!("hash matches --expect");
    }
    ExitCode::SUCCESS
}
