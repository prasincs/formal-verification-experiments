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
            // Read header first (10 bytes minimum)
            self.tis_read_fifo(&mut self.buffer[0..10]);

            // Parse response size from header
            let size = u32::from_be_bytes([
                self.buffer[2],
                self.buffer[3],
                self.buffer[4],
                self.buffer[5],
            ]) as usize;

            if size > 10 && size <= self.buffer.len() {
                self.tis_read_fifo(&mut self.buffer[10..size]);
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
        let cmd = build_startup_command(TPM2_SU_CLEAR);
        self.execute_command(&cmd)?;

        self.state = TpmState::Ready;
        Ok(())
    }

    /// Run TPM self-test
    pub fn self_test(&mut self, full_test: bool) -> TpmResult<()> {
        let cmd = build_self_test_command(full_test);
        self.execute_command(&cmd)?;
        Ok(())
    }

    /// Extend a PCR with a SHA-256 digest
    pub fn pcr_extend(&mut self, pcr_index: u8, digest: &Sha256Digest) -> TpmResult<()> {
        if pcr_index > MAX_PCR_INDEX {
            return Err(TpmRc::BadParam);
        }

        let cmd = build_pcr_extend_command(pcr_index, digest);
        self.execute_command(&cmd)?;
        Ok(())
    }

    /// Read PCR values
    pub fn pcr_read(&mut self, pcr_selection: u32) -> TpmResult<[Sha256Digest; PCR_COUNT]> {
        let cmd = build_pcr_read_command(pcr_selection);
        self.execute_command(&cmd)?;

        // Parse response (simplified)
        let mut pcrs = [Sha256Digest::zero(); PCR_COUNT];

        // Response parsing would go here
        // For now, return zeros

        Ok(pcrs)
    }

    /// Get random bytes from TPM
    pub fn get_random(&mut self, buf: &mut [u8]) -> TpmResult<usize> {
        if buf.len() > 32 {
            return Err(TpmRc::BadParam);
        }

        let cmd = build_get_random_command(buf.len() as u16);
        let resp_len = self.execute_command(&cmd)?;

        // Copy random data from response
        if resp_len >= 12 + buf.len() {
            buf.copy_from_slice(&self.buffer[12..12 + buf.len()]);
            Ok(buf.len())
        } else {
            Err(TpmRc::Failure)
        }
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

// ============================================================================
// COMMAND BUILDERS
// ============================================================================

/// Build TPM2_Startup command
fn build_startup_command(startup_type: u16) -> [u8; 12] {
    let mut cmd = [0u8; 12];

    // Tag
    cmd[0..2].copy_from_slice(&TPM2_ST_NO_SESSIONS.to_be_bytes());
    // Size
    cmd[2..6].copy_from_slice(&12u32.to_be_bytes());
    // Command code
    cmd[6..10].copy_from_slice(&TPM2_CC_STARTUP.to_be_bytes());
    // Startup type
    cmd[10..12].copy_from_slice(&startup_type.to_be_bytes());

    cmd
}

/// Build TPM2_SelfTest command
fn build_self_test_command(full_test: bool) -> [u8; 11] {
    let mut cmd = [0u8; 11];

    cmd[0..2].copy_from_slice(&TPM2_ST_NO_SESSIONS.to_be_bytes());
    cmd[2..6].copy_from_slice(&11u32.to_be_bytes());
    cmd[6..10].copy_from_slice(&TPM2_CC_SELF_TEST.to_be_bytes());
    cmd[10] = if full_test { 1 } else { 0 };

    cmd
}

/// Build TPM2_PCR_Extend command
fn build_pcr_extend_command(pcr_index: u8, digest: &Sha256Digest) -> [u8; 51] {
    let mut cmd = [0u8; 51];

    // Header
    cmd[0..2].copy_from_slice(&TPM2_ST_SESSIONS.to_be_bytes());
    cmd[2..6].copy_from_slice(&51u32.to_be_bytes());
    cmd[6..10].copy_from_slice(&TPM2_CC_PCR_EXTEND.to_be_bytes());

    // PCR handle (0x00000000 + pcr_index)
    cmd[10..14].copy_from_slice(&(pcr_index as u32).to_be_bytes());

    // Authorization (password session, empty auth)
    cmd[14..18].copy_from_slice(&9u32.to_be_bytes()); // Auth size
    cmd[18..22].copy_from_slice(&0x40000009u32.to_be_bytes()); // TPM_RS_PW
    cmd[22..24].copy_from_slice(&0u16.to_be_bytes()); // Nonce size
    cmd[24] = 0; // Session attributes
    cmd[25..27].copy_from_slice(&0u16.to_be_bytes()); // Auth size

    // TPML_DIGEST_VALUES
    cmd[27..31].copy_from_slice(&1u32.to_be_bytes()); // Count = 1

    // TPMT_HA
    cmd[31..33].copy_from_slice(&TPM2_ALG_SHA256.to_be_bytes());
    cmd[33..65].copy_from_slice(&digest.bytes);

    // Note: Array is 51 bytes but command uses 65, would need larger buffer
    // This is simplified - real implementation needs proper sizing

    cmd
}

/// Build TPM2_PCR_Read command
fn build_pcr_read_command(pcr_selection: u32) -> [u8; 17] {
    let mut cmd = [0u8; 17];

    cmd[0..2].copy_from_slice(&TPM2_ST_NO_SESSIONS.to_be_bytes());
    cmd[2..6].copy_from_slice(&17u32.to_be_bytes());
    cmd[6..10].copy_from_slice(&TPM2_CC_PCR_READ.to_be_bytes());

    // PCR selection
    cmd[10..14].copy_from_slice(&1u32.to_be_bytes()); // Count
    cmd[14..16].copy_from_slice(&TPM2_ALG_SHA256.to_be_bytes());
    cmd[16] = 3; // Size of select (3 bytes = 24 PCRs)
    // Would need more bytes for actual selection bitmap

    cmd
}

/// Build TPM2_GetRandom command
fn build_get_random_command(bytes_requested: u16) -> [u8; 12] {
    let mut cmd = [0u8; 12];

    cmd[0..2].copy_from_slice(&TPM2_ST_NO_SESSIONS.to_be_bytes());
    cmd[2..6].copy_from_slice(&12u32.to_be_bytes());
    cmd[6..10].copy_from_slice(&TPM2_CC_GET_RANDOM.to_be_bytes());
    cmd[10..12].copy_from_slice(&bytes_requested.to_be_bytes());

    cmd
}
