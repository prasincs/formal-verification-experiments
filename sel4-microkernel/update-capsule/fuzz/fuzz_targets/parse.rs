//! Fuzz the verified totality parser: no input may panic it.
//! Run: `cargo +nightly fuzz run parse -- -max_total_time=60`

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = update_capsule::header::parse(data);
});
