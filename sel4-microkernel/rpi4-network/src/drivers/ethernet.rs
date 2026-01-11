//! BCM54213PE Gigabit Ethernet Driver for Raspberry Pi 4
//!
//! This driver supports the native Gigabit Ethernet interface on the RPi4,
//! which consists of:
//! - GENET (Gigabit Ethernet) controller on the SoC
//! - BCM54213PE external PHY transceiver
//!
//! # Hardware Details
//!
//! - Base address: 0xfd580000
//! - Size: 64KB (0x10000)
//! - IRQ: Directly mapped via Microkit system description
//! - PHY ID: 0x600d84a2
//!
//! # References
//!
//! - Linux driver: drivers/net/ethernet/broadcom/genet/
//! - Circle bare metal: https://github.com/rsta2/circle/blob/master/lib/bcm54213.cpp

use super::{DriverError, DriverStats, LinkSpeed, LinkStatus, MacAddress, NetworkDriver};

/// GENET controller base address
const GENET_BASE: usize = 0xfd580000;

/// GENET register offsets
#[allow(dead_code)]
mod regs {
    // System control
    pub const SYS_REV_CTRL: usize = 0x00;
    pub const SYS_PORT_CTRL: usize = 0x04;

    // UniMAC registers (offset 0x800)
    pub const UMAC_BASE: usize = 0x800;
    pub const UMAC_CMD: usize = UMAC_BASE + 0x008;
    pub const UMAC_MAC0: usize = UMAC_BASE + 0x00c;
    pub const UMAC_MAC1: usize = UMAC_BASE + 0x010;
    pub const UMAC_MAX_FRAME_LEN: usize = UMAC_BASE + 0x014;
    pub const UMAC_TX_FLUSH: usize = UMAC_BASE + 0x334;

    // MDIO registers (offset 0xe00)
    pub const MDIO_BASE: usize = 0xe00;
    pub const MDIO_CMD: usize = MDIO_BASE + 0x00;

    // TX DMA (offset 0x4000)
    pub const TX_DMA_BASE: usize = 0x4000;

    // RX DMA (offset 0x2000)
    pub const RX_DMA_BASE: usize = 0x2000;
}

/// GENET version
#[derive(Debug, Clone, Copy)]
enum GenetVersion {
    V1,
    V2,
    V3,
    V4,
    V5, // RPi4 uses this
}

/// PHY constants
mod phy {
    /// BCM54213PE PHY ID
    pub const BCM54213PE_PHY_ID: u32 = 0x600d84a2;

    /// PHY address on MDIO bus
    pub const PHY_ADDR: u8 = 1;

    // Standard MII registers
    pub const MII_BMCR: u8 = 0x00; // Basic Mode Control
    pub const MII_BMSR: u8 = 0x01; // Basic Mode Status
    pub const MII_PHYSID1: u8 = 0x02; // PHY ID 1
    pub const MII_PHYSID2: u8 = 0x03; // PHY ID 2
    pub const MII_ADVERTISE: u8 = 0x04; // Auto-negotiation Advertisement
    pub const MII_LPA: u8 = 0x05; // Link Partner Ability
    pub const MII_CTRL1000: u8 = 0x09; // 1000BASE-T Control
    pub const MII_STAT1000: u8 = 0x0a; // 1000BASE-T Status

    // BMCR bits
    pub const BMCR_RESET: u16 = 0x8000;
    pub const BMCR_ANENABLE: u16 = 0x1000;
    pub const BMCR_ANRESTART: u16 = 0x0200;

    // BMSR bits
    pub const BMSR_LSTATUS: u16 = 0x0004;
    pub const BMSR_ANEGCOMPLETE: u16 = 0x0020;
}

/// Ethernet driver state
pub struct EthernetDriver {
    /// Base address for MMIO access
    base: usize,
    /// GENET version detected
    version: GenetVersion,
    /// MAC address
    mac: MacAddress,
    /// Current link status
    link: LinkStatus,
    /// Statistics
    stats: DriverStats,
    /// TX ring buffer index
    tx_index: usize,
    /// RX ring buffer index
    rx_index: usize,
}

impl EthernetDriver {
    /// Create a new Ethernet driver instance
    ///
    /// # Safety
    ///
    /// The base address must be mapped by the Microkit system description
    /// and accessible from this protection domain.
    fn new(base: usize) -> Self {
        Self {
            base,
            version: GenetVersion::V5,
            mac: MacAddress::new([0; 6]),
            link: LinkStatus::down(),
            stats: DriverStats::default(),
            tx_index: 0,
            rx_index: 0,
        }
    }

    /// Read a 32-bit register
    #[inline]
    fn read_reg(&self, offset: usize) -> u32 {
        let addr = (self.base + offset) as *const u32;
        // Safety: Address is within mapped MMIO region
        unsafe { core::ptr::read_volatile(addr) }
    }

    /// Write a 32-bit register
    #[inline]
    fn write_reg(&self, offset: usize, value: u32) {
        let addr = (self.base + offset) as *mut u32;
        // Safety: Address is within mapped MMIO region
        unsafe { core::ptr::write_volatile(addr, value) }
    }

    /// Detect GENET version from hardware
    fn detect_version(&mut self) -> Result<(), DriverError> {
        let rev = self.read_reg(regs::SYS_REV_CTRL);
        let major = (rev >> 24) & 0x0f;

        self.version = match major {
            1 => GenetVersion::V1,
            2 => GenetVersion::V2,
            3 => GenetVersion::V3,
            4 => GenetVersion::V4,
            5 | 6 => GenetVersion::V5, // RPi4
            _ => return Err(DriverError::HardwareNotFound),
        };

        Ok(())
    }

    /// Read from MDIO (PHY registers)
    fn mdio_read(&self, phy_addr: u8, reg: u8) -> Result<u16, DriverError> {
        // Build MDIO command: read operation
        let cmd: u32 = (1 << 29)  // Start of frame
            | (2 << 26)          // Read operation
            | ((phy_addr as u32) << 21)
            | ((reg as u32) << 16);

        self.write_reg(regs::MDIO_CMD, cmd);

        // Wait for completion (with timeout)
        for _ in 0..1000 {
            let status = self.read_reg(regs::MDIO_CMD);
            if (status & (1 << 28)) != 0 {
                // Read complete
                return Ok((status & 0xffff) as u16);
            }
            // Small delay - in real implementation, use proper timing
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }

        Err(DriverError::Timeout)
    }

    /// Write to MDIO (PHY registers)
    fn mdio_write(&self, phy_addr: u8, reg: u8, value: u16) -> Result<(), DriverError> {
        // Build MDIO command: write operation
        let cmd: u32 = (1 << 29)  // Start of frame
            | (1 << 26)          // Write operation
            | ((phy_addr as u32) << 21)
            | ((reg as u32) << 16)
            | (value as u32);

        self.write_reg(regs::MDIO_CMD, cmd);

        // Wait for completion
        for _ in 0..1000 {
            let status = self.read_reg(regs::MDIO_CMD);
            if (status & (1 << 28)) != 0 {
                return Ok(());
            }
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }

        Err(DriverError::Timeout)
    }

    /// Initialize the PHY (BCM54213PE)
    fn init_phy(&mut self) -> Result<(), DriverError> {
        // Verify PHY ID
        let id1 = self.mdio_read(phy::PHY_ADDR, phy::MII_PHYSID1)?;
        let id2 = self.mdio_read(phy::PHY_ADDR, phy::MII_PHYSID2)?;
        let phy_id = ((id1 as u32) << 16) | (id2 as u32);

        if (phy_id & 0xfffffff0) != (phy::BCM54213PE_PHY_ID & 0xfffffff0) {
            return Err(DriverError::HardwareNotFound);
        }

        // Reset PHY
        self.mdio_write(phy::PHY_ADDR, phy::MII_BMCR, phy::BMCR_RESET)?;

        // Wait for reset to complete
        for _ in 0..1000 {
            let bmcr = self.mdio_read(phy::PHY_ADDR, phy::MII_BMCR)?;
            if (bmcr & phy::BMCR_RESET) == 0 {
                break;
            }
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }

        // Enable auto-negotiation
        self.mdio_write(
            phy::PHY_ADDR,
            phy::MII_BMCR,
            phy::BMCR_ANENABLE | phy::BMCR_ANRESTART,
        )?;

        Ok(())
    }

    /// Read MAC address from hardware
    fn read_mac_address(&mut self) {
        let mac0 = self.read_reg(regs::UMAC_MAC0);
        let mac1 = self.read_reg(regs::UMAC_MAC1);

        self.mac = MacAddress::new([
            ((mac0 >> 24) & 0xff) as u8,
            ((mac0 >> 16) & 0xff) as u8,
            ((mac0 >> 8) & 0xff) as u8,
            (mac0 & 0xff) as u8,
            ((mac1 >> 8) & 0xff) as u8,
            (mac1 & 0xff) as u8,
        ]);
    }

    /// Initialize UniMAC
    fn init_umac(&mut self) -> Result<(), DriverError> {
        // Read MAC address
        self.read_mac_address();

        // Set maximum frame length (1518 bytes for standard Ethernet)
        self.write_reg(regs::UMAC_MAX_FRAME_LEN, 1518);

        // TODO: Initialize TX and RX DMA rings
        // TODO: Enable interrupts
        // TODO: Enable UniMAC TX and RX

        Ok(())
    }

    /// Update link status from PHY
    fn update_link_status(&mut self) -> Result<(), DriverError> {
        let bmsr = self.mdio_read(phy::PHY_ADDR, phy::MII_BMSR)?;

        if (bmsr & phy::BMSR_LSTATUS) == 0 {
            self.link = LinkStatus::down();
            return Ok(());
        }

        // Link is up, determine speed
        let stat1000 = self.mdio_read(phy::PHY_ADDR, phy::MII_STAT1000)?;
        let lpa = self.mdio_read(phy::PHY_ADDR, phy::MII_LPA)?;

        let (speed, full_duplex) = if (stat1000 & 0x0800) != 0 {
            // 1000BASE-T Full Duplex
            (LinkSpeed::Speed1000, true)
        } else if (stat1000 & 0x0400) != 0 {
            // 1000BASE-T Half Duplex
            (LinkSpeed::Speed1000, false)
        } else if (lpa & 0x0100) != 0 {
            // 100BASE-TX Full Duplex
            (LinkSpeed::Speed100, true)
        } else if (lpa & 0x0080) != 0 {
            // 100BASE-TX Half Duplex
            (LinkSpeed::Speed100, false)
        } else if (lpa & 0x0040) != 0 {
            // 10BASE-T Full Duplex
            (LinkSpeed::Speed10, true)
        } else {
            // 10BASE-T Half Duplex
            (LinkSpeed::Speed10, false)
        };

        self.link = LinkStatus {
            up: true,
            speed: Some(speed),
            full_duplex,
        };

        Ok(())
    }
}

impl NetworkDriver for EthernetDriver {
    fn init() -> Result<Self, DriverError> {
        let mut driver = Self::new(GENET_BASE);

        // Detect GENET version
        driver.detect_version()?;

        // Initialize PHY
        driver.init_phy()?;

        // Initialize UniMAC
        driver.init_umac()?;

        // Update link status
        driver.update_link_status()?;

        Ok(driver)
    }

    fn mac_address(&self) -> MacAddress {
        self.mac
    }

    fn link_status(&self) -> LinkStatus {
        self.link
    }

    fn transmit(&mut self, packet: &[u8]) -> Result<(), DriverError> {
        if !self.link.up {
            return Err(DriverError::NoLink);
        }

        if packet.len() > 1518 {
            return Err(DriverError::InvalidConfig);
        }

        // TODO: Implement DMA ring buffer transmission
        // 1. Find next available TX descriptor
        // 2. Copy packet to DMA buffer
        // 3. Set descriptor flags and length
        // 4. Trigger TX DMA

        self.stats.tx_packets += 1;
        self.stats.tx_bytes += packet.len() as u64;

        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, DriverError> {
        if !self.link.up {
            return Err(DriverError::NoLink);
        }

        // TODO: Implement DMA ring buffer reception
        // 1. Check if RX descriptor has data
        // 2. Copy from DMA buffer to provided buffer
        // 3. Return descriptor to hardware
        // 4. Return packet length

        // Placeholder: no data available
        Ok(0)
    }

    fn handle_irq(&mut self) {
        // TODO: Implement interrupt handling
        // 1. Read interrupt status register
        // 2. Handle TX completion interrupts
        // 3. Handle RX completion interrupts
        // 4. Handle link change interrupts
        // 5. Clear interrupt status

        // Update link status on link change
        let _ = self.update_link_status();
    }

    fn stats(&self) -> DriverStats {
        self.stats
    }
}
