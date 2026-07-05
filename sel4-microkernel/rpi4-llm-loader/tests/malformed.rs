//! Malformed-GGUF sweep: every corruption class the workplan names
//! (truncated, oversized lengths, overlapping tensors) plus the rest of
//! the error surface — each rejected cleanly with the expected error.

mod common;

use common::*;
use rpi4_llm_loader::{parse, GgufError};

#[test]
fn accepts_valid_file() {
    let buf = valid_gguf();
    let d = parse(&buf).expect("valid gguf must parse");
    assert_eq!(d.config.dim, DIM);
    assert_eq!(d.config.hidden_dim, HIDDEN);
    assert_eq!(d.config.n_layers, LAYERS);
    assert_eq!(d.config.n_heads, HEADS);
    assert_eq!(d.config.n_kv_heads, KV_HEADS);
    assert_eq!(d.config.vocab_size, VOCAB);
    assert_eq!(d.config.head_size, DIM / HEADS);
    assert_eq!(d.config.bos_id, 1);
    assert_eq!(d.config.eos_id, 2);
    assert!(!d.tied_output);
    assert_eq!(d.tensors().len(), 12);
    // vocabulary iterates fully and in order
    let toks: Vec<&[u8]> = d.tokens(&buf).collect();
    assert_eq!(toks.len(), VOCAB as usize);
    assert_eq!(toks[1], b"<s>");
    assert_eq!(toks[3], b"a");
    // every tensor's data is reachable and correctly sized
    for t in d.tensors() {
        let data = d.tensor_data(&buf, t).expect("tensor data in bounds");
        assert_eq!(data.len() as u64, t.size);
    }
}

#[test]
fn accepts_tied_output() {
    let mut specs = llama_tensor_specs();
    specs.retain(|t| t.name != b"output.weight");
    let buf = build_gguf(&default_meta(), &specs);
    let d = parse(&buf).expect("tied-output model must parse");
    assert!(d.tied_output);
}

#[test]
fn rejects_truncation_everywhere() {
    // Every strict prefix of the file must fail closed. (The data region
    // is checked via tensor bounds, so any cut inside it must also fail.)
    let buf = valid_gguf();
    for n in 0..buf.len() {
        assert!(
            parse(&buf[..n]).is_err(),
            "prefix of length {n} unexpectedly accepted"
        );
    }
}

#[test]
fn rejects_bad_magic_and_version() {
    let mut buf = valid_gguf();
    buf[0] = b'X';
    assert_eq!(parse(&buf), Err(GgufError::BadMagic));

    let mut buf = valid_gguf();
    buf[4..8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(parse(&buf), Err(GgufError::UnsupportedVersion));
}

#[test]
fn rejects_absurd_counts() {
    // tensor_count lies live at offset 8, kv_count at 16.
    let mut buf = valid_gguf();
    buf[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(parse(&buf), Err(GgufError::TooManyTensors));

    let mut buf = valid_gguf();
    buf[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(parse(&buf), Err(GgufError::TooManyKeys));
}

#[test]
fn rejects_wrong_architecture() {
    let mut meta = default_meta();
    meta[0] = kv_str(b"general.architecture", b"gpt2");
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::UnsupportedArchitecture));

    // missing entirely
    let meta2: Vec<Vec<u8>> = default_meta().into_iter().skip(1).collect();
    let buf = build_gguf(&meta2, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::UnsupportedArchitecture));
}

#[test]
fn rejects_bad_alignment() {
    for bad in [0u32, 4, 33, 1 << 20] {
        let mut meta = default_meta();
        meta[1] = kv_u32(b"general.alignment", bad);
        let buf = build_gguf(&meta, &llama_tensor_specs());
        assert_eq!(parse(&buf), Err(GgufError::BadAlignment), "alignment {bad}");
    }
}

#[test]
fn rejects_inconsistent_hyperparameters() {
    // dim not divisible by heads
    let mut meta = default_meta();
    meta[5] = kv_u32(b"llama.attention.head_count", 3);
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::BadHyperparameter));

    // kv_heads > heads
    let mut meta = default_meta();
    meta[6] = kv_u32(b"llama.attention.head_count_kv", 4);
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::BadHyperparameter));

    // zero layers
    let mut meta = default_meta();
    meta[4] = kv_u32(b"llama.block_count", 0);
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::BadHyperparameter));

    // missing required hyperparameter
    let meta: Vec<Vec<u8>> = default_meta()
        .into_iter()
        .enumerate()
        .filter(|(i, _)| *i != 2) // drop embedding_length
        .map(|(_, m)| m)
        .collect();
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::MissingHyperparameter));
}

#[test]
fn rejects_bad_float_hyperparameters() {
    for bad in [f32::NAN, 0.0, -1e-5, 2.0] {
        let mut meta = default_meta();
        meta[8] = kv_f32(b"llama.attention.layer_norm_rms_epsilon", bad);
        let buf = build_gguf(&meta, &llama_tensor_specs());
        assert_eq!(parse(&buf), Err(GgufError::BadFloatValue));
    }
}

#[test]
fn rejects_tokenizer_problems() {
    // missing tokens array
    let meta: Vec<Vec<u8>> = default_meta()
        .into_iter()
        .enumerate()
        .filter(|(i, _)| *i != 9)
        .map(|(_, m)| m)
        .collect();
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::MissingTokenizer));

    // scores count differs from token count
    let mut meta = default_meta();
    meta[10] = kv_scores(b"tokenizer.ggml.scores", &[0.0; 7]);
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::ScoresMismatch));

    // BOS outside the vocabulary
    let mut meta = default_meta();
    meta[11] = kv_u32(b"tokenizer.ggml.bos_token_id", 8);
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::BadSpecialToken));

    // a token longer than MAX_TOKEN_BYTES
    let long = vec![b'x'; 129];
    let tokens: Vec<&[u8]> = vec![b"<unk>", b"<s>", b"</s>", &long, b"b", b"c", b"d", b"e"];
    let mut meta = default_meta();
    meta[9] = kv_tokens(b"tokenizer.ggml.tokens", &tokens);
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::TokenTooLong));
}

#[test]
fn rejects_unknown_and_smuggled_value_types() {
    // unknown scalar value type tag on an unknown key
    let mut kv = s(b"custom.key");
    kv.extend_from_slice(&99u32.to_le_bytes());
    let mut meta = default_meta();
    meta.push(kv);
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::BadValueType));

    // nested array
    let mut kv = s(b"custom.nested");
    kv.extend_from_slice(&9u32.to_le_bytes()); // T_ARR
    kv.extend_from_slice(&9u32.to_le_bytes()); // elem T_ARR
    kv.extend_from_slice(&1u64.to_le_bytes());
    let mut meta = default_meta();
    meta.push(kv);
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::NestedArray));

    // array whose declared count cannot fit in the buffer
    let mut kv = s(b"custom.big");
    kv.extend_from_slice(&9u32.to_le_bytes()); // T_ARR
    kv.extend_from_slice(&4u32.to_le_bytes()); // elem T_U32
    kv.extend_from_slice(&u64::MAX.to_le_bytes());
    let mut meta = default_meta();
    meta.push(kv);
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::ArrayOutOfBounds));

    // known key carrying the wrong type
    let mut meta = default_meta();
    meta[2] = kv_str(b"llama.embedding_length", b"8");
    let buf = build_gguf(&meta, &llama_tensor_specs());
    assert_eq!(parse(&buf), Err(GgufError::BadMetadataType));
}

/// Locate the byte offset of a tensor's info record fields within the
/// header, so tests can corrupt them surgically: returns the offset just
/// past the name (i.e. of `n_dims`).
fn tensor_info_field_offset(buf: &[u8], name: &[u8]) -> usize {
    // find the name preceded by its u64 length
    let needle_len = (name.len() as u64).to_le_bytes();
    let mut i = 24;
    while i + 8 + name.len() <= buf.len() {
        if buf[i..i + 8] == needle_len && &buf[i + 8..i + 8 + name.len()] == name {
            return i + 8 + name.len();
        }
        i += 1;
    }
    panic!("tensor {name:?} not found");
}

#[test]
fn rejects_tensor_table_lies() {
    let base = valid_gguf();

    // unknown quantization type
    let mut buf = base.clone();
    let off = tensor_info_field_offset(&buf, b"output.weight");
    let ty_off = off + 4 + 16; // n_dims + two u64 dims
    buf[ty_off..ty_off + 4].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(parse(&buf), Err(GgufError::BadTensorType));

    // misaligned offset
    let mut buf = base.clone();
    let data_off = ty_off + 4;
    let cur = u64::from_le_bytes(buf[data_off..data_off + 8].try_into().unwrap());
    buf[data_off..data_off + 8].copy_from_slice(&(cur + 1).to_le_bytes());
    assert_eq!(parse(&buf), Err(GgufError::MisalignedTensor));

    // offset pointing past the data region
    let mut buf = base.clone();
    buf[data_off..data_off + 8].copy_from_slice(&(1u64 << 40).to_le_bytes());
    assert_eq!(parse(&buf), Err(GgufError::TensorOutOfBounds));

    // overlapping another tensor
    let mut buf = base.clone();
    buf[data_off..data_off + 8].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(parse(&buf), Err(GgufError::OverlappingTensors));

    // zero dimension
    let mut buf = base.clone();
    let dims_off = off + 4;
    buf[dims_off..dims_off + 8].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(parse(&buf), Err(GgufError::BadDims));

    // dimension-product overflow: dims = [2^40, 2^40]
    let mut buf = base.clone();
    buf[dims_off..dims_off + 8].copy_from_slice(&(1u64 << 40).to_le_bytes());
    buf[dims_off + 8..dims_off + 16].copy_from_slice(&(1u64 << 40).to_le_bytes());
    assert!(matches!(
        parse(&buf),
        Err(GgufError::BadDims) | Err(GgufError::BadTensorSize)
    ));

    // bad dim count
    let mut buf = base.clone();
    buf[off..off + 4].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(parse(&buf), Err(GgufError::BadDimCount));
    let mut buf = base.clone();
    buf[off..off + 4].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(parse(&buf), Err(GgufError::BadDimCount));
}

#[test]
fn rejects_shape_and_set_violations() {
    // a required tensor missing
    let mut specs = llama_tensor_specs();
    specs.retain(|t| t.name != b"blk.0.attn_q.weight");
    let buf = build_gguf(&default_meta(), &specs);
    assert_eq!(parse(&buf), Err(GgufError::MissingTensor));

    // a required tensor with the wrong shape
    let mut specs = llama_tensor_specs();
    for t in &mut specs {
        if t.name == b"blk.0.ffn_down.weight" {
            t.dims = vec![DIM as u64, HIDDEN as u64]; // transposed
        }
    }
    let buf = build_gguf(&default_meta(), &specs);
    assert_eq!(parse(&buf), Err(GgufError::BadTensorShape));

    // duplicate tensor names
    let mut specs = llama_tensor_specs();
    let dup = TensorSpec {
        name: b"output.weight".to_vec(),
        dims: vec![DIM as u64, VOCAB as u64],
        ty: 0,
    };
    specs.push(dup);
    let buf = build_gguf(&default_meta(), &specs);
    assert_eq!(parse(&buf), Err(GgufError::DuplicateTensor));
}

#[test]
fn rejects_trailing_data() {
    let mut buf = valid_gguf();
    buf.extend_from_slice(&[0xAA; 64]);
    assert_eq!(parse(&buf), Err(GgufError::TrailingData));
}

#[test]
fn rejects_random_mutations_cleanly() {
    // Deterministic mini-fuzz: single-byte mutations over the whole file
    // must either parse to a descriptor or return an error — the harness
    // (and the absence of panics) is the assertion. Complements the
    // cargo-fuzz target in fuzz/.
    let base = valid_gguf();
    let mut rng: u64 = 0x5eed;
    for _ in 0..20_000 {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let pos = (rng >> 33) as usize % base.len();
        let val = (rng >> 8) as u8;
        let mut buf = base.clone();
        buf[pos] = val;
        let _ = parse(&buf);
    }
}
