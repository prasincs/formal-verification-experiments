//! WP-6: no_std GGUF model loader with a verified bounds surface.
//!
//! GGUF is the inference PD's scariest input: llama.cpp's own history shows
//! model loaders break in parsing and allocation, not math (heap-overflow
//! CVEs from malformed GGUF are real). This crate applies the repo's
//! decoder discipline to it:
//!
//! - [`bounds`] — the Verus-verified surface. Total little-endian readers,
//!   subslicing, and tensor-size arithmetic: no preconditions, `Option`
//!   results, postconditions proven against specification decodes. Checked
//!   standalone by the Verus harness (`verus --crate-type lib
//!   src/bounds.rs`).
//! - [`gguf`] — the container walk composed exclusively from those
//!   primitives (unsafe-free; every read and size computation is total).
//!   Every input yields either a validated [`ModelDescriptor`] — tensor
//!   table with checked offsets, sizes, quantization types, alignment and
//!   pairwise disjointness, consistency-checked llama hyperparameters, a
//!   pre-walked tokenizer — or a distinct [`GgufError`].
//!
//! The descriptor is plain data (offsets, not borrows): a PD keeps it in
//! private memory while the weights stay in the shared mapped region, and
//! sizes all downstream arenas from *validated* fields only.

#![no_std]
#![forbid(unsafe_code)]

pub mod bounds;
pub mod gguf;

pub use gguf::{
    parse, GgufError, LlamaConfig, ModelDescriptor, TensorDesc, TokenIter, TokenizerRegions,
};
