use verus_builtin_macros::verus;

#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {

pub const VERIFIED_RING_SIZE: u32 = 64;

/// Executable modulo operation used by the concrete TX/RX permit path.
pub fn verified_slot(counter: u32) -> (slot: usize)
    ensures
        slot < VERIFIED_RING_SIZE as usize,
        slot == (counter % VERIFIED_RING_SIZE) as usize,
{
    (counter % VERIFIED_RING_SIZE) as usize
}

/// Ownership state corresponding to one existing entry's `VALID` bit.
pub struct EntryOwnership {
    pub consumer_owned: bool,
}

impl EntryOwnership {
    pub fn released() -> (state: Self)
        ensures !state.consumer_owned,
    {
        Self { consumer_owned: false }
    }

    /// The producer cannot publish an entry until the consumer released it.
    pub fn publish(&mut self)
        requires !old(self).consumer_owned,
        ensures self.consumer_owned,
    {
        self.consumer_owned = true;
    }

    /// The consumer returns ownership to the producer.
    pub fn release(&mut self)
        requires old(self).consumer_owned,
        ensures !self.consumer_owned,
    {
        self.consumer_owned = false;
    }
}

/// Logical form of either concrete sequence-counter pair:
/// TX write/read or RX write/read.
pub struct SpscCounters {
    pub write_counter: u64,
    pub read_counter: u64,
}

impl SpscCounters {
    pub open spec fn valid(&self) -> bool {
        self.read_counter <= self.write_counter
            && self.write_counter - self.read_counter <= VERIFIED_RING_SIZE as u64
    }

    pub fn empty() -> (state: Self)
        ensures
            state.valid(),
            state.write_counter == 0u64,
            state.read_counter == 0u64,
    {
        Self {
            write_counter: 0,
            read_counter: 0,
        }
    }

    pub fn publish(&mut self)
        requires
            old(self).valid(),
            old(self).write_counter - old(self).read_counter < VERIFIED_RING_SIZE as u64,
        ensures
            self.valid(),
            self.write_counter == old(self).write_counter + 1u64,
            self.read_counter == old(self).read_counter,
    {
        self.write_counter = self.write_counter + 1u64;
    }

    pub fn release(&mut self)
        requires
            old(self).valid(),
            old(self).read_counter < old(self).write_counter,
        ensures
            self.valid(),
            self.read_counter == old(self).read_counter + 1u64,
            self.write_counter == old(self).write_counter,
    {
        self.read_counter = self.read_counter + 1u64;
    }
}

} // verus!
