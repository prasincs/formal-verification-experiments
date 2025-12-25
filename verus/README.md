# Verus Formal Verification Demo

A verified Rust library that works with both standard Cargo and Verus verification.

## The Key Insight

The `verus_builtin_macros` crate on crates.io is Verus's official `verus!` macro. It automatically strips verification constructs when compiled without `cfg(verus_keep_ghost)`:

| Tool | Result |
|------|--------|
| `cargo build` | Specs stripped, compiles as pure Rust |
| `cargo test` | Tests run normally |
| `verus` | Full verification with SMT solver |

**Single source of truth** - no code duplication, no build.rs, no macro gymnastics.

## Quick Start

```bash
# Standard Cargo - works immediately
cargo build
cargo test
cargo run -p app

# Verus verification (via container)
./run.sh              # Verify the library
./run.sh shell        # Interactive shell
./run.sh examples     # Run all failure examples
```

## Project Structure

```
verus/
├── verified/               # Verified library (works with cargo + verus)
│   ├── Cargo.toml
│   └── src/lib.rs
├── app/                    # Example app using the library
│   └── src/main.rs
├── examples/               # Verus failure examples (verus-only)
│   ├── 01_division_by_zero.rs
│   ├── 02_array_out_of_bounds.rs
│   └── ...
├── Containerfile           # Container for running Verus
└── run.sh                  # Helper script
```

## How It Works

### verified/Cargo.toml

```toml
[dependencies]
verus_builtin_macros = "0.0.0-2025-12-07-0054"
verus_builtin = "0.0.0-2025-12-07-0054"

[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(verus_keep_ghost)'] }
```

### verified/src/lib.rs

```rust
use verus_builtin_macros::verus;

verus! {

pub fn safe_divide(a: u64, b: u64) -> (result: u64)
    requires
        b != 0,
    ensures
        result == a / b,
{
    a / b
}

} // verus!
```

**With cargo:** `pub fn safe_divide(a: u64, b: u64) -> u64 { a / b }`

**With verus:** Full verification - proves division never panics when precondition holds.

## What Gets Stripped

| Construct | Verus Meaning | Cargo Build |
|-----------|---------------|-------------|
| `requires` | Precondition | Removed |
| `ensures` | Postcondition | Removed |
| `spec fn` | Specification function | Removed entirely |
| `proof fn` | Proof function | Removed entirely |
| `invariant` | Loop invariant | Removed |
| `decreases` | Termination measure | Removed |
| `open`/`closed` | Visibility modifiers | Removed |
| `(result: T)` | Named return | Becomes `-> T` |

## Failure Examples

The `examples/` directory contains standalone files demonstrating bugs Verus catches:

```bash
# In container or with local Verus:
verus examples/01_division_by_zero.rs
verus examples/02_array_out_of_bounds.rs
verus examples/03_integer_overflow.rs
# ... etc
```

Each shows the verification error and includes a commented fix.

## Running Verus

### Option 1: Container (Recommended)

No installation required. Works with Docker, Podman, or any OCI runtime.

```bash
./run.sh              # Verify the library
./run.sh shell        # Interactive shell
./run.sh examples     # Run all failure examples
./run.sh build        # Rebuild container
```

First build takes ~30 minutes (compiles Z3 and Verus). Subsequent runs are instant.

**Example output:**

```
=== Verus Formal Verification Demo ===

Project structure:
  verified/src/lib.rs  - Verified library (cargo + verus compatible)
  app/src/main.rs      - Example application
  examples/            - 8 failure examples

Running: verus --crate-type lib verified/src/lib.rs

verification results:: 9 verified, 0 errors

---
Try also:
  ./run.sh shell     - Interactive shell
  ./run.sh examples  - Run all failure examples
```

### Option 2: Local Installation

```bash
git clone https://github.com/verus-lang/verus.git
cd verus
./tools/get-z3.sh
./tools/cargo.sh build --release
export PATH="$PATH:$(pwd)/source/target/release"
```

Then verify with:

```bash
verus --crate-type lib verified/src/lib.rs
```

## Key Concepts

### Preconditions & Postconditions

```rust
fn safe_divide(a: u64, b: u64) -> u64
    requires b != 0        // Caller's obligation
    ensures result <= a    // Function's guarantee
{
    a / b
}
```

### Spec Functions

Pure mathematical functions for specifications (no runtime cost):

```rust
spec fn is_valid(x: u64) -> bool {
    x <= MAX_VALUE
}
```

### The Amount Example

The library includes a verified `Amount` type for overflow-safe arithmetic:

```rust
impl Amount {
    pub fn checked_add(&self, other: &Self) -> Option<Self>
        requires
            self.valid(),
            other.valid(),
        ensures
            result.is_some() ==> result.unwrap().valid(),
    {
        // Verus proves this never overflows
    }
}
```

## IDE Setup

For full Verus IDE support, use [verus-analyzer](https://github.com/verus-lang/verus-analyzer) instead of rust-analyzer.

Standard rust-analyzer works for the cargo-compatible subset (after macro expansion).

## Why Verification Matters

From real incidents: a single unguarded `.unwrap()` can cause hours of global outages. Verus catches these at compile time:

- **Division by zero** - Requires proof that denominator != 0
- **Array bounds** - Requires proof that index < length
- **Integer overflow** - Requires proof that result fits in type
- **Unwrap on None** - Requires proof that Option is Some

## Resources

- [Verus Guide](https://verus-lang.github.io/verus/guide/)
- [Verus GitHub](https://github.com/verus-lang/verus)
