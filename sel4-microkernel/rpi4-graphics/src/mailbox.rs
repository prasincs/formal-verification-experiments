//! # VideoCore Mailbox Driver
//!
//! Communicates with the VideoCore GPU via the mailbox interface.
//! Used to allocate and configure the framebuffer.
//!
//! ## Reference
//! - https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interface

use core::ptr::{read_volatile, write_volatile};

/// Mailbox register offsets from base
const MAILBOX_READ: usize = 0x00;
const MAILBOX_STATUS: usize = 0x18;
const MAILBOX_WRITE: usize = 0x20;

/// Status register bits
const MAILBOX_FULL: u32 = 0x8000_0000;
const MAILBOX_EMPTY: u32 = 0x4000_0000;

/// Mailbox channels
const CHANNEL_PROPERTY: u32 = 8;

/// Property tag request/response codes
const REQUEST_CODE: u32 = 0x0000_0000;
const RESPONSE_SUCCESS: u32 = 0x8000_0000;

/// Property tags for framebuffer
pub mod tags {
    pub const SET_PHYSICAL_SIZE: u32 = 0x0004_8003;
    pub const SET_VIRTUAL_SIZE: u32 = 0x0004_8004;
    pub const SET_VIRTUAL_OFFSET: u32 = 0x0004_8009;
    pub const SET_DEPTH: u32 = 0x0004_8005;
    pub const SET_PIXEL_ORDER: u32 = 0x0004_8006;
    pub const ALLOCATE_BUFFER: u32 = 0x0004_0001;
    pub const GET_PITCH: u32 = 0x0004_0008;

    // Verification tags
    pub const GET_FIRMWARE_REV: u32 = 0x0000_0001;
    pub const GET_BOARD_MODEL: u32 = 0x0001_0001;
    pub const GET_BOARD_REVISION: u32 = 0x0001_0002;
    pub const GET_BOARD_SERIAL: u32 = 0x0001_0004;
    pub const GET_ARM_MEMORY: u32 = 0x0001_0005;
    pub const GET_VC_MEMORY: u32 = 0x0001_0006;
}

/// Mailbox communication errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxError {
    /// Mailbox request failed
    RequestFailed,
    /// Invalid response from GPU
    InvalidResponse,
    /// Timeout waiting for mailbox
    Timeout,
    /// Buffer allocation failed
    AllocationFailed,
}

/// Mailbox driver for VideoCore communication
pub struct Mailbox {
    base: usize,
}

impl Mailbox {
    /// Create a new mailbox driver
    ///
    /// # Safety
    /// The base address must be a valid mapped address for the mailbox registers.
    pub const unsafe fn new(base: usize) -> Self {
        Self { base }
    }

    /// Read from a mailbox register
    #[inline]
    fn read_reg(&self, offset: usize) -> u32 {
        unsafe { read_volatile((self.base + offset) as *const u32) }
    }

    /// Write to a mailbox register
    #[inline]
    fn write_reg(&self, offset: usize, value: u32) {
        unsafe { write_volatile((self.base + offset) as *mut u32, value) }
    }

    /// Wait until mailbox is not full (can write)
    fn wait_write_ready(&self) {
        let mut timeout = 1_000_000u32;
        while (self.read_reg(MAILBOX_STATUS) & MAILBOX_FULL) != 0 {
            timeout = timeout.saturating_sub(1);
            if timeout == 0 {
                return;
            }
            core::hint::spin_loop();
        }
    }

    /// Wait until mailbox is not empty (can read)
    fn wait_read_ready(&self) {
        let mut timeout = 1_000_000u32;
        while (self.read_reg(MAILBOX_STATUS) & MAILBOX_EMPTY) != 0 {
            timeout = timeout.saturating_sub(1);
            if timeout == 0 {
                return;
            }
            core::hint::spin_loop();
        }
    }

    /// Send a property tag message and get response
    ///
    /// Uses a DMA buffer at a known physical address since the GPU
    /// needs physical addresses for DMA. The buffer is copied to/from
    /// the DMA region.
    ///
    /// # Safety
    /// The DMA buffer region must be properly mapped.
    pub unsafe fn call(&self, buffer: &mut [u32; 36]) -> Result<(), MailboxError> {
        // Use the DMA buffer which has a known physical address
        let dma_virt = crate::DMA_BUFFER_VIRT as *mut u32;
        let dma_phys = crate::DMA_BUFFER_PHYS;

        // Memory barrier before DMA
        core::arch::asm!("dsb sy");

        // Copy message to DMA buffer
        for i in 0..36 {
            dma_virt.add(i).write_volatile(buffer[i]);
        }

        // Memory barrier to ensure write completes
        core::arch::asm!("dsb sy");

        // Convert physical address to GPU bus address
        let gpu_addr = crate::arm_to_gpu(dma_phys);

        // Wait for mailbox to be ready
        self.wait_write_ready();

        // Write address with channel (lower 4 bits)
        self.write_reg(MAILBOX_WRITE, (gpu_addr & !0xF) | CHANNEL_PROPERTY);

        // Wait for response
        loop {
            self.wait_read_ready();

            let response = self.read_reg(MAILBOX_READ);

            // Check if this is our response (same channel)
            if (response & 0xF) == CHANNEL_PROPERTY {
                // Memory barrier before reading response
                core::arch::asm!("dsb sy");

                // Copy response back from DMA buffer
                for i in 0..36 {
                    buffer[i] = dma_virt.add(i).read_volatile();
                }

                // Check response code
                if buffer[1] == RESPONSE_SUCCESS {
                    return Ok(());
                } else {
                    return Err(MailboxError::RequestFailed);
                }
            }
        }
    }

    /// Get firmware revision
    pub fn get_firmware_revision(&self, buffer: &mut [u32; 36]) -> Result<u32, MailboxError> {
        // Clear buffer
        for i in 0..36 {
            buffer[i] = 0;
        }

        // Build message
        buffer[0] = 8 * 4; // Buffer size
        buffer[1] = REQUEST_CODE;
        buffer[2] = tags::GET_FIRMWARE_REV;
        buffer[3] = 4; // Value buffer size
        buffer[4] = 0; // Request
        buffer[5] = 0; // Value (will be filled by GPU)
        buffer[6] = 0; // End tag
        buffer[7] = 0; // Padding

        unsafe { self.call(buffer)?; }

        Ok(buffer[5])
    }

    /// Get board model
    pub fn get_board_model(&self, buffer: &mut [u32; 36]) -> Result<u32, MailboxError> {
        for i in 0..36 {
            buffer[i] = 0;
        }

        buffer[0] = 8 * 4;
        buffer[1] = REQUEST_CODE;
        buffer[2] = tags::GET_BOARD_MODEL;
        buffer[3] = 4;
        buffer[4] = 0;
        buffer[5] = 0;
        buffer[6] = 0;
        buffer[7] = 0;

        unsafe { self.call(buffer)?; }

        Ok(buffer[5])
    }

    /// Get board serial number
    pub fn get_board_serial(&self, buffer: &mut [u32; 36]) -> Result<u64, MailboxError> {
        for i in 0..36 {
            buffer[i] = 0;
        }

        buffer[0] = 9 * 4;
        buffer[1] = REQUEST_CODE;
        buffer[2] = tags::GET_BOARD_SERIAL;
        buffer[3] = 8;
        buffer[4] = 0;
        buffer[5] = 0;
        buffer[6] = 0;
        buffer[7] = 0;
        buffer[8] = 0;

        unsafe { self.call(buffer)?; }

        Ok((buffer[6] as u64) << 32 | (buffer[5] as u64))
    }
}
