//! Shared deterministic demo runner used by the host binary, QEMU PD, and
//! receipt verifier. It stays allocation-free: callers provide every buffer.

use rpi4_llm_loader::GgufError;

use crate::{ArenaPlan, Engine, EngineError, VocabEntry};

pub const DEFAULT_PROMPT: &[u8] = b"One day, Tom the cat";
pub const DEFAULT_STEPS: usize = 64;
pub const EXPECTED_TOKENS_SHA256: [u8; 32] = [
    0x7b, 0x2b, 0x33, 0x32, 0x3c, 0xba, 0x78, 0xf9, 0x0b, 0x50, 0xf6, 0xac, 0x02, 0xd9, 0x80, 0xf4,
    0x6c, 0x7e, 0x59, 0x20, 0xf1, 0xd0, 0x0b, 0xa2, 0xef, 0x73, 0x6e, 0x2f, 0xe6, 0x4e, 0x6d, 0xce,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generated {
    pub token_count: usize,
    pub text_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunError {
    Loader(GgufError),
    Engine(EngineError),
    OutputTokenBufferTooSmall,
    OutputTextBufferTooSmall,
    TokenByteBufferTooSmall,
}

pub struct RunBuffers<'a> {
    pub arena: &'a mut [f32],
    pub vocab_arena: &'a mut [VocabEntry],
    pub prompt_ids: &'a mut [u32],
    pub output_ids: &'a mut [u32],
    pub output_text: &'a mut [u8],
}

impl From<GgufError> for RunError {
    fn from(value: GgufError) -> Self {
        Self::Loader(value)
    }
}

impl From<EngineError> for RunError {
    fn from(value: EngineError) -> Self {
        Self::Engine(value)
    }
}

pub fn generate_into(
    model: &[u8],
    prompt: &[u8],
    steps: usize,
    buffers: RunBuffers<'_>,
) -> Result<Generated, RunError> {
    let desc = rpi4_llm_loader::parse(model)?;
    let plan = ArenaPlan::for_model(&desc)?;
    if buffers.arena.len() < plan.f32_len {
        return Err(RunError::Engine(EngineError::ArenaTooSmall));
    }
    if buffers.vocab_arena.len() < plan.vocab_len {
        return Err(RunError::Engine(EngineError::VocabArenaTooSmall));
    }

    let mut engine = Engine::new(&desc, model, buffers.arena, buffers.vocab_arena)?;
    let config = *engine.config();
    let n_prompt = engine.encode(prompt, buffers.prompt_ids)?;
    let total = n_prompt
        .saturating_sub(1)
        .saturating_add(steps)
        .min(config.seq_len as usize);

    let mut token_count = 0usize;
    let mut text_len = 0usize;
    let mut piece = [0u8; 128];
    let mut next = buffers.prompt_ids[0];

    for pos in 0..total {
        engine.forward(next, pos as u32)?;
        next = if pos + 1 < n_prompt {
            buffers.prompt_ids[pos + 1]
        } else {
            let id = engine.argmax_logits();
            if id == config.eos_id {
                break;
            }
            let Some(slot) = buffers.output_ids.get_mut(token_count) else {
                return Err(RunError::OutputTokenBufferTooSmall);
            };
            *slot = id;
            token_count += 1;

            let n = engine.decode(id, &mut piece);
            let end = text_len
                .checked_add(n)
                .ok_or(RunError::OutputTextBufferTooSmall)?;
            let Some(dst) = buffers.output_text.get_mut(text_len..end) else {
                return Err(RunError::OutputTextBufferTooSmall);
            };
            dst.copy_from_slice(&piece[..n]);
            text_len = end;
            id
        };
    }

    Ok(Generated {
        token_count,
        text_len,
    })
}

pub fn token_ids_to_le_bytes(ids: &[u32], out: &mut [u8]) -> Result<usize, RunError> {
    let len = ids
        .len()
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or(RunError::TokenByteBufferTooSmall)?;
    let Some(out) = out.get_mut(..len) else {
        return Err(RunError::TokenByteBufferTooSmall);
    };
    let (chunks, remainder) = out.as_chunks_mut::<4>();
    if !remainder.is_empty() {
        return Err(RunError::TokenByteBufferTooSmall);
    }
    for (chunk, id) in chunks.iter_mut().zip(ids) {
        chunk.copy_from_slice(&id.to_le_bytes());
    }
    Ok(len)
}
