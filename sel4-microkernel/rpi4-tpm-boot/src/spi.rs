//! # SPI Driver for Raspberry Pi 4 TPM Communication
//!
//! Low-level SPI interface for communicating with TPM 2.0 modules
//! on the Raspberry Pi 4 (BCM2711).
//!
//! ## Hardware Configuration
//!
//! The Raspberry Pi 4 has multiple SPI controllers. For TPM communication,
//! we use SPI0 with the following configuration:
//!
//! | Function | BCM GPIO | Physical Pin |
//! |----------|----------|--------------|
//! | SCLK     | GPIO 11  | Pin 23       |
//! | MOSI     | GPIO 10  | Pin 19       |
//! | MISO     | GPIO 9   | Pin 21       |
//! | CE0      | GPIO 8   | Pin 24       |
//! | CE1      | GPIO 7   | Pin 26       |
//!
//! ## SPI Mode for SLB 9670
//!
//! The Infineon SLB 9670 uses:
//! - SPI Mode 0 (CPOL=0, CPHA=0)
//! - MSB first
//! - Maximum 43 MHz clock (we use conservative 10 MHz)
//! - CS active low

use crate::{TpmResult, TpmRc};

// ============================================================================
// BCM2711 SPI REGISTERS
// ============================================================================

/// SPI0 base address for BCM2711 (Raspberry Pi 4)
pub const SPI0_BASE: usize = 0xFE204000;

/// SPI register offsets
pub mod spi_reg {
    /// Control and Status
    pub const CS: usize = 0x00;
    /// TX/RX FIFO
    pub const FIFO: usize = 0x04;
    /// Clock divider
    pub const CLK: usize = 0x08;
    /// Data length
    pub const DLEN: usize = 0x0C;
    /// LOSSI mode TOH
    pub const LTOH: usize = 0x10;
    /// DMA DREQ controls
    pub const DC: usize = 0x14;
}

/// CS register bits
pub mod cs_bits {
    /// Chip select (2 bits)
    pub const CS_MASK: u32 = 0x03;
    /// Clock phase
    pub const CPHA: u32 = 1 << 2;
    /// Clock polarity
    pub const CPOL: u32 = 1 << 3;
    /// Clear TX FIFO
    pub const CLEAR_TX: u32 = 1 << 4;
    /// Clear RX FIFO
    pub const CLEAR_RX: u32 = 1 << 5;
    /// Chip select polarity
    pub const CSPOL: u32 = 1 << 6;
    /// Transfer active
    pub const TA: u32 = 1 << 7;
    /// DMAEN
    pub const DMAEN: u32 = 1 << 8;
    /// Interrupt on done
    pub const INTD: u32 = 1 << 9;
    /// Interrupt on RXR
    pub const INTR: u32 = 1 << 10;
    /// Auto deassert CS
    pub const ADCS: u32 = 1 << 11;
    /// Read enable
    pub const REN: u32 = 1 << 12;
    /// LOSSI enable
    pub const LEN: u32 = 1 << 13;
    /// Transfer done
    pub const DONE: u32 = 1 << 16;
    /// RX FIFO contains data
    pub const RXD: u32 = 1 << 17;
    /// TX FIFO can accept data
    pub const TXD: u32 = 1 << 18;
    /// RX FIFO needs reading
    pub const RXR: u32 = 1 << 19;
    /// RX FIFO full
    pub const RXF: u32 = 1 << 20;
    /// CS0 polarity
    pub const CSPOL0: u32 = 1 << 21;
    /// CS1 polarity
    pub const CSPOL1: u32 = 1 << 22;
    /// CS2 polarity
    pub const CSPOL2: u32 = 1 << 23;
    /// DMA LEN mode
    pub const DMA_LEN: u32 = 1 << 24;
    /// Long data word
    pub const LEN_LONG: u32 = 1 << 25;
}

// ============================================================================
// GPIO CONFIGURATION
// ============================================================================

/// GPIO base address for BCM2711
pub const GPIO_BASE: usize = 0xFE200000;

/// GPIO function select registers
pub mod gpio_reg {
    pub const GPFSEL0: usize = 0x00;
    pub const GPFSEL1: usize = 0x04;
    pub const GPFSEL2: usize = 0x08;
    pub const GPSET0: usize = 0x1C;
    pub const GPCLR0: usize = 0x28;
    pub const GPLEV0: usize = 0x34;
}

/// GPIO function codes
pub mod gpio_func {
    pub const INPUT: u32 = 0b000;
    pub const OUTPUT: u32 = 0b001;
    pub const ALT0: u32 = 0b100;  // SPI0
    pub const ALT1: u32 = 0b101;
    pub const ALT2: u32 = 0b110;
    pub const ALT3: u32 = 0b111;
    pub const ALT4: u32 = 0b011;
    pub const ALT5: u32 = 0b010;
}

// ============================================================================
// SPI CONFIGURATION
// ============================================================================

/// SPI clock speed options
#[derive(Clone, Copy, Debug)]
pub enum SpiSpeed {
    /// 1 MHz (safest)
    Slow,
    /// 10 MHz (recommended for TPM)
    Medium,
    /// 25 MHz (fast)
    Fast,
    /// 43 MHz (maximum for SLB9670)
    Maximum,
}

impl SpiSpeed {
    /// Get clock divider for 500 MHz core clock
    pub fn divider(&self) -> u32 {
        match self {
            SpiSpeed::Slow => 500,    // 1 MHz
            SpiSpeed::Medium => 50,   // 10 MHz
            SpiSpeed::Fast => 20,     // 25 MHz
            SpiSpeed::Maximum => 12,  // ~42 MHz
        }
    }
}

/// SPI chip select
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipSelect {
    /// CE0 (GPIO 8)
    Cs0 = 0,
    /// CE1 (GPIO 7)
    Cs1 = 1,
}

/// SPI mode (CPOL, CPHA)
#[derive(Clone, Copy, Debug)]
pub enum SpiMode {
    /// Mode 0: CPOL=0, CPHA=0
    Mode0,
    /// Mode 1: CPOL=0, CPHA=1
    Mode1,
    /// Mode 2: CPOL=1, CPHA=0
    Mode2,
    /// Mode 3: CPOL=1, CPHA=1
    Mode3,
}

// ============================================================================
// SPI DRIVER
// ============================================================================

/// BCM2711 SPI driver for TPM communication
pub struct Spi {
    /// SPI register base (virtual address after mapping)
    spi_base: usize,
    /// GPIO register base (virtual address after mapping)
    gpio_base: usize,
    /// Selected chip select
    chip_select: ChipSelect,
    /// Configured speed
    speed: SpiSpeed,
    /// Initialized flag
    initialized: bool,
}

impl Spi {
    /// Create a new SPI driver instance
    ///
    /// # Arguments
    /// * `spi_base` - Virtual address of SPI registers
    /// * `gpio_base` - Virtual address of GPIO registers
    /// * `chip_select` - Which chip select to use
    pub const fn new(spi_base: usize, gpio_base: usize, chip_select: ChipSelect) -> Self {
        Self {
            spi_base,
            gpio_base,
            chip_select,
            speed: SpiSpeed::Medium,
            initialized: false,
        }
    }

    /// Initialize SPI hardware
    ///
    /// This configures GPIO pins and SPI controller for TPM communication.
    ///
    /// # Safety
    /// Caller must ensure spi_base and gpio_base point to valid mapped registers.
    pub unsafe fn init(&mut self) -> TpmResult<()> {
        // Configure GPIO pins for SPI0 ALT0 function
        self.configure_gpio()?;

        // Configure SPI controller
        self.configure_spi()?;

        self.initialized = true;
        Ok(())
    }

    /// Configure GPIO pins for SPI
    unsafe fn configure_gpio(&self) -> TpmResult<()> {
        let gpfsel0 = (self.gpio_base + gpio_reg::GPFSEL0) as *mut u32;
        let gpfsel1 = (self.gpio_base + gpio_reg::GPFSEL1) as *mut u32;

        // GPIO 7-11 are in GPFSEL0 (bits 21-29) and GPFSEL1 (bits 0-5)
        // GPIO 7 (CE1): GPFSEL0[23:21] = ALT0
        // GPIO 8 (CE0): GPFSEL0[26:24] = ALT0
        // GPIO 9 (MISO): GPFSEL0[29:27] = ALT0
        // GPIO 10 (MOSI): GPFSEL1[2:0] = ALT0
        // GPIO 11 (SCLK): GPFSEL1[5:3] = ALT0

        let mut fsel0 = core::ptr::read_volatile(gpfsel0);
        let mut fsel1 = core::ptr::read_volatile(gpfsel1);

        // Clear and set GPIO 7, 8, 9 to ALT0
        fsel0 &= !(0x1FF << 21); // Clear bits 21-29
        fsel0 |= (gpio_func::ALT0 << 21) | (gpio_func::ALT0 << 24) | (gpio_func::ALT0 << 27);

        // Clear and set GPIO 10, 11 to ALT0
        fsel1 &= !(0x3F << 0); // Clear bits 0-5
        fsel1 |= (gpio_func::ALT0 << 0) | (gpio_func::ALT0 << 3);

        core::ptr::write_volatile(gpfsel0, fsel0);
        core::ptr::write_volatile(gpfsel1, fsel1);

        Ok(())
    }

    /// Configure SPI controller
    unsafe fn configure_spi(&mut self) -> TpmResult<()> {
        let cs_reg = (self.spi_base + spi_reg::CS) as *mut u32;
        let clk_reg = (self.spi_base + spi_reg::CLK) as *mut u32;

        // Clear FIFOs
        core::ptr::write_volatile(cs_reg, cs_bits::CLEAR_TX | cs_bits::CLEAR_RX);

        // Set clock divider
        core::ptr::write_volatile(clk_reg, self.speed.divider());

        // Configure CS register:
        // - SPI Mode 0 (CPOL=0, CPHA=0)
        // - CS active low (default)
        // - Select chip
        let cs_val = self.chip_select as u32;
        core::ptr::write_volatile(cs_reg, cs_val);

        Ok(())
    }

    /// Set SPI speed
    pub fn set_speed(&mut self, speed: SpiSpeed) {
        self.speed = speed;
        if self.initialized {
            unsafe {
                let clk_reg = (self.spi_base + spi_reg::CLK) as *mut u32;
                core::ptr::write_volatile(clk_reg, speed.divider());
            }
        }
    }

    /// Transfer a single byte (full duplex)
    ///
    /// Sends tx_byte and returns received byte.
    ///
    /// # Safety
    /// SPI must be initialized.
    pub unsafe fn transfer_byte(&self, tx_byte: u8) -> u8 {
        let cs_reg = (self.spi_base + spi_reg::CS) as *mut u32;
        let fifo_reg = (self.spi_base + spi_reg::FIFO) as *mut u32;

        // Start transfer
        let cs_val = core::ptr::read_volatile(cs_reg);
        core::ptr::write_volatile(cs_reg, cs_val | cs_bits::TA);

        // Wait for TX FIFO ready
        while (core::ptr::read_volatile(cs_reg) & cs_bits::TXD) == 0 {}

        // Write byte
        core::ptr::write_volatile(fifo_reg, tx_byte as u32);

        // Wait for transfer done
        while (core::ptr::read_volatile(cs_reg) & cs_bits::DONE) == 0 {}

        // Read received byte
        let rx_byte = core::ptr::read_volatile(fifo_reg) as u8;

        // End transfer
        core::ptr::write_volatile(cs_reg, cs_val & !cs_bits::TA);

        rx_byte
    }

    /// Transfer multiple bytes
    ///
    /// # Arguments
    /// * `tx_buf` - Bytes to transmit
    /// * `rx_buf` - Buffer for received bytes (must be same length as tx_buf)
    ///
    /// # Safety
    /// SPI must be initialized.
    pub unsafe fn transfer(&self, tx_buf: &[u8], rx_buf: &mut [u8]) -> TpmResult<()> {
        if tx_buf.len() != rx_buf.len() {
            return Err(TpmRc::BadParam);
        }

        let cs_reg = (self.spi_base + spi_reg::CS) as *mut u32;
        let fifo_reg = (self.spi_base + spi_reg::FIFO) as *mut u32;

        // Clear FIFOs
        let cs_val = core::ptr::read_volatile(cs_reg);
        core::ptr::write_volatile(cs_reg, cs_val | cs_bits::CLEAR_TX | cs_bits::CLEAR_RX);

        // Start transfer
        core::ptr::write_volatile(cs_reg, cs_val | cs_bits::TA);

        let mut tx_idx = 0;
        let mut rx_idx = 0;

        while rx_idx < rx_buf.len() {
            // Fill TX FIFO while we can and have data
            while tx_idx < tx_buf.len()
                && (core::ptr::read_volatile(cs_reg) & cs_bits::TXD) != 0
            {
                core::ptr::write_volatile(fifo_reg, tx_buf[tx_idx] as u32);
                tx_idx += 1;
            }

            // Read RX FIFO while data available
            while rx_idx < rx_buf.len()
                && (core::ptr::read_volatile(cs_reg) & cs_bits::RXD) != 0
            {
                rx_buf[rx_idx] = core::ptr::read_volatile(fifo_reg) as u8;
                rx_idx += 1;
            }
        }

        // Wait for done
        while (core::ptr::read_volatile(cs_reg) & cs_bits::DONE) == 0 {}

        // End transfer
        core::ptr::write_volatile(cs_reg, cs_val & !cs_bits::TA);

        Ok(())
    }

    /// Write bytes (ignore received data)
    ///
    /// # Safety
    /// SPI must be initialized.
    pub unsafe fn write(&self, buf: &[u8]) -> TpmResult<()> {
        let cs_reg = (self.spi_base + spi_reg::CS) as *mut u32;
        let fifo_reg = (self.spi_base + spi_reg::FIFO) as *mut u32;

        // Start transfer
        let cs_val = core::ptr::read_volatile(cs_reg);
        core::ptr::write_volatile(cs_reg, cs_val | cs_bits::TA | cs_bits::CLEAR_RX);

        for &byte in buf {
            // Wait for TX ready
            while (core::ptr::read_volatile(cs_reg) & cs_bits::TXD) == 0 {}
            core::ptr::write_volatile(fifo_reg, byte as u32);
        }

        // Wait for done
        while (core::ptr::read_volatile(cs_reg) & cs_bits::DONE) == 0 {}

        // End transfer
        core::ptr::write_volatile(cs_reg, cs_val & !cs_bits::TA);

        Ok(())
    }

    /// Read bytes (send zeros)
    ///
    /// # Safety
    /// SPI must be initialized.
    pub unsafe fn read(&self, buf: &mut [u8]) -> TpmResult<()> {
        let cs_reg = (self.spi_base + spi_reg::CS) as *mut u32;
        let fifo_reg = (self.spi_base + spi_reg::FIFO) as *mut u32;

        // Start transfer
        let cs_val = core::ptr::read_volatile(cs_reg);
        core::ptr::write_volatile(cs_reg, cs_val | cs_bits::TA | cs_bits::CLEAR_TX);

        for byte in buf.iter_mut() {
            // Send dummy byte
            while (core::ptr::read_volatile(cs_reg) & cs_bits::TXD) == 0 {}
            core::ptr::write_volatile(fifo_reg, 0x00);

            // Wait for RX
            while (core::ptr::read_volatile(cs_reg) & cs_bits::RXD) == 0 {}
            *byte = core::ptr::read_volatile(fifo_reg) as u8;
        }

        // Wait for done
        while (core::ptr::read_volatile(cs_reg) & cs_bits::DONE) == 0 {}

        // End transfer
        core::ptr::write_volatile(cs_reg, cs_val & !cs_bits::TA);

        Ok(())
    }
}

// ============================================================================
// TPM SPI PROTOCOL
// ============================================================================

/// TPM-specific SPI operations
pub struct TpmSpi {
    /// Underlying SPI driver
    spi: Spi,
}

impl TpmSpi {
    /// Create TPM SPI interface
    pub const fn new(spi: Spi) -> Self {
        Self { spi }
    }

    /// Initialize TPM SPI
    ///
    /// # Safety
    /// See Spi::init
    pub unsafe fn init(&mut self) -> TpmResult<()> {
        self.spi.init()
    }

    /// Read from TPM TIS register
    ///
    /// TPM SPI protocol:
    /// 1. Send 4-byte header (read/write, size, 24-bit address)
    /// 2. Wait for flow control (MISO goes high)
    /// 3. Transfer data
    ///
    /// # Safety
    /// SPI must be initialized.
    pub unsafe fn tis_read(&self, address: u32, buf: &mut [u8]) -> TpmResult<()> {
        if buf.is_empty() || buf.len() > 64 {
            return Err(TpmRc::BadParam);
        }

        // Build header: read bit (0x80) | size | address
        let header = [
            0x80 | ((buf.len() - 1) as u8 & 0x3F),
            ((address >> 16) & 0xFF) as u8,
            ((address >> 8) & 0xFF) as u8,
            (address & 0xFF) as u8,
        ];

        // Send header
        let mut rx_header = [0u8; 4];
        self.spi.transfer(&header, &mut rx_header)?;

        // Wait for flow control (last byte of header response should have bit 0 set)
        // In practice, may need to poll with dummy bytes

        // Read data
        self.spi.read(buf)?;

        Ok(())
    }

    /// Write to TPM TIS register
    ///
    /// # Safety
    /// SPI must be initialized.
    pub unsafe fn tis_write(&self, address: u32, buf: &[u8]) -> TpmResult<()> {
        if buf.is_empty() || buf.len() > 64 {
            return Err(TpmRc::BadParam);
        }

        // Build header: write bit (0x00) | size | address
        let header = [
            (buf.len() - 1) as u8 & 0x3F,
            ((address >> 16) & 0xFF) as u8,
            ((address >> 8) & 0xFF) as u8,
            (address & 0xFF) as u8,
        ];

        // Send header
        self.spi.write(&header)?;

        // Write data
        self.spi.write(buf)?;

        Ok(())
    }
}
