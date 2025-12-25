//! FAILURE: Integer overflow
//!
//! Run: verus examples/03_integer_overflow.rs
//!
//! Expected error: "possible arithmetic underflow/overflow"
//!
//! Unlike Rust's release mode (which wraps), Verus requires proof that
//! arithmetic operations don't overflow. This catches subtle bugs.
//!
//! FIX: Add `requires a as int + b as int <= u64::MAX as int`

use vstd::prelude::*;

verus! {

fn add_unsafe(a: u64, b: u64) -> u64 {
    a + b  // ERROR: Could overflow if a + b > u64::MAX
}

// Uncomment to see the fix:
// fn add_safe(a: u64, b: u64) -> u64
//     requires a as int + b as int <= u64::MAX as int
// {
//     a + b
// }

fn main() {}

}
