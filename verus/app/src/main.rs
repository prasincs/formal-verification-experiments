//! Application using the verified library.
//!
//! Build and run with: `cargo run`

use verified::{Amount, safe_divide, MAX_AMOUNT};

fn main() {
    println!("=== Using Verified Library ===\n");

    // Create amounts
    let amount1 = Amount::new(1_000_000).expect("Valid amount");
    let amount2 = Amount::new(500_000).expect("Valid amount");

    println!("Amount 1: {} satoshis", amount1.value());
    println!("Amount 2: {} satoshis", amount2.value());

    // Safe addition
    match amount1.checked_add(&amount2) {
        Some(sum) => println!("Sum: {} satoshis", sum.value()),
        None => println!("Sum would overflow MAX_AMOUNT"),
    }

    // Safe division (verified to never panic when b != 0)
    let avg = safe_divide(amount1.value() + amount2.value(), 2);
    println!("Average: {} satoshis", avg);

    // Overflow protection
    println!("\nMAX_AMOUNT: {} satoshis", MAX_AMOUNT);
    match Amount::new(MAX_AMOUNT + 1) {
        Some(_) => println!("Created invalid amount!"),
        None => println!("Correctly rejected amount > MAX_AMOUNT"),
    }

    println!("\nThe verified library guarantees:");
    println!("  - Amount values are always <= MAX_AMOUNT");
    println!("  - checked_add never overflows");
    println!("  - safe_divide never divides by zero (caller must ensure b != 0)");
}
