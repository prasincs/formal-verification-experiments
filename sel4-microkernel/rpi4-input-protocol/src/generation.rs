//! Restart-safe generation handling for the input SPSC ring.
//!
//! Correctness comes from the lifecycle PD stopping both endpoints before a
//! reset. The per-operation generation check is defense in depth only; it is
//! not a seqlock and cannot make a concurrent reset safe.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::generation_contract::{reset_plan, validate_stable_generation};
use crate::InputRingHeader;

/// Generation zero denotes a legacy deployment with no lifecycle supervisor.
pub const LEGACY_GENERATION: u32 = 0;
/// The first non-legacy, stable generation.
pub const FIRST_STABLE_GENERATION: u32 = 2;
const GENERATION_OFFSET: usize = 0x0c;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation(u32);

impl Generation {
    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn is_legacy(self) -> bool {
        self.0 == LEGACY_GENERATION
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OddGeneration {
    pub observed: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenerationChanged {
    pub expected: Generation,
    pub observed: u32,
}

/// Linear evidence supplied by the lifecycle PD for one quiescent reset.
///
/// The token is deliberately neither `Copy` nor `Clone`: each unsafe assertion
/// of endpoint quiescence authorizes exactly one reset attempt.
#[derive(Debug)]
pub struct EndpointsStopped(());

impl EndpointsStopped {
    /// # Safety
    /// The caller must have stopped producer and consumer, except that the
    /// lifecycle PD itself may be the already-quiescent endpoint.
    pub unsafe fn new_unchecked() -> Self {
        Self(())
    }
}

pub struct ResyncedInputRing<'a> {
    header: &'a InputRingHeader,
    generation: Generation,
}

impl InputRingHeader {
    fn generation_atomic(&self) -> &AtomicU32 {
        // The legacy padding word at 0x0c is aligned and initialized before the
        // region is shared. All post-start accesses are atomic.
        unsafe {
            &*((self as *const Self as *const u8).add(GENERATION_OFFSET)
                as *const AtomicU32)
        }
    }

    pub fn generation(&self) -> Generation {
        Generation(self.generation_atomic().load(Ordering::Acquire))
    }

    /// Re-derive endpoint-local state after boot or restart.
    pub fn resync(&self) -> Result<Generation, OddGeneration> {
        let observed = self.generation_atomic().load(Ordering::Acquire);
        let stable = validate_stable_generation(observed)
            .map_err(|observed| OddGeneration { observed })?;

        debug_assert!(self.capacity > 0);
        debug_assert!(self.current_write_idx() < self.capacity);
        debug_assert!(self.current_read_idx() < self.capacity);
        Ok(Generation(stable))
    }

    pub fn resynced_endpoint(&self) -> Result<ResyncedInputRing<'_>, OddGeneration> {
        let generation = self.resync()?;
        Ok(ResyncedInputRing {
            header: self,
            generation,
        })
    }

    /// Reset while both endpoints are stopped.
    ///
    /// The verified executable `reset_plan` computes the odd and next-even
    /// values. The runtime stores remain intentionally explicit: odd marker,
    /// zero endpoint-visible indices, then final even publication. The odd
    /// marker is diagnostic, not a synchronization protocol for live endpoints;
    /// quiescence is the correctness mechanism.
    ///
    /// # Safety
    /// `stopped` must truthfully represent the current quiescent window.
    pub unsafe fn quiescent_reset(
        &self,
        stopped: EndpointsStopped,
    ) -> Result<Generation, OddGeneration> {
        let current = self.generation_atomic().load(Ordering::Acquire);
        let (odd, next_even) =
            reset_plan(current).map_err(|observed| OddGeneration { observed })?;

        // Consume the linear token in this reset invocation.
        let _consumed = stopped;
        self.generation_atomic().store(odd, Ordering::Release);
        self.write_idx.store(0, Ordering::Release);
        self.read_idx.store(0, Ordering::Release);
        self.generation_atomic()
            .store(next_even, Ordering::Release);

        Ok(Generation(next_even))
    }
}

impl ResyncedInputRing<'_> {
    pub const fn generation(&self) -> Generation {
        self.generation
    }

    fn check_generation(&self) -> Result<(), GenerationChanged> {
        let observed = self.header.generation_atomic().load(Ordering::Acquire);
        if observed == self.generation.get() && observed & 1 == 0 {
            Ok(())
        } else {
            Err(GenerationChanged {
                expected: self.generation,
                observed,
            })
        }
    }

    pub fn current_write_idx(&self) -> Result<u32, GenerationChanged> {
        self.check_generation()?;
        Ok(self.header.current_write_idx())
    }

    pub fn current_read_idx(&self) -> Result<u32, GenerationChanged> {
        self.check_generation()?;
        Ok(self.header.current_read_idx())
    }

    /// Defense-in-depth stale-handle check followed by publication.
    ///
    /// This is deliberately not an atomic check-and-act protocol. A concurrent
    /// reset between the check and store would still race; callers rely on the
    /// lifecycle PD stopping endpoints for every reset.
    pub fn advance_write(&self) -> Result<(), GenerationChanged> {
        self.check_generation()?;
        self.header.advance_write();
        Ok(())
    }

    /// See [`Self::advance_write`] for the quiescence requirement.
    pub fn advance_read(&self) -> Result<(), GenerationChanged> {
        self.check_generation()?;
        self.header.advance_read();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;

    fn header() -> InputRingHeader {
        let mut storage = MaybeUninit::<InputRingHeader>::uninit();
        unsafe {
            InputRingHeader::init(storage.as_mut_ptr());
            storage.assume_init()
        }
    }

    unsafe fn stopped() -> EndpointsStopped {
        EndpointsStopped::new_unchecked()
    }

    #[test]
    fn producer_restart_requires_new_resync() {
        let header = header();
        let endpoint = header.resynced_endpoint().unwrap();
        endpoint.advance_write().unwrap();
        assert_eq!(endpoint.current_write_idx().unwrap(), 1);

        unsafe { header.quiescent_reset(stopped()).unwrap() };
        assert!(endpoint.advance_write().is_err());

        let restarted = header.resynced_endpoint().unwrap();
        assert_eq!(restarted.current_write_idx().unwrap(), 0);
        restarted.advance_write().unwrap();
        assert_eq!(restarted.current_write_idx().unwrap(), 1);
    }

    #[test]
    fn consumer_restart_rederives_shared_cursor() {
        let header = header();
        let endpoint = header.resynced_endpoint().unwrap();
        endpoint.advance_write().unwrap();
        endpoint.advance_read().unwrap();

        unsafe { header.quiescent_reset(stopped()).unwrap() };
        let restarted = header.resynced_endpoint().unwrap();
        assert_eq!(restarted.current_write_idx().unwrap(), 0);
        assert_eq!(restarted.current_read_idx().unwrap(), 0);
    }

    #[test]
    fn both_endpoints_restart_into_next_even_generation() {
        let header = header();
        let before = header.resync().unwrap();
        let after = unsafe { header.quiescent_reset(stopped()).unwrap() };
        assert_eq!(before.get(), LEGACY_GENERATION);
        assert_eq!(after.get(), FIRST_STABLE_GENERATION);
        assert_eq!(header.resync().unwrap(), after);
    }

    #[test]
    fn odd_generation_is_fatal() {
        let header = header();
        header.generation_atomic().store(3, Ordering::Release);
        assert_eq!(header.resync(), Err(OddGeneration { observed: 3 }));
    }

    #[test]
    fn generation_wrap_skips_legacy_zero() {
        let header = header();
        header
            .generation_atomic()
            .store(u32::MAX - 1, Ordering::Release);
        let next = unsafe { header.quiescent_reset(stopped()).unwrap() };
        assert_eq!(next.get(), FIRST_STABLE_GENERATION);
    }

    #[test]
    fn stale_handle_is_detected_after_quiescent_reset() {
        let header = header();
        let stale = header.resynced_endpoint().unwrap();
        unsafe { header.quiescent_reset(stopped()).unwrap() };
        assert_eq!(
            stale.advance_write(),
            Err(GenerationChanged {
                expected: Generation(LEGACY_GENERATION),
                observed: FIRST_STABLE_GENERATION,
            })
        );
    }
}
