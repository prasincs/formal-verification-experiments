//! FAILURE: Division by zero
//!
//! Run: verus examples/01_division_by_zero.rs
//!
//! Expected error: "possible division by zero"
//!
//! The division operator in Verus requires proof that the denominator is non-zero.
//! Without a `requires` clause, Verus cannot verify this is safe.
//!
//! FIX: Add `requires b != 0` to the function signature.

use vstd::prelude::*;

verus! {

fn divide_unsafe(a: u64, b: u64) -> u64 {
    a / b  // error: possible division by zero
}

// Uncomment to see the fix:
// fn divide_safe(a: u64, b: u64) -> u64
//     requires b != 0
// {
//     a / b
// }

fn main() {}

}
