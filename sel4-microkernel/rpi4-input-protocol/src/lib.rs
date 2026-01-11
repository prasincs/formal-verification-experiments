//! # Input IPC Protocol
//!
//! Shared data structures for IPC between Input PD and Graphics PD.
//! Uses a lock-free single-producer single-consumer ring buffer.
//!
//! ## Memory Layout (4KB shared region)
//!
//! ```text
//! +-------------------+ 0x000
//! | InputRingHeader   | (16 bytes)
//! +-------------------+ 0x010
//! | InputRingEntry[0] | (4 bytes each)
//! | InputRingEntry[1] |
//! | ...               |
//! | InputRingEntry[N] |
//! +-------------------+ 0x1000
//! ```

#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

/// Channel ID for input notifications
pub const INPUT_CHANNEL_ID: usize = 1;

/// Ring buffer capacity (number of entries)
/// With 4-byte entries and 16-byte header, we can fit ~1000 entries in 4KB
pub const RING_CAPACITY: u32 = 1000;

/// Event types
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventType {
    /// No event (empty slot)
    None = 0,
    /// Keyboard key event
    Key = 1,
    /// IR remote button event
    IrRemote = 2,
}

/// Key state
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    Released = 0,
    Pressed = 1,
}

/// Ring buffer header (must be 16-byte aligned for atomics)
#[repr(C, align(16))]
pub struct InputRingHeader {
    /// Write index (written by Input PD only)
    pub write_idx: AtomicU32,
    /// Read index (written by Graphics PD only)
    pub read_idx: AtomicU32,
    /// Capacity (set once at init)
    pub capacity: u32,
    /// Padding to 16 bytes
    _pad: u32,
}

impl InputRingHeader {
    /// Initialize a new ring buffer header
    ///
    /// # Safety
    /// The pointer must point to properly mapped shared memory.
    pub unsafe fn init(ptr: *mut Self) {
        (*ptr).write_idx = AtomicU32::new(0);
        (*ptr).read_idx = AtomicU32::new(0);
        (*ptr).capacity = RING_CAPACITY;
        (*ptr)._pad = 0;
    }

    /// Check if the ring buffer has data available
    pub fn has_data(&self) -> bool {
        let write = self.write_idx.load(Ordering::Acquire);
        let read = self.read_idx.load(Ordering::Acquire);
        write != read
    }

    /// Check if the ring buffer is full
    pub fn is_full(&self) -> bool {
        let write = self.write_idx.load(Ordering::Acquire);
        let read = self.read_idx.load(Ordering::Acquire);
        ((write + 1) % self.capacity) == read
    }

    /// Get the next write index (for producer)
    pub fn next_write_idx(&self) -> u32 {
        (self.write_idx.load(Ordering::Acquire) + 1) % self.capacity
    }

    /// Advance the write index (called after writing entry)
    pub fn advance_write(&self) {
        let next = self.next_write_idx();
        self.write_idx.store(next, Ordering::Release);
    }

    /// Get the current read index
    pub fn current_read_idx(&self) -> u32 {
        self.read_idx.load(Ordering::Acquire)
    }

    /// Advance the read index (called after reading entry)
    pub fn advance_read(&self) {
        let next = (self.read_idx.load(Ordering::Acquire) + 1) % self.capacity;
        self.read_idx.store(next, Ordering::Release);
    }
}

/// A single input event entry in the ring buffer
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct InputRingEntry {
    /// Event type (Key, IrRemote, etc.)
    pub event_type: u8,
    /// Key code (from rpi4_input::KeyCode as u8)
    pub key_code: u8,
    /// Key state (Pressed/Released)
    pub key_state: u8,
    /// Modifier flags
    pub modifiers: u8,
}

impl InputRingEntry {
    /// Create a new key event entry
    pub const fn key(code: u8, state: KeyState, modifiers: u8) -> Self {
        Self {
            event_type: EventType::Key as u8,
            key_code: code,
            key_state: state as u8,
            modifiers,
        }
    }

    /// Create an empty entry
    pub const fn empty() -> Self {
        Self {
            event_type: EventType::None as u8,
            key_code: 0,
            key_state: 0,
            modifiers: 0,
        }
    }

    /// Check if this is a key pressed event
    pub fn is_key_pressed(&self) -> bool {
        self.event_type == EventType::Key as u8 && self.key_state == KeyState::Pressed as u8
    }
}

/// Size of the header in bytes
pub const HEADER_SIZE: usize = core::mem::size_of::<InputRingHeader>();

/// Size of each entry in bytes
pub const ENTRY_SIZE: usize = core::mem::size_of::<InputRingEntry>();

/// Offset of entries from start of shared memory
pub const ENTRIES_OFFSET: usize = 16; // After 16-byte header

/// Helper to get entry array pointer from shared memory base
///
/// # Safety
/// The base pointer must be valid and properly aligned.
pub unsafe fn entries_ptr(base: *mut u8) -> *mut InputRingEntry {
    base.add(ENTRIES_OFFSET) as *mut InputRingEntry
}

/// Helper to get header pointer from shared memory base
///
/// # Safety
/// The base pointer must be valid and properly aligned.
pub unsafe fn header_ptr(base: *mut u8) -> *mut InputRingHeader {
    base as *mut InputRingHeader
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_size() {
        assert_eq!(ENTRY_SIZE, 4);
    }

    #[test]
    fn test_header_size() {
        assert_eq!(HEADER_SIZE, 16);
    }
}
