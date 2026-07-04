//! Monotonic time for the network stack.
//!
//! On AArch64 this reads the architectural virtual counter. The counter is
//! monotonic across protection-domain execution and does not depend on a wall
//! clock or timer service. Host tests use an explicit mock counter.

use smoltcp::time::Instant;

#[cfg(target_arch = "aarch64")]
#[inline]
pub fn counter_frequency_hz() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mrs {value}, cntfrq_el0", value = out(reg) value);
    }
    value
}

#[cfg(target_arch = "aarch64")]
#[inline]
pub fn counter_ticks() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mrs {value}, cntvct_el0", value = out(reg) value);
    }
    value
}

#[cfg(target_arch = "aarch64")]
pub fn monotonic_millis() -> u64 {
    ticks_to_millis(counter_ticks(), counter_frequency_hz())
}

#[cfg(target_arch = "aarch64")]
pub fn instant() -> Instant {
    let millis = monotonic_millis().min(i64::MAX as u64) as i64;
    Instant::from_millis(millis)
}

/// Convert architectural counter ticks to milliseconds without overflowing
/// the intermediate multiplication.
pub const fn ticks_to_millis(ticks: u64, frequency_hz: u64) -> u64 {
    if frequency_hz == 0 {
        return 0;
    }

    let whole_seconds = ticks / frequency_hz;
    let remainder = ticks % frequency_hz;
    whole_seconds
        .saturating_mul(1_000)
        .saturating_add(remainder.saturating_mul(1_000) / frequency_hz)
}

#[cfg(all(test, not(target_arch = "aarch64")))]
mod tests {
    use super::*;

    #[test]
    fn converts_counter_ticks_without_overflow() {
        assert_eq!(ticks_to_millis(54_000_000, 54_000_000), 1_000);
        assert_eq!(ticks_to_millis(81_000_000, 54_000_000), 1_500);
        assert_eq!(ticks_to_millis(u64::MAX, 1_000_000_000), 18_446_744_073_709);
    }

    #[test]
    fn zero_frequency_is_safe() {
        assert_eq!(ticks_to_millis(123, 0), 0);
    }
}
