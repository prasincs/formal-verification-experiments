//! Host-side test harness for the `rpi4-photoframe` pure-logic modules.
//!
//! The photoframe protection-domain binary cannot be built on a host — it
//! depends on `sel4-microkit` and a bare-metal `aarch64-sel4-microkit` target.
//! Its decode / validation / allocator logic, however, is target-independent.
//! We pull those modules in verbatim with `#[path]` and run them under std
//! `cargo test`, so CI gets fast, deterministic coverage of the exact code an
//! attacker-supplied image flows through, without needing the Microkit SDK.
//!
//! The aarch64 compile itself (deps, `no_std`, the seL4 integration) is gated
//! separately by the cross-build job in `.github/workflows/photoframe.yml`.

// The pulled-in modules use `alloc` (via zune's `Vec`); std provides it.
extern crate alloc;

#[path = "../../rpi4-photoframe/src/bounded_alloc.rs"]
pub mod bounded_alloc;

#[path = "../../rpi4-photoframe/src/decoder.rs"]
pub mod decoder;

#[path = "../../rpi4-photoframe/src/validate.rs"]
pub mod validate;

#[path = "../../rpi4-photoframe/src/secure_decode.rs"]
pub mod secure_decode;
