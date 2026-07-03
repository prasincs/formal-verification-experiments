#![no_std]
#![allow(clippy::module_inception)]

// Keep the established protocol implementation intact.  The generation-ring
// API is layered alongside it so legacy generation-0 images remain ABI
// compatible while restart-aware endpoints can opt into the stronger API.
#[path = "lib.rs"]
mod legacy;

pub use legacy::*;

mod generation;
pub use generation::*;
