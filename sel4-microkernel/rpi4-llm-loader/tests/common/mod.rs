//! Minimal GGUF builder for loader tests: emits a structurally valid,
//! tiny llama file whose every field the tests can then corrupt.

pub const ALIGN: usize = 32;

// Test-model shape (all constraints satisfied: dim % heads == 0, even head
// size, heads % kv_heads == 0, vocab >= 3).
pub const DIM: u32 = 8;
pub const HIDDEN: u32 = 16;
pub const LAYERS: u32 = 1;
pub const HEADS: u32 = 2;
pub const KV_HEADS: u32 = 1;
pub const VOCAB: u32 = 8;
pub const SEQ: u32 = 8;

pub fn s(b: &[u8]) -> Vec<u8> {
    let mut v = (b.len() as u64).to_le_bytes().to_vec();
    v.extend_from_slice(b);
    v
}

pub fn kv_str(key: &[u8], val: &[u8]) -> Vec<u8> {
    let mut v = s(key);
    v.extend_from_slice(&8u32.to_le_bytes()); // T_STR
    v.extend_from_slice(&s(val));
    v
}

pub fn kv_u32(key: &[u8], val: u32) -> Vec<u8> {
    let mut v = s(key);
    v.extend_from_slice(&4u32.to_le_bytes()); // T_U32
    v.extend_from_slice(&val.to_le_bytes());
    v
}

pub fn kv_f32(key: &[u8], val: f32) -> Vec<u8> {
    let mut v = s(key);
    v.extend_from_slice(&6u32.to_le_bytes()); // T_F32
    v.extend_from_slice(&val.to_le_bytes());
    v
}

pub fn kv_tokens(key: &[u8], tokens: &[&[u8]]) -> Vec<u8> {
    let mut v = s(key);
    v.extend_from_slice(&9u32.to_le_bytes()); // T_ARR
    v.extend_from_slice(&8u32.to_le_bytes()); // elem T_STR
    v.extend_from_slice(&(tokens.len() as u64).to_le_bytes());
    for t in tokens {
        v.extend_from_slice(&s(t));
    }
    v
}

pub fn kv_scores(key: &[u8], scores: &[f32]) -> Vec<u8> {
    let mut v = s(key);
    v.extend_from_slice(&9u32.to_le_bytes()); // T_ARR
    v.extend_from_slice(&6u32.to_le_bytes()); // elem T_F32
    v.extend_from_slice(&(scores.len() as u64).to_le_bytes());
    for x in scores {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v
}

pub struct TensorSpec {
    pub name: Vec<u8>,
    pub dims: Vec<u64>,
    pub ty: u32,
}

pub fn llama_tensor_specs() -> Vec<TensorSpec> {
    let t = |name: &str, dims: &[u64]| TensorSpec {
        name: name.as_bytes().to_vec(),
        dims: dims.to_vec(),
        ty: 0, // F32
    };
    let d = DIM as u64;
    let h = HIDDEN as u64;
    let v = VOCAB as u64;
    let kv = (DIM / HEADS * KV_HEADS) as u64;
    let mut out = vec![t("token_embd.weight", &[d, v])];
    for l in 0..LAYERS {
        let p = |suf: &str| format!("blk.{l}.{suf}");
        out.push(t(&p("attn_norm.weight"), &[d]));
        out.push(t(&p("attn_q.weight"), &[d, d]));
        out.push(t(&p("attn_k.weight"), &[d, kv]));
        out.push(t(&p("attn_v.weight"), &[d, kv]));
        out.push(t(&p("attn_output.weight"), &[d, d]));
        out.push(t(&p("ffn_norm.weight"), &[d]));
        out.push(t(&p("ffn_gate.weight"), &[d, h]));
        out.push(t(&p("ffn_down.weight"), &[h, d]));
        out.push(t(&p("ffn_up.weight"), &[d, h]));
    }
    out.push(t("output_norm.weight", &[d]));
    out.push(t("output.weight", &[d, v]));
    out
}

pub fn default_meta() -> Vec<Vec<u8>> {
    let tokens: Vec<&[u8]> = vec![b"<unk>", b"<s>", b"</s>", b"a", b"b", b"c", b"d", b"e"];
    vec![
        kv_str(b"general.architecture", b"llama"),
        kv_u32(b"general.alignment", ALIGN as u32),
        kv_u32(b"llama.embedding_length", DIM),
        kv_u32(b"llama.feed_forward_length", HIDDEN),
        kv_u32(b"llama.block_count", LAYERS),
        kv_u32(b"llama.attention.head_count", HEADS),
        kv_u32(b"llama.attention.head_count_kv", KV_HEADS),
        kv_u32(b"llama.context_length", SEQ),
        kv_f32(b"llama.attention.layer_norm_rms_epsilon", 1e-5),
        kv_tokens(b"tokenizer.ggml.tokens", &tokens),
        kv_scores(b"tokenizer.ggml.scores", &[0.0; 8]),
        kv_u32(b"tokenizer.ggml.bos_token_id", 1),
        kv_u32(b"tokenizer.ggml.eos_token_id", 2),
    ]
}

/// Assemble a GGUF file from metadata entries and tensor specs, filling
/// tensor data with a byte pattern. Offsets are laid out contiguously with
/// alignment padding, matching what llama.cpp writes.
pub fn build_gguf(meta: &[Vec<u8>], tensors: &[TensorSpec]) -> Vec<u8> {
    let mut out = b"GGUF".to_vec();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&(tensors.len() as u64).to_le_bytes());
    out.extend_from_slice(&(meta.len() as u64).to_le_bytes());
    for m in meta {
        out.extend_from_slice(m);
    }
    let mut offset = 0u64;
    let mut sizes = Vec::new();
    for t in tensors {
        out.extend_from_slice(&s(&t.name));
        out.extend_from_slice(&(t.dims.len() as u32).to_le_bytes());
        for d in &t.dims {
            out.extend_from_slice(&d.to_le_bytes());
        }
        out.extend_from_slice(&t.ty.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        let nelems: u64 = t.dims.iter().product();
        let size = match t.ty {
            0 => nelems * 4,
            1 => nelems * 2,
            2 => nelems / 32 * 18,
            8 => nelems / 32 * 34,
            _ => nelems * 4, // for bad-type tests; size is irrelevant
        };
        sizes.push(size);
        offset += size;
        offset = offset.div_ceil(ALIGN as u64) * ALIGN as u64;
    }
    // pad to data start
    while !out.len().is_multiple_of(ALIGN) {
        out.push(0);
    }
    for (i, size) in sizes.iter().enumerate() {
        out.extend(core::iter::repeat_n((i % 251) as u8, *size as usize));
        while !out.len().is_multiple_of(ALIGN) {
            out.push(0);
        }
    }
    out
}

pub fn valid_gguf() -> Vec<u8> {
    build_gguf(&default_meta(), &llama_tensor_specs())
}
