//! FAILURE: Precondition not satisfied by caller
//!
//! Run: verus examples/04_precondition_not_satisfied.rs
//!
//! Expected error: "precondition not satisfied" when calling safe_divide
//!
//! When you call a function with `requires`, you must prove those requirements.
//! The caller's obligation is to establish the precondition.
//!
//! FIX: Either add `requires y != 0` to the caller, or check at runtime.

use vstd::prelude::*;

verus! {

fn safe_divide(a: u64, b: u64) -> u64
    requires b != 0
{
    a / b
}

fn call_without_checking(x: u64, y: u64) -> u64 {
    safe_divide(x, y)  // ERROR: Caller doesn't prove y != 0
}

// Uncomment to see fix option 1 - propagate the requirement:
// fn call_with_requires(x: u64, y: u64) -> u64
//     requires y != 0
// {
//     safe_divide(x, y)
// }

// Uncomment to see fix option 2 - check at runtime:
// fn call_with_check(x: u64, y: u64) -> Option<u64> {
//     if y != 0 {
//         Some(safe_divide(x, y))
//     } else {
//         None
//     }
// }

fn main() {}

}
