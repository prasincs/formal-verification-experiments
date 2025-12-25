//! # Verus Formal Verification Demo
//!
//! A verified library that works with both:
//! - `cargo build/test` - specs stripped, compiles as pure Rust
//! - `verus` - full verification
//!
//! ## How It Works
//!
//! The `verus_builtin_macros` crate provides the official `verus!` macro.
//! When compiled without `cfg(verus_keep_ghost)` (i.e., with cargo),
//! it strips all verification constructs, producing valid Rust.

use verus_builtin_macros::verus;

verus! {

// ============================================================================
// SECTION 1: BASICS - PRECONDITIONS & POSTCONDITIONS
// ============================================================================
//
// Verus extends Rust with `requires` (preconditions) and `ensures` (postconditions).
// These are compile-time checked contracts - violations are caught during verification.

/// Safe division - caller must ensure denominator is non-zero.
pub fn safe_divide(a: u64, b: u64) -> (result: u64)
    requires
        b != 0,
    ensures
        result == a / b,
{
    a / b
}

/// Spec functions are pure mathematical functions used only in specifications.
/// They don't generate runtime code - they exist purely for verification.
pub open spec fn spec_max(a: u64, b: u64) -> u64 {
    if a >= b { a } else { b }
}

/// Using spec functions in ensures clauses to specify behavior precisely.
pub fn max(a: u64, b: u64) -> (result: u64)
    ensures
        result == spec_max(a, b),
        result >= a,
        result >= b,
{
    if a >= b { a } else { b }
}

// ============================================================================
// SECTION 2: PANIC DETECTION - THE CORE VALUE PROP
// ============================================================================
//
// This is where Verus shines. It catches runtime panics at compile time:
// unwrap on None, array out-of-bounds, division by zero, integer overflow.

/// Array bounds verification: Verus proves index is always valid.
pub fn safe_index(arr: &[u64], index: usize) -> (value: u64)
    requires
        index < arr.len(),
    ensures
        value == arr[index as int],
{
    arr[index]
}

/// Arithmetic overflow protection.
pub fn safe_add(a: u64, b: u64) -> (result: u64)
    requires
        a as int + b as int <= u64::MAX as int,
    ensures
        result == a + b,
{
    a + b
}

/// Subtraction with underflow protection.
pub fn safe_subtract(a: u64, b: u64) -> (result: u64)
    requires
        a >= b,
    ensures
        result == a - b,
{
    a - b
}

// ============================================================================
// SECTION 3: PRACTICAL APPLICATION - VERIFIED AMOUNT ARITHMETIC
// ============================================================================
//
// A complete example: overflow-safe monetary calculations.

/// Maximum amount (e.g., 21 million BTC in satoshis).
pub const MAX_AMOUNT: u64 = 21_000_000 * 100_000_000;

/// Represents a monetary amount in the smallest unit.
#[derive(Clone, Copy)]
pub struct Amount {
    pub value: u64,
}

impl Amount {
    /// Spec function: is this a valid amount?
    pub open spec fn valid(&self) -> bool {
        self.value <= MAX_AMOUNT
    }

    /// Create a new Amount with validation.
    pub fn new(value: u64) -> (result: Option<Self>)
        ensures
            match result {
                Some(amt) => amt.valid() && amt.value == value,
                None => value > MAX_AMOUNT,
            },
    {
        if value <= MAX_AMOUNT {
            Some(Amount { value })
        } else {
            None
        }
    }

    /// Get the underlying value.
    pub fn value(&self) -> (v: u64)
        ensures v == self.value,
    {
        self.value
    }

    /// Checked addition: returns None on overflow.
    pub fn checked_add(&self, other: &Self) -> (result: Option<Self>)
        requires
            self.valid(),
            other.valid(),
        ensures
            match result {
                Some(amt) => {
                    amt.valid()
                    && amt.value as int == self.value as int + other.value as int
                },
                None => self.value as int + other.value as int > MAX_AMOUNT as int,
            },
    {
        let sum = self.value as u128 + other.value as u128;
        if sum <= MAX_AMOUNT as u128 {
            Some(Amount { value: sum as u64 })
        } else {
            None
        }
    }

    /// Checked subtraction: returns None on underflow.
    pub fn checked_sub(&self, other: &Self) -> (result: Option<Self>)
        requires
            self.valid(),
            other.valid(),
        ensures
            match result {
                Some(amt) => {
                    amt.valid()
                    && amt.value as int == self.value as int - other.value as int
                },
                None => self.value < other.value,
            },
    {
        if self.value >= other.value {
            Some(Amount { value: self.value - other.value })
        } else {
            None
        }
    }
}

} // verus!

// ============================================================================
// TESTS - Run with `cargo test`
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_divide() {
        assert_eq!(safe_divide(10, 2), 5);
        assert_eq!(safe_divide(7, 3), 2);
    }

    #[test]
    fn test_max() {
        assert_eq!(max(5, 3), 5);
        assert_eq!(max(3, 5), 5);
        assert_eq!(max(4, 4), 4);
    }

    #[test]
    fn test_amount_new() {
        assert!(Amount::new(1000).is_some());
        assert!(Amount::new(MAX_AMOUNT).is_some());
        assert!(Amount::new(MAX_AMOUNT + 1).is_none());
    }

    #[test]
    fn test_checked_add() {
        let a = Amount::new(100).unwrap();
        let b = Amount::new(200).unwrap();
        assert_eq!(a.checked_add(&b).unwrap().value(), 300);
    }

    #[test]
    fn test_checked_add_overflow() {
        let a = Amount::new(MAX_AMOUNT).unwrap();
        let b = Amount::new(1).unwrap();
        assert!(a.checked_add(&b).is_none());
    }

    #[test]
    fn test_checked_sub() {
        let a = Amount::new(300).unwrap();
        let b = Amount::new(100).unwrap();
        assert_eq!(a.checked_sub(&b).unwrap().value(), 200);
    }

    #[test]
    fn test_checked_sub_underflow() {
        let a = Amount::new(100).unwrap();
        let b = Amount::new(200).unwrap();
        assert!(a.checked_sub(&b).is_none());
    }
}
