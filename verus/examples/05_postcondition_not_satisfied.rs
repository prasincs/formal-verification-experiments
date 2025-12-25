//! FAILURE: Postcondition not satisfied
//!
//! Run: verus examples/05_postcondition_not_satisfied.rs
//!
//! Expected error: "postcondition not satisfied"
//!
//! The `ensures` clause is a promise to callers. If the implementation
//! doesn't actually satisfy it, Verus catches the lie.
//!
//! FIX: Implement the function correctly to satisfy the postcondition.

use vstd::prelude::*;

verus! {

fn broken_max(a: u64, b: u64) -> (result: u64)
    ensures
        result >= a,
        result >= b,  // error: postcondition not satisfied
{
    a  // Bug: always returns a, even when b > a
}

// Uncomment to see the fix:
// fn correct_max(a: u64, b: u64) -> (result: u64)
//     ensures
//         result >= a,
//         result >= b,
// {
//     if a >= b { a } else { b }
// }

fn main() {}

}
