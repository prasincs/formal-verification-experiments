#![no_std]

#[path = "lib.rs"]
mod legacy;

pub use legacy::*;

mod generation_contract;
mod generation;
pub use generation::*;
