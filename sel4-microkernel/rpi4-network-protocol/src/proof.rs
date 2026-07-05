//! Executable ownership guards for the existing `NetSharedMemory` TX/RX rings.
//!
//! Both directions use wrapping sequence counters and `counter % RING_SIZE`.
//! These helpers bind the Verus-checked slot calculation to the existing
//! `VALID` ownership bit and concrete outstanding-entry count.

#[path = "ring_contract.rs"]
mod ring_contract;

use crate::{ring_flags, RING_SIZE};
use ring_contract::verified_slot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermitError {
    RingFull,
    RingEmpty,
    EntryNotReleased,
    EntryNotPublished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProducerPermit {
    slot: usize,
}

impl ProducerPermit {
    pub const fn slot(self) -> usize {
        self.slot
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsumerPermit {
    slot: usize,
}

impl ConsumerPermit {
    pub const fn slot(self) -> usize {
        self.slot
    }
}

pub const fn occupancy(write_counter: u32, read_counter: u32) -> u32 {
    write_counter.wrapping_sub(read_counter)
}

pub fn slot_for(counter: u32) -> usize {
    verified_slot(counter)
}

/// Grant the sole producer access to the current free slot.
pub fn producer_permit(
    write_counter: u32,
    read_counter: u32,
    entry_flags: u32,
) -> Result<ProducerPermit, PermitError> {
    if occupancy(write_counter, read_counter) >= RING_SIZE as u32 {
        return Err(PermitError::RingFull);
    }
    if entry_flags & ring_flags::VALID != 0 {
        return Err(PermitError::EntryNotReleased);
    }
    Ok(ProducerPermit {
        slot: slot_for(write_counter),
    })
}

/// Grant the sole consumer access to the current published slot.
pub fn consumer_permit(
    write_counter: u32,
    read_counter: u32,
    entry_flags: u32,
) -> Result<ConsumerPermit, PermitError> {
    if occupancy(write_counter, read_counter) == 0 {
        return Err(PermitError::RingEmpty);
    }
    if entry_flags & ring_flags::VALID == 0 {
        return Err(PermitError::EntryNotPublished);
    }
    Ok(ConsumerPermit {
        slot: slot_for(read_counter),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concrete_slot_is_always_in_bounds() {
        for counter in [0, 1, 63, 64, 65, u32::MAX] {
            assert!(slot_for(counter) < RING_SIZE);
        }
    }

    #[test]
    fn producer_cannot_reuse_published_entry() {
        assert_eq!(
            producer_permit(0, 0, ring_flags::VALID),
            Err(PermitError::EntryNotReleased)
        );
    }

    #[test]
    fn producer_cannot_overrun_consumer() {
        assert_eq!(
            producer_permit(RING_SIZE as u32, 0, 0),
            Err(PermitError::RingFull)
        );
    }

    #[test]
    fn consumer_requires_a_published_entry() {
        assert_eq!(
            consumer_permit(1, 0, 0),
            Err(PermitError::EntryNotPublished)
        );
        assert_eq!(
            consumer_permit(0, 0, ring_flags::VALID),
            Err(PermitError::RingEmpty)
        );
    }

    #[test]
    fn valid_permits_use_the_existing_slot_formula() {
        assert_eq!(producer_permit(65, 64, 0).unwrap().slot(), 1);
        assert_eq!(
            consumer_permit(65, 64, ring_flags::VALID)
                .unwrap()
                .slot(),
            0
        );
    }
}
