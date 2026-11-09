//! Boot-time measurement helpers.
//!
//! The pure functions are tested on the host with
//! `rustc --edition 2021 --test src/bootprof.rs` (run by
//! `scripts/test-baremetal.sh`); this crate is `no_main` and links against
//! seL4, so `cargo test` is not available.

/// Read the time-stamp counter. seL4 leaves CR4.TSD clear, so RDTSC is
/// permitted at CPL3.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
#[allow(dead_code)]
pub fn rdtsc() -> u64 {
    #[allow(unused_unsafe)]
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
}

/// Decode the `SEL4_BOOTINFO_HEADER_X86_TSC_FREQ` payload: a native-endian
/// (x86: little-endian) u32 in MHz, written by seL4 `init_sys_state`.
/// Rejects short payloads and a zero frequency.
pub fn parse_tsc_mhz(content: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = content.get(..4)?.try_into().ok()?;
    match u32::from_le_bytes(bytes) {
        0 => None,
        mhz => Some(mhz),
    }
}

/// TSC cycles to microseconds; `None` for a zero frequency.
pub fn cycles_to_us(cycles: u64, mhz: u32) -> Option<u64> {
    cycles.checked_div(u64::from(mhz))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_le_u32() {
        assert_eq!(parse_tsc_mhz(&3100u32.to_le_bytes()), Some(3100));
    }

    #[test]
    fn ignores_trailing_bytes() {
        assert_eq!(parse_tsc_mhz(&[0x1c, 0x0c, 0, 0, 0xff, 0xff]), Some(3100));
    }

    #[test]
    fn rejects_short_and_zero() {
        assert_eq!(parse_tsc_mhz(&[]), None);
        assert_eq!(parse_tsc_mhz(&[1, 2, 3]), None);
        assert_eq!(parse_tsc_mhz(&0u32.to_le_bytes()), None);
    }

    #[test]
    fn converts_cycles() {
        assert_eq!(cycles_to_us(3_100_000, 3100), Some(1000));
        assert_eq!(cycles_to_us(3099, 3100), Some(0)); // truncates
        assert_eq!(cycles_to_us(u64::MAX, 1), Some(u64::MAX));
    }

    #[test]
    fn zero_frequency_is_none() {
        assert_eq!(cycles_to_us(1, 0), None);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn rdtsc_is_monotonic_on_one_core() {
        let a = rdtsc();
        let b = rdtsc();
        assert!(b >= a);
    }
}
