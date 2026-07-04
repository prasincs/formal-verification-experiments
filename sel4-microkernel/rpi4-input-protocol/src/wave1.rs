#![no_std]
#![allow(clippy::module_inception)]

#[path = "lib.rs"]
mod legacy;

pub use legacy::*;

mod generation_contract;
mod generation;
pub use generation::*;
