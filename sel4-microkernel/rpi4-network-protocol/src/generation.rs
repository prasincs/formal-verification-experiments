//! IC-1 restart-safe header for network SPSC rings.
//!
//! This is additive: the legacy `NetSharedMemory` layout remains unchanged.
//! Restart-aware products place this 16-byte header immediately before each
//! ring's entries and use the typed endpoint API below.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::RING_SIZE;
use verus_builtin_macros::verus;

pub const LEGACY_GENERATION: u32 = 0;
pub const FIRST_STABLE_GENERATION: u32 = 2;

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

/// Evidence that the lifecycle PD has stopped both endpoints.
#[derive(Clone, Copy, Debug)]
pub struct EndpointsStopped(());

impl EndpointsStopped {
    /// # Safety
    /// Both producer and consumer must be stopped and unable to publish.
    pub unsafe fn new_unchecked() -> Self {
        Self(())
    }
}

/// Canonical IC-1 header.  The generation word is at offset `0x0c`.
#[repr(C, align(16))]
pub struct NetworkRingHeader {
    pub write_idx: AtomicU32,
    pub read_idx: AtomicU32,
    pub capacity: u32,
    pub generation_word: AtomicU32,
}

impl NetworkRingHeader {
    pub const fn new(capacity: u32) -> Self {
        Self {
            write_idx: AtomicU32::new(0),
            read_idx: AtomicU32::new(0),
            capacity,
            generation_word: AtomicU32::new(LEGACY_GENERATION),
        }
    }

    pub fn generation(&self) -> Generation {
        Generation(self.generation_word.load(Ordering::Acquire))
    }

    pub fn resync(&self) -> Result<Generation, OddGeneration> {
        let observed = self.generation_word.load(Ordering::Acquire);
        if observed & 1 != 0 {
            return Err(OddGeneration { observed });
        }
        debug_assert!(self.capacity > 0);
        debug_assert!(self.write_idx.load(Ordering::Acquire) < self.capacity);
        debug_assert!(self.read_idx.load(Ordering::Acquire) < self.capacity);
        Ok(Generation(observed))
    }

    pub fn resynced_endpoint(&self) -> Result<ResyncedNetworkRing<'_>, OddGeneration> {
        Ok(ResyncedNetworkRing {
            header: self,
            generation: self.resync()?,
        })
    }

    /// Execute the IC-1 odd/clear/even sequence.
    ///
    /// # Safety
    /// The token must truthfully represent that both endpoints are stopped.
    pub unsafe fn quiescent_reset(
        &self,
        _stopped: EndpointsStopped,
    ) -> Result<Generation, OddGeneration> {
        let current = self.generation_word.load(Ordering::Acquire);
        if current & 1 != 0 {
            return Err(OddGeneration { observed: current });
        }

        let odd = current.wrapping_add(1);
        let mut next_even = odd.wrapping_add(1);
        if next_even == LEGACY_GENERATION {
            next_even = FIRST_STABLE_GENERATION;
        }

        self.generation_word.store(odd, Ordering::Release);
        self.write_idx.store(0, Ordering::Release);
        self.read_idx.store(0, Ordering::Release);
        self.generation_word.store(next_even, Ordering::Release);
        Ok(Generation(next_even))
    }
}

impl Default for NetworkRingHeader {
    fn default() -> Self {
        Self::new(RING_SIZE as u32)
    }
}

pub struct ResyncedNetworkRing<'a> {
    header: &'a NetworkRingHeader,
    generation: Generation,
}

impl ResyncedNetworkRing<'_> {
    pub const fn generation(&self) -> Generation {
        self.generation
    }

    fn check_generation(&self) -> Result<(), GenerationChanged> {
        let observed = self.header.generation_word.load(Ordering::Acquire);
        if observed == self.generation.get() && observed & 1 == 0 {
            Ok(())
        } else {
            Err(GenerationChanged {
                expected: self.generation,
                observed,
            })
        }
    }

    pub fn write_index(&self) -> Result<u32, GenerationChanged> {
        self.check_generation()?;
        Ok(self.header.write_idx.load(Ordering::Acquire))
    }

    pub fn read_index(&self) -> Result<u32, GenerationChanged> {
        self.check_generation()?;
        Ok(self.header.read_idx.load(Ordering::Acquire))
    }

    pub fn advance_write(&self) -> Result<(), GenerationChanged> {
        self.check_generation()?;
        let current = self.header.write_idx.load(Ordering::Acquire);
        let next = (current + 1) % self.header.capacity;
        self.header.write_idx.store(next, Ordering::Release);
        Ok(())
    }

    pub fn advance_read(&self) -> Result<(), GenerationChanged> {
        self.check_generation()?;
        let current = self.header.read_idx.load(Ordering::Acquire);
        let next = (current + 1) % self.header.capacity;
        self.header.read_idx.store(next, Ordering::Release);
        Ok(())
    }
}

verus! {

pub struct NetworkRestartModel {
    pub write_idx: u32,
    pub read_idx: u32,
    pub capacity: u32,
    pub generation: u32,
    pub resynced: bool,
}

impl NetworkRestartModel {
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

    pub fn reset_quiescent(&mut self, endpoints_stopped: bool)
        requires old(self).valid(), endpoints_stopped,
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
}

} // verus!

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn stopped() -> EndpointsStopped {
        EndpointsStopped::new_unchecked()
    }

    #[test]
    fn header_layout_matches_ic1() {
        assert_eq!(core::mem::size_of::<NetworkRingHeader>(), 16);
        let header = NetworkRingHeader::default();
        let base = &header as *const _ as usize;
        let generation = &header.generation_word as *const _ as usize;
        assert_eq!(generation - base, 0x0c);
    }

    #[test]
    fn producer_consumer_and_both_restart() {
        let header = NetworkRingHeader::default();
        let first = header.resynced_endpoint().unwrap();
        first.advance_write().unwrap();
        first.advance_read().unwrap();

        let next = unsafe { header.quiescent_reset(stopped()).unwrap() };
        assert_eq!(next.get(), FIRST_STABLE_GENERATION);
        assert!(first.advance_write().is_err());

        let restarted = header.resynced_endpoint().unwrap();
        assert_eq!(restarted.write_index().unwrap(), 0);
        assert_eq!(restarted.read_index().unwrap(), 0);
    }

    #[test]
    fn odd_generation_parks_endpoint() {
        let header = NetworkRingHeader::default();
        header.generation_word.store(7, Ordering::Release);
        assert_eq!(header.resync(), Err(OddGeneration { observed: 7 }));
    }

    #[test]
    fn wrap_skips_generation_zero() {
        let header = NetworkRingHeader::default();
        header
            .generation_word
            .store(u32::MAX - 1, Ordering::Release);
        let next = unsafe { header.quiescent_reset(stopped()).unwrap() };
        assert_eq!(next.get(), FIRST_STABLE_GENERATION);
    }

    #[test]
    fn stale_handle_detects_rejected_concurrent_reset() {
        let header = NetworkRingHeader::default();
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
