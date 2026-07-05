//! Monotonic time for the network stack.
//!
//! Deployed AArch64 builds read the architectural virtual counter. The QEMU
//! Microkit hypervisor configuration used by CI traps direct timer-register
//! access, so `qemu-time-fallback` selects a deterministic logical clock that
//! advances on each stack poll. The fallback is explicit and never enabled for
//! hardware products.

use smoltcp::time::Instant;

#[cfg(feature = "qemu-time-fallback")]
use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(all(target_arch = "aarch64", not(feature = "qemu-time-fallback")))]
#[inline]
pub fn counter_frequency_hz() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mrs {value}, cntfrq_el0", value = out(reg) value);
    }
    value
}

#[cfg(all(target_arch = "aarch64", not(feature = "qemu-time-fallback")))]
#[inline]
pub fn counter_ticks() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mrs {value}, cntvct_el0", value = out(reg) value);
    }
    value
}

#[cfg(all(target_arch = "aarch64", not(feature = "qemu-time-fallback")))]
pub fn monotonic_millis() -> u64 {
    ticks_to_millis(counter_ticks(), counter_frequency_hz())
}

#[cfg(feature = "qemu-time-fallback")]
pub fn monotonic_millis() -> u64 {
    static LOGICAL_MILLIS: AtomicU64 = AtomicU64::new(0);
    LOGICAL_MILLIS.fetch_add(10, Ordering::Relaxed)
}

#[cfg(any(target_arch = "aarch64", feature = "qemu-time-fallback"))]
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

    #[cfg(feature = "qemu-time-fallback")]
    #[test]
    fn qemu_clock_advances_deterministically() {
        let first = monotonic_millis();
        let second = monotonic_millis();
        assert_eq!(second - first, 10);
    }
}
