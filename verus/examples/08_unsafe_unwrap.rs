//! FAILURE: Unsafe unwrap on Option
//!
//! Run: verus examples/08_unsafe_unwrap.rs
//!
//! Expected error: "precondition not satisfied" for unwrap
//!
//! Calling .unwrap() on an Option requires proof that it's Some.
//! This is exactly the class of bug that caused major outages
//! (e.g., Cloudflare November 2025).
//!
//! FIX: Either add `requires opt.is_some()` or use pattern matching.

use vstd::prelude::*;

verus! {

fn unsafe_unwrap(opt: Option<u64>) -> u64 {
    opt.unwrap()  // ERROR: Could be None!
}

//Uncomment to see fix option 1 - require Some:
fn unwrap_with_requires(opt: Option<u64>) -> u64
    requires opt.is_some()
{
    opt.unwrap()
}

// Uncomment to see fix option 2 - use pattern matching:
// fn unwrap_with_default(opt: Option<u64>, default: u64) -> u64 {
//     match opt {
//         Some(v) => v,
//         None => default,
//     }
// }

fn main() {}

}
