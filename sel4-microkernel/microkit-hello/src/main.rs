//! seL4 Microkit Hello World Protection Domain
//!
//! This is the simplest possible Microkit protection domain:
//! - Initializes and prints a message
//! - Demonstrates the formally verified seL4 kernel is running
//! - Shows the Microkit API for handling notifications
//!
//! The seL4 kernel underneath is formally verified to:
//! - Never crash
//! - Never allow unauthorized access
//! - Correctly implement the specification

#![no_std]
#![no_main]

use sel4_microkit::{debug_println, protection_domain, Handler, Infallible};

use verus_builtin_macros::verus;

// ============================================================================
// Verified Components
// ============================================================================

verus! {

/// Verified counter that proves we never overflow
pub struct VerifiedCounter {
    value: u64,
    max: u64,
}

impl VerifiedCounter {
    /// Specification: is the counter in a valid state?
    pub open spec fn valid(&self) -> bool {
        self.value <= self.max
    }

    /// Create a new counter with a maximum value
    pub fn new(max: u64) -> (result: Self)
        ensures result.valid(),
    {
        VerifiedCounter { value: 0, max }
    }

    /// Increment the counter, returning whether it succeeded
    /// Verus proves this never overflows
    pub fn increment(&mut self) -> (success: bool)
        requires old(self).valid(),
        ensures
            self.valid(),
            success ==> self.value == old(self).value + 1,
            !success ==> self.value == old(self).value,
    {
        if self.value < self.max {
            self.value = self.value + 1;
            true
        } else {
            false
        }
    }

    /// Get the current value
    pub fn get(&self) -> (v: u64)
        ensures v == self.value,
    {
        self.value
    }
}

/// Verified capability rights (seL4-style)
/// Demonstrates the capability derivation principle
pub const RIGHT_READ: u64 = 1 << 0;
pub const RIGHT_WRITE: u64 = 1 << 1;
pub const RIGHT_GRANT: u64 = 1 << 2;
pub const RIGHT_EXECUTE: u64 = 1 << 3;

#[derive(Clone, Copy)]
pub struct Capability {
    pub rights: u64,
}

impl Capability {
    /// Specification: check if capability has a specific right
    pub open spec fn has_right(&self, right: u64) -> bool {
        (self.rights & right) != 0
    }

    /// Derive a child capability with reduced rights
    /// Verus proves: child can never have more rights than parent
    pub fn derive(&self, mask: u64) -> (child: Self)
        ensures
            // Child rights are subset of parent rights
            child.rights == (self.rights & mask),
            // Child cannot have rights parent doesn't have
            forall|r: u64| child.has_right(r) ==> self.has_right(r),
    {
        Capability {
            rights: self.rights & mask,
        }
    }

    /// Check if this capability has a right
    pub fn check_right(&self, right: u64) -> (has: bool)
        ensures has == self.has_right(right),
    {
        (self.rights & right) != 0
    }
}

} // verus!

// ============================================================================
// Microkit Protection Domain
// ============================================================================

/// The protection domain handler
struct HelloPd {
    counter: VerifiedCounter,
}

impl HelloPd {
    fn new() -> Self {
        HelloPd {
            counter: VerifiedCounter::new(1000),
        }
    }
}

impl Handler for HelloPd {
    type Error = Infallible;
}

// Declare this as a Microkit protection domain
#[protection_domain]
fn init() -> HelloPd {
    debug_println!("========================================");
    debug_println!("  seL4 Microkit - Formally Verified OS  ");
    debug_println!("========================================");
    debug_println!();
    debug_println!("Protection Domain 'hello' initialized!");
    debug_println!();
    debug_println!("This system is running on seL4, the world's");
    debug_println!("most secure operating system kernel.");
    debug_println!();
    debug_println!("Verification guarantees:");
    debug_println!("  - Functional correctness (C matches spec)");
    debug_println!("  - Binary verification (ARM: binary matches C)");
    debug_println!("  - Security: integrity, confidentiality");
    debug_println!("  - Availability: kernel cannot crash");
    debug_println!();

    // Demonstrate verified capability system
    debug_println!("Demonstrating verified capability derivation:");
    let parent = Capability {
        rights: RIGHT_READ | RIGHT_WRITE | RIGHT_GRANT,
    };
    debug_println!("  Parent capability: read, write, grant");

    let child = parent.derive(RIGHT_READ | RIGHT_WRITE);
    debug_println!("  Child capability (derived): read, write");
    debug_println!(
        "  Child can read: {}",
        child.check_right(RIGHT_READ)
    );
    debug_println!(
        "  Child can grant: {}",
        child.check_right(RIGHT_GRANT)
    );
    debug_println!();

    // Demonstrate verified counter
    let mut pd = HelloPd::new();
    debug_println!("Demonstrating verified counter (overflow-safe):");
    for _ in 0..5 {
        pd.counter.increment();
    }
    debug_println!("  Counter value after 5 increments: {}", pd.counter.get());
    debug_println!();

    debug_println!("System ready.");
    debug_println!();

    pd
}
