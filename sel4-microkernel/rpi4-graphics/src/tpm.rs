//! # TPM 2.0 Driver (ST33KTPM2I3WBZA9)
//!
//! Driver for STMicroelectronics ST33K TPM 2.0 chip via SPI.
//!
//! ## Features
//! - Measured boot (extend PCRs)
//! - Remote attestation
//! - Secure key storage
//!
//! ## Hardware Connection (SPI)
//! - SCLK: GPIO 11 (SPI0_SCLK)
//! - MOSI: GPIO 10 (SPI0_MOSI)
//! - MISO: GPIO 9 (SPI0_MISO)
//! - CS:   GPIO 8 (SPI0_CE0)
//! - RST:  GPIO 24 (optional reset)
//!
//! ## Reference
//! - ST33KTPM2I3WBZA9 datasheet
//! - TCG TPM 2.0 Library Specification

use core::ptr::{read_volatile, write_volatile};

/// TPM TIS (TPM Interface Specification) register offsets
pub mod regs {
    pub const TPM_ACCESS: usize = 0x0000;
    pub const TPM_INT_ENABLE: usize = 0x0008;
    pub const TPM_INT_VECTOR: usize = 0x000C;
    pub const TPM_INT_STATUS: usize = 0x0010;
    pub const TPM_INTF_CAPS: usize = 0x0014;
    pub const TPM_STS: usize = 0x0018;
    pub const TPM_DATA_FIFO: usize = 0x0024;
    pub const TPM_DID_VID: usize = 0x0F00;
    pub const TPM_RID: usize = 0x0F04;
}

/// TPM status register bits
pub mod status {
    pub const STS_VALID: u8 = 0x80;
    pub const STS_COMMAND_READY: u8 = 0x40;
    pub const STS_GO: u8 = 0x20;
    pub const STS_DATA_AVAIL: u8 = 0x10;
    pub const STS_DATA_EXPECT: u8 = 0x08;
    pub const STS_SELF_TEST_DONE: u8 = 0x04;
    pub const STS_RESPONSE_RETRY: u8 = 0x02;
}

/// TPM access register bits
pub mod access {
    pub const ACCESS_VALID: u8 = 0x80;
    pub const ACCESS_ACTIVE_LOCALITY: u8 = 0x20;
    pub const ACCESS_REQUEST_PENDING: u8 = 0x04;
    pub const ACCESS_REQUEST_USE: u8 = 0x02;
    pub const ACCESS_ESTABLISHMENT: u8 = 0x01;
}

/// TPM 2.0 command codes
pub mod commands {
    pub const TPM2_CC_STARTUP: u32 = 0x0000_0144;
    pub const TPM2_CC_SHUTDOWN: u32 = 0x0000_0145;
    pub const TPM2_CC_SELF_TEST: u32 = 0x0000_0143;
    pub const TPM2_CC_PCR_EXTEND: u32 = 0x0000_0182;
    pub const TPM2_CC_PCR_READ: u32 = 0x0000_017E;
    pub const TPM2_CC_GET_RANDOM: u32 = 0x0000_017B;
    pub const TPM2_CC_QUOTE: u32 = 0x0000_0158;
    pub const TPM2_CC_CREATE_PRIMARY: u32 = 0x0000_0131;
}

/// TPM startup types
pub mod startup_type {
    pub const TPM2_SU_CLEAR: u16 = 0x0000;
    pub const TPM2_SU_STATE: u16 = 0x0001;
}

/// PCR bank indices
pub mod pcr {
    pub const PCR_FIRMWARE: usize = 0;      // Boot firmware
    pub const PCR_KERNEL: usize = 1;        // seL4 kernel
    pub const PCR_MICROKIT: usize = 2;      // Microkit system
    pub const PCR_PD_CONFIG: usize = 3;     // Protection Domain config
    pub const PCR_RUNTIME: usize = 4;       // Runtime measurements
}

/// TPM driver errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpmError {
    /// TPM not responding
    NotReady,
    /// Communication timeout
    Timeout,
    /// Invalid response from TPM
    InvalidResponse,
    /// Command failed with error code
    CommandFailed(u32),
    /// SPI communication error
    SpiError,
    /// TPM self-test failed
    SelfTestFailed,
}

/// SPI interface for TPM communication
pub trait SpiInterface {
    /// Transfer data via SPI (full duplex)
    fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> Result<(), TpmError>;
}

/// ST33K TPM 2.0 driver
pub struct Tpm<SPI: SpiInterface> {
    spi: SPI,
    locality: u8,
}

impl<SPI: SpiInterface> Tpm<SPI> {
    /// Create a new TPM driver instance
    pub fn new(spi: SPI) -> Self {
        Self { spi, locality: 0 }
    }

    /// Read a TIS register
    fn read_reg(&mut self, reg: usize) -> Result<u8, TpmError> {
        // SPI TIS command format: 1 byte command + 3 bytes address
        let addr = (self.locality as usize * 0x1000) + reg;
        let tx = [
            0x80 | ((addr >> 8) as u8 & 0x3F),  // Read command + addr high
            addr as u8,                          // addr low
            0x00,                                // dummy for read
        ];
        let mut rx = [0u8; 3];

        self.spi.transfer(&tx, &mut rx)?;
        Ok(rx[2])
    }

    /// Write a TIS register
    fn write_reg(&mut self, reg: usize, value: u8) -> Result<(), TpmError> {
        let addr = (self.locality as usize * 0x1000) + reg;
        let tx = [
            0x00 | ((addr >> 8) as u8 & 0x3F),  // Write command + addr high
            addr as u8,                          // addr low
            value,
        ];
        let mut rx = [0u8; 3];

        self.spi.transfer(&tx, &mut rx)
    }

    /// Request locality
    fn request_locality(&mut self) -> Result<(), TpmError> {
        self.write_reg(regs::TPM_ACCESS, access::ACCESS_REQUEST_USE)?;

        // Wait for locality to be granted
        for _ in 0..1000 {
            let access = self.read_reg(regs::TPM_ACCESS)?;
            if (access & access::ACCESS_ACTIVE_LOCALITY) != 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }

        Err(TpmError::Timeout)
    }

    /// Release locality
    fn release_locality(&mut self) -> Result<(), TpmError> {
        self.write_reg(regs::TPM_ACCESS, access::ACCESS_ACTIVE_LOCALITY)
    }

    /// Wait for TPM to be ready for command
    fn wait_for_ready(&mut self) -> Result<(), TpmError> {
        for _ in 0..10000 {
            let status = self.read_reg(regs::TPM_STS)?;
            if (status & status::STS_COMMAND_READY) != 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(TpmError::Timeout)
    }

    /// Initialize TPM (startup)
    pub fn init(&mut self) -> Result<(), TpmError> {
        // Request locality
        self.request_locality()?;

        // Send TPM2_Startup(CLEAR)
        self.startup(startup_type::TPM2_SU_CLEAR)?;

        // Run self-test
        self.self_test()?;

        Ok(())
    }

    /// TPM2_Startup command
    pub fn startup(&mut self, startup_type: u16) -> Result<(), TpmError> {
        let cmd = [
            0x80, 0x01,                         // TPM_ST_NO_SESSIONS
            0x00, 0x00, 0x00, 0x0C,             // Command size (12 bytes)
            0x00, 0x00, 0x01, 0x44,             // TPM2_CC_Startup
            (startup_type >> 8) as u8,          // Startup type high
            startup_type as u8,                 // Startup type low
        ];

        let mut response = [0u8; 10];
        self.send_command(&cmd, &mut response)?;

        // Check response code (offset 6-9)
        let rc = u32::from_be_bytes([response[6], response[7], response[8], response[9]]);
        if rc != 0 && rc != 0x100 {  // 0x100 = already started (OK)
            return Err(TpmError::CommandFailed(rc));
        }

        Ok(())
    }

    /// TPM2_SelfTest command
    pub fn self_test(&mut self) -> Result<(), TpmError> {
        let cmd = [
            0x80, 0x01,                         // TPM_ST_NO_SESSIONS
            0x00, 0x00, 0x00, 0x0B,             // Command size (11 bytes)
            0x00, 0x00, 0x01, 0x43,             // TPM2_CC_SelfTest
            0x01,                               // fullTest = YES
        ];

        let mut response = [0u8; 10];
        self.send_command(&cmd, &mut response)?;

        let rc = u32::from_be_bytes([response[6], response[7], response[8], response[9]]);
        if rc != 0 {
            return Err(TpmError::SelfTestFailed);
        }

        Ok(())
    }

    /// Extend a PCR with a measurement
    ///
    /// This is the core of measured boot - each component extends
    /// its hash into the TPM's PCR, creating a chain of trust.
    pub fn pcr_extend(&mut self, pcr_index: usize, digest: &[u8; 32]) -> Result<(), TpmError> {
        // Build TPM2_PCR_Extend command
        // This is simplified - full implementation needs proper marshaling
        let mut cmd = [0u8; 64];

        // Header
        cmd[0..2].copy_from_slice(&[0x80, 0x02]);  // TPM_ST_SESSIONS
        // Size will be filled later
        cmd[6..10].copy_from_slice(&0x0000_0182u32.to_be_bytes());  // TPM2_CC_PCR_Extend

        // PCR handle
        cmd[10..14].copy_from_slice(&(pcr_index as u32).to_be_bytes());

        // Authorization (simplified null auth)
        cmd[14..18].copy_from_slice(&0x0000_0009u32.to_be_bytes());  // auth size
        cmd[18..22].copy_from_slice(&0x4000_0009u32.to_be_bytes());  // TPM_RS_PW
        cmd[22..24].copy_from_slice(&[0x00, 0x00]);  // nonce size
        cmd[24] = 0x01;  // session attributes
        cmd[25..27].copy_from_slice(&[0x00, 0x00]);  // hmac size

        // Digest count and algorithm
        cmd[27..31].copy_from_slice(&0x0000_0001u32.to_be_bytes());  // count = 1
        cmd[31..33].copy_from_slice(&0x000Bu16.to_be_bytes());  // TPM_ALG_SHA256

        // Digest
        cmd[33..65].copy_from_slice(digest);

        // Fill size
        let size = 65u32;
        cmd[2..6].copy_from_slice(&size.to_be_bytes());

        let mut response = [0u8; 20];
        self.send_command(&cmd[..size as usize], &mut response)?;

        let rc = u32::from_be_bytes([response[6], response[7], response[8], response[9]]);
        if rc != 0 {
            return Err(TpmError::CommandFailed(rc));
        }

        Ok(())
    }

    /// Get random bytes from TPM
    pub fn get_random(&mut self, output: &mut [u8]) -> Result<(), TpmError> {
        if output.len() > 32 {
            return Err(TpmError::InvalidResponse);
        }

        let mut cmd = [0u8; 14];
        cmd[0..2].copy_from_slice(&[0x80, 0x01]);  // TPM_ST_NO_SESSIONS
        cmd[2..6].copy_from_slice(&14u32.to_be_bytes());  // size
        cmd[6..10].copy_from_slice(&0x0000_017Bu32.to_be_bytes());  // TPM2_CC_GetRandom
        cmd[10..12].copy_from_slice(&(output.len() as u16).to_be_bytes());

        let mut response = [0u8; 64];
        self.send_command(&cmd, &mut response)?;

        let rc = u32::from_be_bytes([response[6], response[7], response[8], response[9]]);
        if rc != 0 {
            return Err(TpmError::CommandFailed(rc));
        }

        // Copy random bytes from response
        let random_size = u16::from_be_bytes([response[10], response[11]]) as usize;
        let copy_len = random_size.min(output.len());
        output[..copy_len].copy_from_slice(&response[12..12 + copy_len]);

        Ok(())
    }

    /// Send a command to the TPM and receive response
    fn send_command(&mut self, cmd: &[u8], response: &mut [u8]) -> Result<(), TpmError> {
        // Wait for TPM ready
        self.wait_for_ready()?;

        // Write command to FIFO
        for byte in cmd {
            self.write_reg(regs::TPM_DATA_FIFO, *byte)?;
        }

        // Execute command
        self.write_reg(regs::TPM_STS, status::STS_GO)?;

        // Wait for data available
        for _ in 0..100000 {
            let sts = self.read_reg(regs::TPM_STS)?;
            if (sts & status::STS_DATA_AVAIL) != 0 {
                break;
            }
            if (sts & status::STS_VALID) != 0 && (sts & status::STS_COMMAND_READY) != 0 {
                return Err(TpmError::InvalidResponse);
            }
            core::hint::spin_loop();
        }

        // Read response from FIFO
        for byte in response.iter_mut() {
            *byte = self.read_reg(regs::TPM_DATA_FIFO)?;
        }

        // Signal command complete
        self.write_reg(regs::TPM_STS, status::STS_COMMAND_READY)?;

        Ok(())
    }
}

/// Compute SHA-256 hash of data (for PCR extension)
/// Note: This is a placeholder - real implementation needs a SHA-256 library
pub fn sha256(data: &[u8]) -> [u8; 32] {
    // TODO: Implement or use a verified SHA-256 implementation
    // For now, return a simple hash (NOT SECURE - placeholder only)
    let mut hash = [0u8; 32];
    for (i, byte) in data.iter().enumerate() {
        hash[i % 32] ^= byte;
    }
    hash
}

/// Measured boot: extend PCR with component measurement
pub fn measure_component<SPI: SpiInterface>(
    tpm: &mut Tpm<SPI>,
    pcr: usize,
    _component_name: &str,
    component_data: &[u8],
) -> Result<(), TpmError> {
    let digest = sha256(component_data);

    // Log measurement (in debug builds)
    #[cfg(debug_assertions)]
    {
        // Would use debug_println! here
    }

    tpm.pcr_extend(pcr, &digest)
}
