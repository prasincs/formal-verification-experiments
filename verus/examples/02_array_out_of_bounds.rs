//! FAILURE: Array out of bounds
//!
//! Run: verus examples/02_array_out_of_bounds.rs
//!
//! Expected error: "precondition not satisfied" for slice indexing
//!
//! Verus requires proof that array indices are within bounds.
//! This prevents index-out-of-bounds panics at compile time.
//!
//! FIX: Add `requires index < arr.len()` to the function signature.

use vstd::prelude::*;

verus! {

fn get_element_unsafe(arr: &[u64], index: usize) -> u64 {
    arr[index]  // error: precondition not satisfied
}

// Uncomment to see the fix:
// fn get_element_safe(arr: &[u64], index: usize) -> u64
//     requires index < arr.len()
// {
//     arr[index]
// }

fn main() {}

}
