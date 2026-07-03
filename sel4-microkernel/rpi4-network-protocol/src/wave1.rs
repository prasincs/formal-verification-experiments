#![no_std]

#[path = "lib.rs"]
mod legacy;

pub use legacy::*;

mod generation;
pub use generation::*;
