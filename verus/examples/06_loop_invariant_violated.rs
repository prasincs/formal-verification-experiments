//! FAILURE: Loop invariant not maintained
//!
//! Run: verus examples/06_loop_invariant_violated.rs
//!
//! Expected error: "invariant not satisfied at end of loop body"
//!
//! Loop invariants must be true before the loop, after each iteration,
//! and therefore after the loop. If the body breaks the invariant,
//! verification fails.
//!
//! FIX: Either fix the code to maintain the invariant, or fix the invariant.

use vstd::prelude::*;

verus! {

fn broken_loop_invariant(n: u64) -> usize
    requires n < 1000
{
    let mut i: usize = 0;
    let mut count: usize = 0;
    while i < n as usize
        invariant
            count <= i,  // error: invariant not satisfied at end of loop body
        decreases
            n - i as u64,
    {
        count = count + 2;  // Bug: count grows faster than i
        i = i + 1;
    }
    count
}

// Uncomment to see the fix (correct invariant):
// fn fixed_loop_invariant(n: u64) -> usize
//     requires n < 1000
// {
//     let mut i: usize = 0;
//     let mut count: usize = 0;
//     while i < n as usize
//         invariant
//             count == 2 * i,  // Correct invariant
//             i <= n as usize,
//         decreases
//             n - i as u64,
//     {
//         count = count + 2;
//         i = i + 1;
//     }
//     count
// }

fn main() {}

}
