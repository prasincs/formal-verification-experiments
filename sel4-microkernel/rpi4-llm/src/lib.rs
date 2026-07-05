//! WP-6: arena-based llama2.c-style inference engine (`no_std`).
//!
//! Design rules, following `docs/decoder-allocation-security.md` and the
//! design doc's Route B:
//!
//! - **No allocator.** Every buffer the forward pass touches — activations,
//!   attention scores, logits, KV cache, the tokenizer index — is carved
//!   from caller-provided arenas whose sizes come from [`ArenaPlan`],
//!   computed with checked arithmetic from the *validated*
//!   [`ModelDescriptor`] only. A PD backs these with static memory; the
//!   host demo uses two `Vec`s.
//! - **Single-threaded, fixed evaluation order, pinned math.** Transcendental
//!   functions come from an exactly-pinned pure-Rust `libm`; no SIMD
//!   intrinsics, no FMA contraction, greedy (argmax, lowest-index-wins)
//!   sampling. Bit-identical output across runs and hosts is a *tested
//!   property* (the demo and CI assert an output hash), conditional on the
//!   pins the design doc enumerates — not an assumption.
//! - **The hot loops stay ordinary Rust.** Verus proves the loader's
//!   envelope (bounds, sizes, totality); the linear algebra is covered by
//!   the reference-implementation cross-check in
//!   `fixtures/reference_infer.py` and the pinned-hash tests. Honest
//!   division of labor, same as the repo's crypto rule.
//!
//! Engine limits (documented): F32 tensors only (quantized kernels are a
//! follow-up), greedy sampling, whatever tokenizer the checkpoint carries
//! (SentencePiece-style greedy BPE with byte fallback).

#![no_std]
#![forbid(unsafe_code)]

mod engine;
mod math;
pub mod receipt;
pub mod run;
mod tokenizer;

pub use engine::{ArenaPlan, Engine, EngineError};
pub use run::{
    generate_into, token_ids_to_le_bytes, Generated, RunBuffers, RunError, DEFAULT_PROMPT,
    DEFAULT_STEPS, EXPECTED_TOKENS_SHA256,
};
pub use tokenizer::VocabEntry;
