use verus_builtin_macros::verus;

#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {

pub const VERIFIED_RING_SIZE: u32 = 64;

pub fn verified_slot(counter: u32) -> (slot: usize)
    ensures
        slot < VERIFIED_RING_SIZE as usize,
        slot == (counter % VERIFIED_RING_SIZE) as usize,
{
    (counter % VERIFIED_RING_SIZE) as usize
}

pub struct EntryOwnership {
    pub consumer_owned: bool,
}

impl EntryOwnership {
    pub fn released() -> (state: Self)
        ensures !state.consumer_owned,
    {
        Self { consumer_owned: false }
    }

    pub fn publish(self) -> (next: Self)
        requires !self.consumer_owned,
        ensures next.consumer_owned,
    {
        Self { consumer_owned: true }
    }

    pub fn release(self) -> (next: Self)
        requires self.consumer_owned,
        ensures !next.consumer_owned,
    {
        Self { consumer_owned: false }
    }
}

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

    pub fn publish(self) -> (next: Self)
        requires
            self.valid(),
            self.write_counter < u64::MAX,
            self.write_counter - self.read_counter < VERIFIED_RING_SIZE as u64,
        ensures
            next.valid(),
            next.write_counter == self.write_counter + 1u64,
            next.read_counter == self.read_counter,
    {
        Self {
            write_counter: self.write_counter + 1u64,
            read_counter: self.read_counter,
        }
    }

    pub fn release(self) -> (next: Self)
        requires
            self.valid(),
            self.read_counter < self.write_counter,
        ensures
            next.valid(),
            next.read_counter == self.read_counter + 1u64,
            next.write_counter == self.write_counter,
    {
        Self {
            write_counter: self.write_counter,
            read_counter: self.read_counter + 1u64,
        }
    }
}

} // verus!
