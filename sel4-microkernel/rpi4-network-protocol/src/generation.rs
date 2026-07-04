//! IC-1 restart-safe header for network SPSC rings.
//!
//! This is additive: the legacy `NetSharedMemory` layout remains unchanged.
//! Restart-aware products place this 16-byte header immediately before each
//! ring's entries and use the typed endpoint API below.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::generation_contract::{reset_plan, validate_stable_generation};
use crate::RING_SIZE;

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

/// Linear evidence that the lifecycle PD has stopped both endpoints.
#[derive(Debug)]
pub struct EndpointsStopped(());

impl EndpointsStopped {
    /// # Safety
    /// Both producer and consumer must be stopped and unable to publish.
    pub unsafe fn new_unchecked() -> Self {
        Self(())
    }
}

#[repr(C, align(16))]
pub struct NetworkRingHeader {
    pub write_idx: AtomicU32,
    pub read_idx: AtomicU32,
    pub capacity: u32,
    pub generation_word: AtomicU32,
}

impl NetworkRingHeader {
    pub const fn new(capacity: u32) -> Self {
        assert!(capacity > 1);
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
        let stable = validate_stable_generation(observed)
            .map_err(|observed| OddGeneration { observed })?;
        debug_assert!(self.capacity > 1);
        debug_assert!(self.write_idx.load(Ordering::Acquire) < self.capacity);
        debug_assert!(self.read_idx.load(Ordering::Acquire) < self.capacity);
        Ok(Generation(stable))
    }

    pub fn resynced_endpoint(&self) -> Result<ResyncedNetworkRing<'_>, OddGeneration> {
        Ok(ResyncedNetworkRing {
            header: self,
            generation: self.resync()?,
        })
    }

    /// Execute the IC-1 odd/clear/even sequence.
    ///
    /// The verified executable `reset_plan` computes the publication values.
    /// The odd marker is diagnostic only; live endpoints must never rely on it
    /// as synchronization. Endpoint quiescence is the correctness mechanism.
    ///
    /// # Safety
    /// The token must truthfully represent the current quiescent window.
    pub unsafe fn quiescent_reset(
        &self,
        stopped: EndpointsStopped,
    ) -> Result<Generation, OddGeneration> {
        let current = self.generation_word.load(Ordering::Acquire);
        let (odd, next_even) =
            reset_plan(current).map_err(|observed| OddGeneration { observed })?;

        let _consumed = stopped;
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

    /// Defense-in-depth stale-handle check followed by publication.
    ///
    /// This check and the index store are not atomic. A concurrent reset would
    /// still race; the lifecycle PD must stop both endpoints before reset.
    pub fn advance_write(&self) -> Result<(), GenerationChanged> {
        self.check_generation()?;
        let current = self.header.write_idx.load(Ordering::Acquire);
        let next = (current + 1) % self.header.capacity;
        self.header.write_idx.store(next, Ordering::Release);
        Ok(())
    }

    /// See [`Self::advance_write`] for the quiescence requirement.
    pub fn advance_read(&self) -> Result<(), GenerationChanged> {
        self.check_generation()?;
        let current = self.header.read_idx.load(Ordering::Acquire);
        let next = (current + 1) % self.header.capacity;
        self.header.read_idx.store(next, Ordering::Release);
        Ok(())
    }
}

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
    fn stale_handle_is_detected_after_quiescent_reset() {
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

    #[test]
    #[should_panic]
    fn zero_capacity_is_rejected_at_construction() {
        let _ = NetworkRingHeader::new(0);
    }
}
