//! Verified input IPC protocol for the isolated input/graphics products.
//!
//! The legacy ABI remains unchanged. Wave 1 adds restart-generation handling
//! alongside the existing entry and index APIs from this crate root.

#![no_std]
#![allow(unused)]
#![allow(clippy::assign_op_pattern)]
#![allow(clippy::new_without_default)]

use core::sync::atomic::{AtomicU32, Ordering};
use verus_builtin_macros::verus;

verus! {

pub const INPUT_CHANNEL_ID: usize = 1;
pub const RING_CAPACITY: u32 = 1000;
pub const HEADER_SIZE: usize = 16;
pub const ENTRY_SIZE: usize = 4;
pub const ENTRIES_OFFSET: usize = 16;

pub const KEY_CODE_MAX: u8 = 40;
pub const KEY_UP: u8 = 1;
pub const KEY_DOWN: u8 = 2;
pub const KEY_LEFT: u8 = 3;
pub const KEY_RIGHT: u8 = 4;
pub const KEY_ENTER: u8 = 5;
pub const KEY_ESCAPE: u8 = 6;
pub const KEY_SPACE: u8 = 7;
pub const KEY_NUM0: u8 = 10;
pub const KEY_NUM9: u8 = 19;
pub const KEY_HOME: u8 = 20;
pub const KEY_END: u8 = 21;
pub const KEY_PAGEUP: u8 = 22;
pub const KEY_PAGEDOWN: u8 = 23;
pub const KEY_VOLUMEUP: u8 = 30;
pub const KEY_VOLUMEDOWN: u8 = 31;
pub const KEY_MUTE: u8 = 32;
pub const KEY_UNKNOWN: u8 = 0;

pub open spec fn valid_key_code(code: u8) -> bool {
    code <= KEY_CODE_MAX
}

pub const EVENT_NONE: u8 = 0;
pub const EVENT_KEY: u8 = 1;
pub const EVENT_IR: u8 = 2;

pub open spec fn valid_event_type(value: u8) -> bool {
    value == EVENT_NONE || value == EVENT_KEY || value == EVENT_IR
}

pub const STATE_RELEASED: u8 = 0;
pub const STATE_PRESSED: u8 = 1;

pub open spec fn valid_key_state(value: u8) -> bool {
    value == STATE_RELEASED || value == STATE_PRESSED
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct InputRingEntry {
    pub event_type: u8,
    pub key_code: u8,
    pub key_state: u8,
    pub modifiers: u8,
}

impl InputRingEntry {
    pub open spec fn valid(&self) -> bool {
        valid_event_type(self.event_type)
            && valid_key_code(self.key_code)
            && valid_key_state(self.key_state)
    }

    pub fn new_key(code: u8, state: u8, modifiers: u8) -> (entry: Self)
        requires
            valid_key_code(code),
            valid_key_state(state),
        ensures
            entry.valid(),
            entry.event_type == EVENT_KEY,
            entry.key_code == code,
            entry.key_state == state,
            entry.modifiers == modifiers,
    {
        Self {
            event_type: EVENT_KEY,
            key_code: code,
            key_state: state,
            modifiers,
        }
    }

    pub fn empty() -> (entry: Self)
        ensures
            entry.valid(),
            entry.event_type == EVENT_NONE,
    {
        Self {
            event_type: EVENT_NONE,
            key_code: 0,
            key_state: 0,
            modifiers: 0,
        }
    }

    pub fn is_key_pressed(&self) -> (result: bool)
        requires self.valid(),
        ensures result == (self.event_type == EVENT_KEY && self.key_state == STATE_PRESSED),
    {
        self.event_type == EVENT_KEY && self.key_state == STATE_PRESSED
    }
}

pub struct RingIndices {
    write_idx: u32,
    read_idx: u32,
    capacity: u32,
}

impl RingIndices {
    pub open spec fn valid(&self) -> bool {
        self.capacity > 0
            && self.capacity <= RING_CAPACITY
            && self.write_idx < self.capacity
            && self.read_idx < self.capacity
    }

    pub open spec fn count(&self) -> u32
        recommends self.valid()
    {
        if self.write_idx >= self.read_idx {
            self.write_idx - self.read_idx
        } else {
            self.capacity - self.read_idx + self.write_idx
        }
    }

    pub open spec fn is_empty_spec(&self) -> bool
        recommends self.valid()
    {
        self.write_idx == self.read_idx
    }

    pub open spec fn is_full_spec(&self) -> bool
        recommends self.valid()
    {
        (self.write_idx + 1) % self.capacity == self.read_idx
    }

    pub fn new(capacity: u32) -> (indices: Self)
        requires
            capacity > 0,
            capacity <= RING_CAPACITY,
        ensures
            indices.valid(),
            indices.is_empty_spec(),
            !indices.is_full_spec(),
    {
        Self {
            write_idx: 0,
            read_idx: 0,
            capacity,
        }
    }

    pub fn is_empty(&self) -> (result: bool)
        requires self.valid(),
        ensures result == self.is_empty_spec(),
    {
        self.write_idx == self.read_idx
    }

    pub fn is_full(&self) -> (result: bool)
        requires self.valid(),
        ensures result == self.is_full_spec(),
    {
        (self.write_idx + 1) % self.capacity == self.read_idx
    }

    pub fn has_data(&self) -> (result: bool)
        requires self.valid(),
        ensures result == !self.is_empty_spec(),
    {
        self.write_idx != self.read_idx
    }

    pub fn write_index(&self) -> (result: u32)
        requires self.valid(),
        ensures result == self.write_idx, result < self.capacity,
    {
        self.write_idx
    }

    pub fn read_index(&self) -> (result: u32)
        requires self.valid(),
        ensures result == self.read_idx, result < self.capacity,
    {
        self.read_idx
    }

    pub fn advance_write(&mut self)
        requires
            old(self).valid(),
            !old(self).is_full_spec(),
        ensures
            self.valid(),
            self.write_idx == (old(self).write_idx + 1) % old(self).capacity,
            self.read_idx == old(self).read_idx,
            self.capacity == old(self).capacity,
    {
        self.write_idx = (self.write_idx + 1) % self.capacity;
    }

    pub fn advance_read(&mut self)
        requires
            old(self).valid(),
            !old(self).is_empty_spec(),
        ensures
            self.valid(),
            self.read_idx == (old(self).read_idx + 1) % old(self).capacity,
            self.write_idx == old(self).write_idx,
            self.capacity == old(self).capacity,
    {
        self.read_idx = (self.read_idx + 1) % self.capacity;
    }
}

pub const RING_BUFFER_VADDR: usize = 0x5_0400_0000;
pub const RING_BUFFER_SIZE: usize = 0x1000;

pub open spec fn in_ring_buffer_region(address: usize) -> bool {
    address >= RING_BUFFER_VADDR && address < RING_BUFFER_VADDR + RING_BUFFER_SIZE
}

pub open spec fn valid_entry_index(index: u32) -> bool {
    (index as usize) < RING_CAPACITY as usize
        && ENTRIES_OFFSET + (index as usize) * ENTRY_SIZE < RING_BUFFER_SIZE
}

pub fn entry_address(base: usize, index: u32) -> (address: usize)
    requires
        base == RING_BUFFER_VADDR,
        valid_entry_index(index),
    ensures
        in_ring_buffer_region(address),
        address == base + ENTRIES_OFFSET + (index as usize) * ENTRY_SIZE,
{
    base + ENTRIES_OFFSET + (index as usize) * ENTRY_SIZE
}

pub const INPUT_PD_UART_BASE: usize = 0x5_0300_0000;
pub const INPUT_PD_UART_SIZE: usize = 0x1000;
pub const INPUT_PD_USB_REGS_BASE: usize = 0x5_0500_0000;
pub const INPUT_PD_USB_REGS_SIZE: usize = 0x10000;
pub const INPUT_PD_USB_DMA_BASE: usize = 0x5_0600_0000;
pub const INPUT_PD_USB_DMA_SIZE: usize = 0x1000;

pub open spec fn input_pd_can_access(address: usize) -> bool {
    (address >= INPUT_PD_UART_BASE && address < INPUT_PD_UART_BASE + INPUT_PD_UART_SIZE)
        || (address >= INPUT_PD_USB_REGS_BASE
            && address < INPUT_PD_USB_REGS_BASE + INPUT_PD_USB_REGS_SIZE)
        || (address >= INPUT_PD_USB_DMA_BASE
            && address < INPUT_PD_USB_DMA_BASE + INPUT_PD_USB_DMA_SIZE)
        || in_ring_buffer_region(address)
}

pub const GRAPHICS_PD_MAILBOX_BASE: usize = 0x5_0000_0000;
pub const GRAPHICS_PD_MAILBOX_SIZE: usize = 0x1000;
pub const GRAPHICS_PD_GPIO_BASE: usize = 0x5_0200_0000;
pub const GRAPHICS_PD_GPIO_SIZE: usize = 0x1000;
pub const GRAPHICS_PD_FB_BASE: usize = 0x5_0001_0000;
pub const GRAPHICS_PD_FB_SIZE: usize = 0x1000000;
pub const GRAPHICS_PD_DMA_BASE: usize = 0x5_0300_0000;
pub const GRAPHICS_PD_DMA_SIZE: usize = 0x1000;

pub open spec fn graphics_pd_can_access(address: usize) -> bool {
    (address >= GRAPHICS_PD_MAILBOX_BASE
        && address < GRAPHICS_PD_MAILBOX_BASE + GRAPHICS_PD_MAILBOX_SIZE)
        || (address >= GRAPHICS_PD_GPIO_BASE
            && address < GRAPHICS_PD_GPIO_BASE + GRAPHICS_PD_GPIO_SIZE)
        || (address >= GRAPHICS_PD_FB_BASE
            && address < GRAPHICS_PD_FB_BASE + GRAPHICS_PD_FB_SIZE)
        || (address >= GRAPHICS_PD_DMA_BASE
            && address < GRAPHICS_PD_DMA_BASE + GRAPHICS_PD_DMA_SIZE)
        || in_ring_buffer_region(address)
}

} // verus!

/// Runtime ring-buffer header. The final word remains at offset 0x0c and is
/// interpreted by `generation` without changing the legacy ABI.
#[repr(C, align(16))]
pub struct InputRingHeader {
    pub write_idx: AtomicU32,
    pub read_idx: AtomicU32,
    pub capacity: u32,
    _pad: u32,
}

impl InputRingHeader {
    /// # Safety
    /// `ptr` must be valid, writable, and aligned for `InputRingHeader`.
    pub unsafe fn init(ptr: *mut Self) {
        (*ptr).write_idx = AtomicU32::new(0);
        (*ptr).read_idx = AtomicU32::new(0);
        (*ptr).capacity = RING_CAPACITY;
        (*ptr)._pad = 0;
    }

    pub fn has_data(&self) -> bool {
        self.current_write_idx() != self.current_read_idx()
    }

    pub fn is_full(&self) -> bool {
        ((self.current_write_idx() + 1) % self.capacity) == self.current_read_idx()
    }

    pub fn current_write_idx(&self) -> u32 {
        self.write_idx.load(Ordering::Acquire)
    }

    pub fn current_read_idx(&self) -> u32 {
        self.read_idx.load(Ordering::Acquire)
    }

    pub fn advance_write(&self) {
        let next = (self.current_write_idx() + 1) % self.capacity;
        self.write_idx.store(next, Ordering::Release);
    }

    pub fn advance_read(&self) {
        let next = (self.current_read_idx() + 1) % self.capacity;
        self.read_idx.store(next, Ordering::Release);
    }
}

/// # Safety
/// `base` must be a valid shared-memory address with the protocol alignment.
pub unsafe fn header_ptr(base: *mut u8) -> *mut InputRingHeader {
    base as *mut InputRingHeader
}

/// # Safety
/// `base` must be a valid shared-memory address for the full ring region.
pub unsafe fn entries_ptr(base: *mut u8) -> *mut InputRingEntry {
    base.add(ENTRIES_OFFSET) as *mut InputRingEntry
}

pub use self::STATE_PRESSED as KeyStatePressed;
pub use self::STATE_RELEASED as KeyStateReleased;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    Released = 0,
    Pressed = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventType {
    None = 0,
    Key = 1,
    IrRemote = 2,
}

impl InputRingEntry {
    pub const fn key(code: u8, state: KeyState, modifiers: u8) -> Self {
        Self {
            event_type: EVENT_KEY,
            key_code: code,
            key_state: state as u8,
            modifiers,
        }
    }
}

mod generation_contract;
mod generation;
pub use generation::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_size_is_stable() {
        assert_eq!(core::mem::size_of::<InputRingEntry>(), ENTRY_SIZE);
    }

    #[test]
    fn header_size_is_stable() {
        assert_eq!(core::mem::size_of::<InputRingHeader>(), HEADER_SIZE);
    }

    #[test]
    fn legacy_indices_still_work() {
        let mut indices = RingIndices::new(10);
        assert!(indices.is_empty());
        indices.advance_write();
        assert!(indices.has_data());
        indices.advance_read();
        assert!(indices.is_empty());
    }
}
