//! Restart-safe generation handling for the input SPSC ring.
//!
//! Correctness comes from the lifecycle PD stopping both endpoints before a
//! reset.  The generation check in [`ResyncedInputRing`] is defense in depth;
//! it is not a substitute for quiescence.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::InputRingHeader;
use verus_builtin_macros::verus;

/// Generation zero denotes a legacy deployment with no lifecycle supervisor.
pub const LEGACY_GENERATION: u32 = 0;
/// The first non-legacy, stable generation.
pub const FIRST_STABLE_GENERATION: u32 = 2;
const GENERATION_OFFSET: usize = 0x0c;

/// A stable (even) ring generation observed by an endpoint.
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

/// Returned when an endpoint observes a reset in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OddGeneration {
    pub observed: u32,
}

/// Returned when a defense-in-depth generation check detects stale state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenerationChanged {
    pub expected: Generation,
    pub observed: u32,
}

/// Evidence supplied by the lifecycle PD that both ring endpoints are stopped.
///
/// # Safety
/// Constructing this token is an assertion about the Microkit lifecycle state.
/// The caller must have stopped both the producer and consumer (except that the
/// lifecycle PD itself may be one endpoint and already quiescent in the fault
/// handler).  Violating this precondition reintroduces the reset/publication
/// race rejected by IC-1.
#[derive(Clone, Copy, Debug)]
pub struct EndpointsStopped(());

impl EndpointsStopped {
    pub unsafe fn new_unchecked() -> Self {
        Self(())
    }
}

/// An endpoint view that can only be obtained after `resync`.
pub struct ResyncedInputRing<'a> {
    header: &'a InputRingHeader,
    generation: Generation,
}

impl InputRingHeader {
    fn generation_atomic(&self) -> &AtomicU32 {
        // The legacy header reserved bytes 0x0c..0x10 as a u32 padding word.
        // IC-1 assigns that already-aligned word to the atomic generation.
        // Initialization writes it before sharing; all post-start accesses use
        // atomics through this accessor.
        unsafe {
            &*((self as *const Self as *const u8).add(GENERATION_OFFSET)
                as *const AtomicU32)
        }
    }

    /// Acquire-load the current generation.
    pub fn generation(&self) -> Generation {
        Generation(self.generation_atomic().load(Ordering::Acquire))
    }

    /// Re-derive endpoint-local state after boot or restart.
    ///
    /// An odd value is a lifecycle fault: endpoints must park rather than
    /// retrying or publishing into a ring being reset.
    pub fn resync(&self) -> Result<Generation, OddGeneration> {
        let observed = self.generation_atomic().load(Ordering::Acquire);
        if observed & 1 != 0 {
            return Err(OddGeneration { observed });
        }

        debug_assert!(self.capacity > 0);
        debug_assert!(self.current_write_idx() < self.capacity);
        debug_assert!(self.current_read_idx() < self.capacity);
        Ok(Generation(observed))
    }

    /// Create the typed endpoint view used for the first and later
    /// publications after restart.
    pub fn resynced_endpoint(&self) -> Result<ResyncedInputRing<'_>, OddGeneration> {
        let generation = self.resync()?;
        Ok(ResyncedInputRing {
            header: self,
            generation,
        })
    }

    /// Reset the shared ring while both endpoints are stopped.
    ///
    /// The sequence is normative IC-1: publish odd, clear shared state, publish
    /// the next even generation.  Generation zero is skipped on wrap so it
    /// remains an unambiguous legacy marker.
    ///
    /// # Safety
    /// `stopped` must truthfully represent quiescence of both endpoints.
    pub unsafe fn quiescent_reset(
        &self,
        _stopped: EndpointsStopped,
    ) -> Result<Generation, OddGeneration> {
        let current = self.generation_atomic().load(Ordering::Acquire);
        if current & 1 != 0 {
            return Err(OddGeneration { observed: current });
        }

        let odd = current.wrapping_add(1);
        let mut next_even = odd.wrapping_add(1);
        if next_even == LEGACY_GENERATION {
            next_even = FIRST_STABLE_GENERATION;
        }

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

    /// Publish one producer advance.  Constructing this object proves the
    /// endpoint called `resync` before its first publication.
    pub fn advance_write(&self) -> Result<(), GenerationChanged> {
        self.check_generation()?;
        self.header.advance_write();
        Ok(())
    }

    /// Publish one consumer advance after a successful resynchronization.
    pub fn advance_read(&self) -> Result<(), GenerationChanged> {
        self.check_generation()?;
        self.header.advance_read();
        Ok(())
    }
}

verus! {

/// Proof model for the lifecycle-visible part of a restartable ring.
pub struct RestartRingModel {
    pub write_idx: u32,
    pub read_idx: u32,
    pub capacity: u32,
    pub generation: u32,
    pub resynced: bool,
}

impl RestartRingModel {
    pub open spec fn valid(&self) -> bool {
        self.capacity > 0 &&
        self.write_idx < self.capacity &&
        self.read_idx < self.capacity &&
        self.generation % 2 == 0
    }

    pub fn new(capacity: u32) -> (state: Self)
        requires capacity > 0,
        ensures
            state.valid(),
            state.write_idx == 0,
            state.read_idx == 0,
            state.generation == LEGACY_GENERATION,
            !state.resynced,
    {
        Self {
            write_idx: 0,
            read_idx: 0,
            capacity,
            generation: LEGACY_GENERATION,
            resynced: false,
        }
    }

    /// Model the atomic result of the odd/clear/even reset sequence.  The
    /// implementation's intermediate odd state is intentionally unreachable
    /// to endpoints under the explicit quiescence precondition.
    pub fn reset_quiescent(&mut self, endpoints_stopped: bool)
        requires
            old(self).valid(),
            endpoints_stopped,
        ensures
            self.valid(),
            self.write_idx == 0,
            self.read_idx == 0,
            self.capacity == old(self).capacity,
            self.generation % 2 == 0,
            !self.resynced,
    {
        self.write_idx = 0;
        self.read_idx = 0;
        self.generation = if self.generation >= 0xffff_fffe {
            FIRST_STABLE_GENERATION
        } else {
            self.generation + 2
        };
        self.resynced = false;
    }

    pub fn resync(&mut self)
        requires old(self).valid(),
        ensures self.valid(), self.resynced,
    {
        self.resynced = true;
    }

    pub fn publish_write(&mut self)
        requires
            old(self).valid(),
            old(self).resynced,
            (old(self).write_idx + 1) % old(self).capacity != old(self).read_idx,
        ensures
            self.valid(),
            self.resynced,
            self.write_idx == (old(self).write_idx + 1) % old(self).capacity,
    {
        self.write_idx = (self.write_idx + 1) % self.capacity;
    }

    pub fn publish_read(&mut self)
        requires
            old(self).valid(),
            old(self).resynced,
            old(self).write_idx != old(self).read_idx,
        ensures
            self.valid(),
            self.resynced,
            self.read_idx == (old(self).read_idx + 1) % old(self).capacity,
    {
        self.read_idx = (self.read_idx + 1) % self.capacity;
    }
}

} // verus!

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
    fn rejected_concurrent_reset_interleaving_invalidates_stale_handle() {
        let header = header();
        let stale = header.resynced_endpoint().unwrap();

        // This deliberately violates the safety contract to preserve the
        // counterexample that killed the earlier concurrent-reset designs.
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
