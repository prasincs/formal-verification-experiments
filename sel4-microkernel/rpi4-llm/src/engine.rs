//! The forward pass: llama2.c structure (RMSNorm → RoPE attention with a
//! KV cache and GQA → SwiGLU FFN), reading F32 weights in place from the
//! model buffer, all state in caller-provided arenas.

use rpi4_llm_loader::bounds::GGML_TYPE_F32;
use rpi4_llm_loader::gguf::MAX_NAME;
use rpi4_llm_loader::{ModelDescriptor, TensorDesc};

use crate::math;
use crate::tokenizer::{self, VocabEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineError {
    /// A weight tensor is not F32 (quantized kernels are a follow-up).
    UnsupportedQuant,
    /// The float arena is smaller than [`ArenaPlan::f32_len`].
    ArenaTooSmall,
    /// The vocab arena is smaller than [`ArenaPlan::vocab_len`].
    VocabArenaTooSmall,
    /// Arena sizing overflowed (descriptor caps make this unreachable for
    /// accepted models; checked anyway).
    PlanOverflow,
    /// The prompt does not fit the token buffer or context window.
    PromptTooLong,
    /// The context window is exhausted.
    ContextOverflow,
    /// A prompt byte has no piece and no byte-fallback token.
    TokenizerFailure,
    /// A tensor vanished between validation and binding (foreign buffer).
    BadDescriptor,
}

/// Arena sizes computed from validated descriptor fields with checked
/// arithmetic. `f32_len` counts f32 slots; `vocab_len` counts
/// [`VocabEntry`] slots.
#[derive(Clone, Copy, Debug)]
pub struct ArenaPlan {
    pub f32_len: usize,
    pub vocab_len: usize,
}

impl ArenaPlan {
    pub fn for_model(desc: &ModelDescriptor) -> Result<ArenaPlan, EngineError> {
        let c = &desc.config;
        let dim = c.dim as usize;
        let hidden = c.hidden_dim as usize;
        let layers = c.n_layers as usize;
        let seq = c.seq_len as usize;
        let heads = c.n_heads as usize;
        let kv_dim = (c.head_size as usize)
            .checked_mul(c.n_kv_heads as usize)
            .ok_or(EngineError::PlanOverflow)?;
        let vocab = c.vocab_size as usize;

        let kv_cache = layers
            .checked_mul(seq)
            .and_then(|v| v.checked_mul(kv_dim))
            .ok_or(EngineError::PlanOverflow)?;
        let att = heads.checked_mul(seq).ok_or(EngineError::PlanOverflow)?;

        // x, xb, xb2, hb, hb2, q, att, logits, key cache, value cache
        let mut total = 0usize;
        for part in [
            dim, dim, dim, hidden, hidden, dim, att, vocab, kv_cache, kv_cache,
        ] {
            total = total.checked_add(part).ok_or(EngineError::PlanOverflow)?;
        }
        Ok(ArenaPlan {
            f32_len: total,
            vocab_len: vocab,
        })
    }
}

/// Per-layer weight views (byte offsets into the model buffer resolved to
/// slices once, at construction).
struct LayerWeights<'a> {
    attn_norm: &'a [u8],
    wq: &'a [u8],
    wk: &'a [u8],
    wv: &'a [u8],
    wo: &'a [u8],
    ffn_norm: &'a [u8],
    w_gate: &'a [u8],
    w_down: &'a [u8],
    w_up: &'a [u8],
}

const MAX_LAYERS: usize = rpi4_llm_loader::gguf::MAX_LAYERS as usize;

pub struct Engine<'a> {
    desc: &'a ModelDescriptor,
    buf: &'a [u8],
    vocab: &'a [VocabEntry],

    token_embd: &'a [u8],
    output_norm: &'a [u8],
    output: &'a [u8],
    layers: [Option<LayerWeights<'a>>; MAX_LAYERS],

    // activations
    x: &'a mut [f32],
    xb: &'a mut [f32],
    xb2: &'a mut [f32],
    hb: &'a mut [f32],
    hb2: &'a mut [f32],
    q: &'a mut [f32],
    att: &'a mut [f32],
    logits: &'a mut [f32],
    key_cache: &'a mut [f32],
    value_cache: &'a mut [f32],
}

fn f32_tensor<'a>(
    desc: &ModelDescriptor,
    buf: &'a [u8],
    t: &TensorDesc,
) -> Result<&'a [u8], EngineError> {
    if t.ty != GGML_TYPE_F32 {
        return Err(EngineError::UnsupportedQuant);
    }
    desc.tensor_data(buf, t).ok_or(EngineError::BadDescriptor)
}

fn named<'a>(desc: &'a ModelDescriptor, name: &[u8]) -> Result<&'a TensorDesc, EngineError> {
    desc.find(name).ok_or(EngineError::BadDescriptor)
}

/// `blk.{i}{suffix}` without alloc (layer count is capped at two digits).
fn layer_name(out: &mut [u8; MAX_NAME], layer: u32, suffix: &[u8]) -> usize {
    let mut n = 0;
    for &b in b"blk." {
        out[n] = b;
        n += 1;
    }
    if layer >= 10 {
        out[n] = b'0' + (layer / 10) as u8;
        n += 1;
    }
    out[n] = b'0' + (layer % 10) as u8;
    n += 1;
    for &b in suffix {
        out[n] = b;
        n += 1;
    }
    n
}

impl<'a> Engine<'a> {
    /// Bind a validated descriptor + model buffer to caller-provided
    /// arenas. Fails closed on non-F32 tensors or undersized arenas.
    pub fn new(
        desc: &'a ModelDescriptor,
        buf: &'a [u8],
        arena: &'a mut [f32],
        vocab_arena: &'a mut [VocabEntry],
    ) -> Result<Engine<'a>, EngineError> {
        let plan = ArenaPlan::for_model(desc)?;
        if arena.len() < plan.f32_len {
            return Err(EngineError::ArenaTooSmall);
        }
        if vocab_arena.len() < plan.vocab_len {
            return Err(EngineError::VocabArenaTooSmall);
        }

        let c = &desc.config;
        let dim = c.dim as usize;
        let hidden = c.hidden_dim as usize;
        let seq = c.seq_len as usize;
        let kv_dim = c.head_size as usize * c.n_kv_heads as usize;
        let kv_cache_len = c.n_layers as usize * seq * kv_dim;

        let token_embd = f32_tensor(desc, buf, named(desc, b"token_embd.weight")?)?;
        let output_norm = f32_tensor(desc, buf, named(desc, b"output_norm.weight")?)?;
        let output = if desc.tied_output {
            token_embd
        } else {
            f32_tensor(desc, buf, named(desc, b"output.weight")?)?
        };

        let mut layers: [Option<LayerWeights<'a>>; MAX_LAYERS] = [const { None }; MAX_LAYERS];
        let mut name = [0u8; MAX_NAME];
        for l in 0..c.n_layers {
            let mut get = |suffix: &[u8]| -> Result<&'a [u8], EngineError> {
                let n = layer_name(&mut name, l, suffix);
                f32_tensor(desc, buf, named(desc, &name[..n])?)
            };
            layers[l as usize] = Some(LayerWeights {
                attn_norm: get(b".attn_norm.weight")?,
                wq: get(b".attn_q.weight")?,
                wk: get(b".attn_k.weight")?,
                wv: get(b".attn_v.weight")?,
                wo: get(b".attn_output.weight")?,
                ffn_norm: get(b".ffn_norm.weight")?,
                w_gate: get(b".ffn_gate.weight")?,
                w_down: get(b".ffn_down.weight")?,
                w_up: get(b".ffn_up.weight")?,
            });
        }

        let (x, rest) = arena.split_at_mut(dim);
        let (xb, rest) = rest.split_at_mut(dim);
        let (xb2, rest) = rest.split_at_mut(dim);
        let (hb, rest) = rest.split_at_mut(hidden);
        let (hb2, rest) = rest.split_at_mut(hidden);
        let (q, rest) = rest.split_at_mut(dim);
        let (att, rest) = rest.split_at_mut(c.n_heads as usize * seq);
        let (logits, rest) = rest.split_at_mut(c.vocab_size as usize);
        let (key_cache, rest) = rest.split_at_mut(kv_cache_len);
        let (value_cache, _) = rest.split_at_mut(kv_cache_len);

        let vocab = &mut vocab_arena[..plan.vocab_len];
        tokenizer::build_index(desc, buf, vocab);

        Ok(Engine {
            desc,
            buf,
            vocab,
            token_embd,
            output_norm,
            output,
            layers,
            x,
            xb,
            xb2,
            hb,
            hb2,
            q,
            att,
            logits,
            key_cache,
            value_cache,
        })
    }

    pub fn config(&self) -> &rpi4_llm_loader::LlamaConfig {
        &self.desc.config
    }

    /// Encode a prompt (BOS included) into `out`; returns the token count.
    pub fn encode(&self, text: &[u8], out: &mut [u32]) -> Result<usize, EngineError> {
        tokenizer::encode(self.desc, self.buf, self.vocab, text, out)
    }

    /// Decoded bytes of token `id`, written into `out`.
    pub fn decode(&self, id: u32, out: &mut [u8; 128]) -> usize {
        tokenizer::decode(self.buf, self.vocab, id, out)
    }

    /// Greedy argmax over the current logits.
    pub fn argmax_logits(&self) -> u32 {
        math::argmax(self.logits)
    }

    /// Run one token through the model at position `pos`; returns the
    /// logits over the vocabulary.
    pub fn forward(&mut self, token: u32, pos: u32) -> Result<&[f32], EngineError> {
        let c = &self.desc.config;
        if pos >= c.seq_len {
            return Err(EngineError::ContextOverflow);
        }
        if token >= c.vocab_size {
            return Err(EngineError::BadDescriptor);
        }
        let dim = c.dim as usize;
        let head = c.head_size as usize;
        let heads = c.n_heads as usize;
        let kv_heads = c.n_kv_heads as usize;
        let kv_dim = head * kv_heads;
        let seq = c.seq_len as usize;
        let rep = heads / kv_heads;
        let pos_u = pos as usize;

        // token embedding row
        let row = &self.token_embd[token as usize * dim * 4..][..dim * 4];
        for (o, b) in self.x.iter_mut().zip(row.as_chunks::<4>().0) {
            *o = f32::from_le_bytes(*b);
        }

        for l in 0..c.n_layers as usize {
            let w = self.layers[l].as_ref().ok_or(EngineError::BadDescriptor)?;
            let (k_cache, v_cache) = {
                let base = l * seq * kv_dim + pos_u * kv_dim;
                (
                    &mut self.key_cache[base..base + kv_dim],
                    &mut self.value_cache[base..base + kv_dim],
                )
            };

            // attention rmsnorm
            math::rmsnorm(self.xb, self.x, w.attn_norm, c.rms_eps);

            // qkv
            math::matmul(self.q, w.wq, self.xb);
            math::matmul(k_cache, w.wk, self.xb);
            math::matmul(v_cache, w.wv, self.xb);

            // RoPE on q and this position's k
            for h in 0..heads {
                math::rope_rotate(&mut self.q[h * head..(h + 1) * head], pos, c.rope_theta);
            }
            for h in 0..kv_heads {
                math::rope_rotate(&mut k_cache[h * head..(h + 1) * head], pos, c.rope_theta);
            }

            // attention per head over cached positions 0..=pos
            let inv_sqrt = 1.0 / libm::sqrtf(head as f32);
            for h in 0..heads {
                let kv_h = h / rep;
                let q_h = &self.q[h * head..(h + 1) * head];
                let att = &mut self.att[h * seq..h * seq + pos_u + 1];
                for (t, a) in att.iter_mut().enumerate() {
                    let k_row =
                        &self.key_cache[l * seq * kv_dim + t * kv_dim + kv_h * head..][..head];
                    let mut score = 0.0f32;
                    for (qv, kv) in q_h.iter().zip(k_row) {
                        score += qv * kv;
                    }
                    *a = score * inv_sqrt;
                }
                math::softmax(att);
                let out = &mut self.xb[h * head..(h + 1) * head];
                out.fill(0.0);
                for (t, a) in att.iter().enumerate() {
                    let v_row =
                        &self.value_cache[l * seq * kv_dim + t * kv_dim + kv_h * head..][..head];
                    for (o, vv) in out.iter_mut().zip(v_row) {
                        *o += a * vv;
                    }
                }
            }

            // attention output + residual
            math::matmul(self.xb2, w.wo, self.xb);
            for (xv, dv) in self.x.iter_mut().zip(self.xb2.iter()) {
                *xv += dv;
            }

            // ffn rmsnorm + SwiGLU + residual
            math::rmsnorm(self.xb, self.x, w.ffn_norm, c.rms_eps);
            math::matmul(self.hb, w.w_gate, self.xb);
            math::matmul(self.hb2, w.w_up, self.xb);
            for (g, u) in self.hb.iter_mut().zip(self.hb2.iter()) {
                *g = math::silu(*g) * u;
            }
            math::matmul(self.xb2, w.w_down, self.hb);
            for (xv, dv) in self.x.iter_mut().zip(self.xb2.iter()) {
                *xv += dv;
            }
        }

        // final norm + classifier
        math::rmsnorm(self.xb, self.x, self.output_norm, c.rms_eps);
        math::matmul(self.logits, self.output, self.xb);
        Ok(self.logits)
    }
}
