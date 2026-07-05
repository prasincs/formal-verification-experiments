//! # Infineon SLB 9670 TPM 2.0 Driver
//!
//! Low-level driver for the Infineon Optiga SLB 9670 TPM 2.0 chip
//! as found on the GeeekPi TPM9670 module for Raspberry Pi.
//!
//! ## Hardware Configuration
//!
//! The GeeekPi TPM9670 connects via SPI to the Raspberry Pi GPIO header:
//!
//! | TPM Pin | RPi GPIO | Function    |
//! |---------|----------|-------------|
//! | SCLK    | GPIO 11  | SPI0_SCLK   |
//! | MOSI    | GPIO 10  | SPI0_MOSI   |
//! | MISO    | GPIO 9   | SPI0_MISO   |
//! | CS      | GPIO 8   | SPI0_CE0    |
//! | RST     | GPIO 24  | Reset (opt) |
//! | IRQ     | GPIO 25  | Interrupt   |
//!
//! ## SPI Protocol
//!
//! The SLB 9670 uses the TCG PC Client Platform TPM Profile (PTP)
//! SPI Hardware Interface Specification. Key characteristics:
//!
//! - SPI Mode 0 (CPOL=0, CPHA=0)
//! - Maximum 43 MHz SPI clock
//! - Big-endian register access
//! - Flow control via MISO wait states

use crate::{Sha256Digest, TpmRc, TpmResult, BootStage};

// ============================================================================
// SLB 9670 CONSTANTS
// ============================================================================

/// Infineon vendor ID
pub const SLB9670_VENDOR_ID: u16 = 0x15D1;

/// SLB 9670 device ID
pub const SLB9670_DEVICE_ID: u16 = 0x001B;

/// TPM TIS base address for SPI (locality 0)
pub const TIS_BASE_ADDR: u32 = 0xD40000;

/// Locality stride (4KB per locality)
pub const LOCALITY_STRIDE: u32 = 0x1000;

// TIS Register offsets (relative to locality base)
pub const TIS_ACCESS: u32 = 0x00;
pub const TIS_INT_ENABLE: u32 = 0x08;
pub const TIS_INT_VECTOR: u32 = 0x0C;
pub const TIS_INT_STATUS: u32 = 0x10;
pub const TIS_INTF_CAPABILITY: u32 = 0x14;
pub const TIS_STS: u32 = 0x18;
pub const TIS_BURST_COUNT: u32 = 0x19;
pub const TIS_DATA_FIFO: u32 = 0x24;
pub const TIS_XDATA_FIFO: u32 = 0x80;
pub const TIS_DID_VID: u32 = 0xF00;
pub const TIS_RID: u32 = 0xF04;

// TIS_ACCESS register bits
pub const ACCESS_ESTABLISHMENT: u8 = 1 << 0;
pub const ACCESS_REQUEST_USE: u8 = 1 << 1;
pub const ACCESS_PENDING_REQUEST: u8 = 1 << 2;
pub const ACCESS_SEIZE: u8 = 1 << 3;
pub const ACCESS_BEEN_SEIZED: u8 = 1 << 4;
pub const ACCESS_ACTIVE_LOCALITY: u8 = 1 << 5;
pub const ACCESS_VALID: u8 = 1 << 7;

// TIS_STS register bits
pub const STS_RESPONSE_RETRY: u8 = 1 << 1;
pub const STS_SELF_TEST_DONE: u8 = 1 << 2;
pub const STS_EXPECT: u8 = 1 << 3;
pub const STS_DATA_AVAIL: u8 = 1 << 4;
pub const STS_GO: u8 = 1 << 5;
pub const STS_COMMAND_READY: u8 = 1 << 6;
pub const STS_VALID: u8 = 1 << 7;

// TPM 2.0 Command Codes
pub const TPM2_CC_STARTUP: u32 = 0x00000144;
pub const TPM2_CC_SHUTDOWN: u32 = 0x00000145;
pub const TPM2_CC_SELF_TEST: u32 = 0x00000143;
pub const TPM2_CC_PCR_EXTEND: u32 = 0x00000182;
pub const TPM2_CC_PCR_READ: u32 = 0x0000017E;
pub const TPM2_CC_GET_RANDOM: u32 = 0x0000017B;
pub const TPM2_CC_QUOTE: u32 = 0x00000158;
pub const TPM2_CC_GET_CAPABILITY: u32 = 0x0000017A;

// TPM 2.0 Startup Types
pub const TPM2_SU_CLEAR: u16 = 0x0000;
pub const TPM2_SU_STATE: u16 = 0x0001;

// TPM 2.0 Algorithm IDs
pub const TPM2_ALG_SHA256: u16 = 0x000B;
pub const TPM2_ALG_NULL: u16 = 0x0010;

// TPM 2.0 Structure Tags
pub const TPM2_ST_NO_SESSIONS: u16 = 0x8001;
pub const TPM2_ST_SESSIONS: u16 = 0x8002;

// Maximum PCR index (TPM 2.0 supports 0-23)
pub const MAX_PCR_INDEX: u8 = 23;

// Number of PCRs
pub const PCR_COUNT: usize = 24;

// ============================================================================
// SPI COMMUNICATION
// ============================================================================

/// SPI transaction type for TPM TIS
#[derive(Debug, Clone, Copy)]
pub enum SpiTransaction {
    /// Read from TIS register
    Read { address: u32, len: usize },
    /// Write to TIS register
    Write { address: u32 },
}

/// SPI frame header for SLB 9670
#[derive(Debug, Clone, Copy)]
pub struct SpiHeader {
    /// Read (1) or Write (0)
    pub read: bool,
    /// Transfer size (0 = 1 byte, 1 = 2 bytes, etc.)
    pub size: u8,
    /// 24-bit address
    pub address: u32,
}

impl SpiHeader {
    /// Create a new SPI header
    pub fn new(read: bool, size: usize, address: u32) -> Self {
        Self {
            read,
            size: (size.saturating_sub(1) as u8) & 0x3F,
            address: address & 0xFFFFFF,
        }
    }

    /// Encode header to 4 bytes
    pub fn encode(&self) -> [u8; 4] {
        let byte0 = if self.read { 0x80 } else { 0x00 } | (self.size & 0x3F);
        [
            byte0,
            ((self.address >> 16) & 0xFF) as u8,
            ((self.address >> 8) & 0xFF) as u8,
            (self.address & 0xFF) as u8,
        ]
    }
}

// ============================================================================
// TPM DRIVER STATE
// ============================================================================

/// Current TPM state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpmState {
    /// TPM not initialized
    Uninitialized,
    /// TPM initialized and ready
    Ready,
    /// Command in progress
    CommandInProgress,
    /// Response available
    ResponseAvailable,
    /// Error state
    Error(TpmRc),
}

/// SLB 9670 TPM driver
pub struct Slb9670Tpm {
    /// Current locality (0-4)
    locality: u8,
    /// Driver state
    state: TpmState,
    /// SPI base address for memory-mapped I/O
    spi_base: usize,
    /// GPIO base address for chip select control
    gpio_base: usize,
    /// Command/response buffer
    buffer: [u8; 4096],
    /// Buffer position
    buffer_pos: usize,
}

impl Slb9670Tpm {
    /// Create a new TPM driver instance
    ///
    /// # Arguments
    /// * `spi_base` - Base address of SPI registers (0xFE204000 for BCM2711)
    /// * `gpio_base` - Base address of GPIO registers (0xFE200000 for BCM2711)
    pub const fn new(spi_base: usize, gpio_base: usize) -> Self {
        Self {
            locality: 0,
            state: TpmState::Uninitialized,
            spi_base,
            gpio_base,
            buffer: [0u8; 4096],
            buffer_pos: 0,
        }
    }

    /// Get current state
    pub fn state(&self) -> TpmState {
        self.state
    }

    /// Get current locality
    pub fn locality(&self) -> u8 {
        self.locality
    }

    /// Calculate TIS address for current locality
    fn tis_address(&self, offset: u32) -> u32 {
        TIS_BASE_ADDR + (self.locality as u32 * LOCALITY_STRIDE) + offset
    }

    // ========================================================================
    // LOW-LEVEL SPI OPERATIONS
    // ========================================================================

    /// Read a single byte from TIS register
    ///
    /// # Safety
    /// Caller must ensure spi_base points to valid SPI peripheral registers
    pub unsafe fn tis_read_byte(&self, offset: u32) -> u8 {
        let address = self.tis_address(offset);
        let header = SpiHeader::new(true, 1, address);

        // In a real implementation, this would:
        // 1. Assert CS (GPIO 8 low)
        // 2. Send header bytes
        // 3. Wait for MISO flow control
        // 4. Read response byte
        // 5. Deassert CS

        // Placeholder - actual SPI transaction
        self.spi_transfer_byte(header.encode(), 0x00)
    }

    /// Write a single byte to TIS register
    ///
    /// # Safety
    /// Caller must ensure spi_base points to valid SPI peripheral registers
    pub unsafe fn tis_write_byte(&self, offset: u32, value: u8) {
        let address = self.tis_address(offset);
        let header = SpiHeader::new(false, 1, address);

        // Placeholder - actual SPI transaction
        self.spi_transfer_byte(header.encode(), value);
    }

    /// Read multiple bytes from TIS FIFO
    ///
    /// # Safety
    /// Caller must ensure spi_base points to valid SPI peripheral registers
    pub unsafe fn tis_read_fifo(&self, buf: &mut [u8]) -> usize {
        let address = self.tis_address(TIS_DATA_FIFO);

        for (i, byte) in buf.iter_mut().enumerate() {
            let header = SpiHeader::new(true, 1, address);
            *byte = self.spi_transfer_byte(header.encode(), 0x00);
        }

        buf.len()
    }

    /// Write multiple bytes to TIS FIFO
    ///
    /// # Safety
    /// Caller must ensure spi_base points to valid SPI peripheral registers
    pub unsafe fn tis_write_fifo(&self, buf: &[u8]) -> usize {
        let address = self.tis_address(TIS_DATA_FIFO);

        for byte in buf {
            let header = SpiHeader::new(false, 1, address);
            self.spi_transfer_byte(header.encode(), *byte);
        }

        buf.len()
    }

    /// Low-level SPI byte transfer (placeholder)
    unsafe fn spi_transfer_byte(&self, _header: [u8; 4], data: u8) -> u8 {
        // This would be implemented using actual SPI hardware registers
        // For now, return placeholder
        data
    }

    // ========================================================================
    // TIS PROTOCOL OPERATIONS
    // ========================================================================

    /// Request access to a locality
    pub fn request_locality(&mut self, locality: u8) -> TpmResult<()> {
        if locality > 4 {
            return Err(TpmRc::BadParam);
        }

        self.locality = locality;

        unsafe {
            // Write REQUEST_USE to ACCESS register
            self.tis_write_byte(TIS_ACCESS, ACCESS_REQUEST_USE);

            // Poll until we have the locality
            for _ in 0..1000 {
                let access = self.tis_read_byte(TIS_ACCESS);
                if (access & ACCESS_ACTIVE_LOCALITY) != 0 {
                    return Ok(());
                }
                // Small delay would go here
            }
        }

        Err(TpmRc::Locality)
    }

    /// Release current locality
    pub fn release_locality(&mut self) -> TpmResult<()> {
        unsafe {
            self.tis_write_byte(TIS_ACCESS, ACCESS_ACTIVE_LOCALITY);
        }
        Ok(())
    }

    /// Wait for command ready state
    fn wait_command_ready(&self) -> TpmResult<()> {
        unsafe {
            // Request command ready
            self.tis_write_byte(TIS_STS, STS_COMMAND_READY);

            // Poll for ready
            for _ in 0..10000 {
                let sts = self.tis_read_byte(TIS_STS);
                if (sts & STS_COMMAND_READY) != 0 {
                    return Ok(());
                }
            }
        }
        Err(TpmRc::Retry)
    }

    /// Get burst count (how many bytes can be written at once)
    fn get_burst_count(&self) -> u16 {
        unsafe {
            let lo = self.tis_read_byte(TIS_BURST_COUNT) as u16;
            let hi = self.tis_read_byte(TIS_BURST_COUNT + 1) as u16;
            (hi << 8) | lo
        }
    }

    /// Wait for data available
    fn wait_data_available(&self) -> TpmResult<()> {
        unsafe {
            for _ in 0..100000 {
                let sts = self.tis_read_byte(TIS_STS);
                if (sts & STS_VALID) != 0 {
                    if (sts & STS_DATA_AVAIL) != 0 {
                        return Ok(());
                    }
                }
            }
        }
        Err(TpmRc::Retry)
    }

    // ========================================================================
    // TPM COMMAND INTERFACE
    // ========================================================================

    /// Send a command to the TPM and receive response
    pub fn execute_command(&mut self, cmd: &[u8]) -> TpmResult<usize> {
        if cmd.len() < 10 {
            return Err(TpmRc::BadParam);
        }

        // Ensure command ready
        self.wait_command_ready()?;

        unsafe {
            // Write command to FIFO
            let mut written = 0;
            while written < cmd.len() {
                let burst = self.get_burst_count() as usize;
                if burst == 0 {
                    continue;
                }
                let to_write = core::cmp::min(burst, cmd.len() - written);
                self.tis_write_fifo(&cmd[written..written + to_write]);
                written += to_write;
            }

            // Execute command
            self.tis_write_byte(TIS_STS, STS_GO);
        }

        self.state = TpmState::CommandInProgress;

        // Wait for response
        self.wait_data_available()?;

        // Read response
        self.buffer_pos = 0;
        unsafe {
            // Read header first (10 bytes minimum) into a local: the FIFO
            // helpers borrow &self, so they cannot write into self.buffer
            // directly.
            let mut header = [0u8; 10];
            self.tis_read_fifo(&mut header);
            self.buffer[0..10].copy_from_slice(&header);

            // Parse response size from header
            let size = u32::from_be_bytes([header[2], header[3], header[4], header[5]]) as usize;

            // The size field comes from the device — never trust it: a
            // value outside [10, buffer.len()] would otherwise put
            // buffer_pos out of bounds and panic in response().
            if size < 10 || size > self.buffer.len() {
                self.state = TpmState::Error(TpmRc::Failure);
                return Err(TpmRc::Failure);
            }

            if size > 10 {
                let mut body = self.buffer; // [u8; 4096] is Copy
                self.tis_read_fifo(&mut body[10..size]);
                self.buffer = body;
            }

            self.buffer_pos = size;
        }

        self.state = TpmState::ResponseAvailable;

        // Check response code
        let rc = u32::from_be_bytes([
            self.buffer[6],
            self.buffer[7],
            self.buffer[8],
            self.buffer[9],
        ]);

        if rc != 0 {
            return Err(TpmRc::from(rc));
        }

        Ok(self.buffer_pos)
    }

    /// Get response buffer
    pub fn response(&self) -> &[u8] {
        &self.buffer[..self.buffer_pos]
    }

    // ========================================================================
    // HIGH-LEVEL TPM COMMANDS
    // ========================================================================

    /// Initialize the TPM (startup clear)
    pub fn startup(&mut self) -> TpmResult<()> {
        self.request_locality(0)?;

        // Build TPM2_Startup command
        let cmd = crate::commands::build_startup(TPM2_SU_CLEAR);
        self.execute_command(&cmd)?;

        self.state = TpmState::Ready;
        Ok(())
    }

    /// Run TPM self-test
    pub fn self_test(&mut self, full_test: bool) -> TpmResult<()> {
        let cmd = crate::commands::build_self_test(full_test);
        self.execute_command(&cmd)?;
        Ok(())
    }

    /// Extend a PCR with a SHA-256 digest
    pub fn pcr_extend(&mut self, pcr_index: u8, digest: &Sha256Digest) -> TpmResult<()> {
        // build_pcr_extend validates the index (the previous local
        // builder emitted a truncated 51-byte command).
        let cmd = crate::commands::build_pcr_extend(pcr_index, digest)?;
        self.execute_command(&cmd)?;
        Ok(())
    }

    /// Read PCR values
    pub fn pcr_read(&mut self, pcr_selection: u32) -> TpmResult<[Sha256Digest; PCR_COUNT]> {
        let selection = crate::pcr::PcrSelection::from_bitmap(pcr_selection);
        let cmd = crate::commands::build_pcr_read(selection);
        self.execute_command(&cmd)?;

        // Parse the digests the TPM returned; unread PCRs stay zero.
        let result = crate::commands::parse_pcr_read(self.response())?;
        let mut pcrs = [Sha256Digest::zero(); PCR_COUNT];
        for &(index, digest) in result.values() {
            pcrs[index as usize] = digest;
        }

        Ok(pcrs)
    }

    /// Get random bytes from TPM
    pub fn get_random(&mut self, buf: &mut [u8]) -> TpmResult<usize> {
        if buf.len() > 32 {
            return Err(TpmRc::BadParam);
        }

        let cmd = crate::commands::build_get_random(buf.len() as u16);
        self.execute_command(&cmd)?;

        let random = crate::commands::parse_get_random(self.response())?;
        if random.len() < buf.len() {
            return Err(TpmRc::Failure);
        }
        buf.copy_from_slice(&random[..buf.len()]);
        Ok(buf.len())
    }

    /// Read vendor/device ID
    pub fn read_device_id(&self) -> (u16, u16) {
        unsafe {
            let did_vid = u32::from_le_bytes([
                self.tis_read_byte(TIS_DID_VID),
                self.tis_read_byte(TIS_DID_VID + 1),
                self.tis_read_byte(TIS_DID_VID + 2),
                self.tis_read_byte(TIS_DID_VID + 3),
            ]);

            let vendor_id = (did_vid & 0xFFFF) as u16;
            let device_id = ((did_vid >> 16) & 0xFFFF) as u16;

            (vendor_id, device_id)
        }
    }

    /// Verify this is an SLB 9670
    pub fn verify_device(&self) -> TpmResult<()> {
        let (vendor_id, device_id) = self.read_device_id();

        if vendor_id == SLB9670_VENDOR_ID && device_id == SLB9670_DEVICE_ID {
            Ok(())
        } else {
            Err(TpmRc::Failure)
        }
    }
}

// Command construction and response parsing live in `crate::commands`
// (transport-agnostic, shared with the generic `Tpm<T: TpmTransport>`
// layer). The private builders that used to sit here included a
// truncated 51-byte PCR_Extend command that panicked on use.
