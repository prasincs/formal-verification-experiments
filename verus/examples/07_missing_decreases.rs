//! FAILURE: Missing loop termination proof
//!
//! Run: verus examples/07_missing_decreases.rs
//!
//! Expected error: "loop must have a decreases clause"
//!
//! Verus requires proof that loops terminate. The `decreases` clause
//! specifies a value that decreases on each iteration and is bounded below.
//!
//! FIX: Add `decreases n - i` to prove the loop terminates.

use vstd::prelude::*;

verus! {

fn missing_decreases(n: u64) -> u64
    requires n < 1000  // Bound to prevent overflow in sum
{
    let mut i: u64 = 0;
    let mut sum: u64 = 0;
    while i < n
        invariant
            i <= n,
            sum == i * (i - 1) / 2,  // Sum of 0..i
        // error: loop must have a decreases clause
    {
        sum = sum + i;
        i = i + 1;
    }
    sum
}

// Uncomment to see the fix:
// fn with_decreases(n: u64) -> u64
//     requires n < 1000
// {
//     let mut i: u64 = 0;
//     let mut sum: u64 = 0;
//     while i < n
//         invariant
//             i <= n,
//         decreases
//             n - i,  // Proves termination: decreases each iteration, bounded by 0
//     {
//         sum = sum + i;
//         i = i + 1;
//     }
//     sum
// }

fn main() {}

}
