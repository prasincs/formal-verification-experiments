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
//! # DMA Buffer Region
//!
//! The GENET DMA engines require *physical* addresses for packet buffers,
//! while the driver reads/writes the buffers through a Microkit-mapped
//! *virtual* address. The system description therefore provides a dedicated
//! `net_dma` memory region with a fixed physical address, mapped uncached
//! into the network PD:
//!
//! - Physical address: `0x3e700000` (1MiB, below the mailbox DMA buffer at
//!   0x3e875000 and the framebuffer carve-out at 0x3e876000)
//! - Virtual address:  `0x5_0800_0000` (network PD mapping)
//!
//! These addresses are wired up by the caller (see `tvdemo-network.system`);
//! the driver itself only consumes them through [`DmaRegion`] and never
//! hardcodes them.
//!
//! Layout inside the region (2KiB buffers, 256 descriptors per ring):
//!
//! ```text
//! offset 0x00000 .. 0x80000 : 256 RX buffers (256 * 2048 = 512KiB)
//! offset 0x80000 .. 0x100000: 256 TX buffers (256 * 2048 = 512KiB)
//! ```
//!
//! The DMA *descriptors* do not live in this region: on GENET they are in
//! on-chip SRAM at the start of the RDMA/TDMA register blocks.
//!
//! # References
//!
//! - Linux driver: drivers/net/ethernet/broadcom/genet/ (bcmgenet.c/.h)
//! - Circle bare metal: https://github.com/rsta2/circle/blob/master/lib/bcm54213.cpp

use super::{DmaRegion, DriverError, DriverStats, LinkSpeed, LinkStatus, MacAddress, NetworkDriver};

/// GENET register offsets
///
/// Offsets follow the Linux `bcmgenet.h` GENET v5 hardware parameters
/// (`bcmgenet_hw_params` for `GENET_V5`: tdma_offset 0x4000, rdma_offset
/// 0x2000, words_per_bd 3) and the v4+ DMA ring register map
/// (`genet_dma_ring_regs_v4` / `bcmgenet_dma_regs_v3plus`).
#[allow(dead_code)]
mod regs {
    // System control
    pub const SYS_REV_CTRL: usize = 0x00;
    pub const SYS_PORT_CTRL: usize = 0x04;

    /// SYS_PORT_CTRL: external gigabit PHY (Linux PORT_MODE_EXT_GPHY)
    pub const PORT_MODE_EXT_GPHY: u32 = 3;

    // INTRL2 CPU interrupt controller 0 (offset 0x200, Linux GENET_INTRL2_0_OFF)
    pub const INTRL2_0_BASE: usize = 0x200;
    pub const INTRL2_CPU_STAT: usize = INTRL2_0_BASE + 0x00;
    pub const INTRL2_CPU_SET: usize = INTRL2_0_BASE + 0x04;
    pub const INTRL2_CPU_CLEAR: usize = INTRL2_0_BASE + 0x08;
    pub const INTRL2_CPU_MASK_STATUS: usize = INTRL2_0_BASE + 0x0c;
    pub const INTRL2_CPU_MASK_SET: usize = INTRL2_0_BASE + 0x10;
    pub const INTRL2_CPU_MASK_CLEAR: usize = INTRL2_0_BASE + 0x14;

    // INTRL2_0 interrupt bits (Linux UMAC_IRQ_*)
    pub const IRQ_LINK_UP: u32 = 1 << 4;
    pub const IRQ_LINK_DOWN: u32 = 1 << 5;
    pub const IRQ_LINK_EVENT: u32 = IRQ_LINK_UP | IRQ_LINK_DOWN;
    /// RXDMA "multi-buffer done" (Linux UMAC_IRQ_RXDMA_MBDONE == UMAC_IRQ_RXDMA_DONE)
    pub const IRQ_RXDMA_DONE: u32 = 1 << 13;
    /// TXDMA "multi-buffer done" (Linux UMAC_IRQ_TXDMA_MBDONE == UMAC_IRQ_TXDMA_DONE)
    pub const IRQ_TXDMA_DONE: u32 = 1 << 16;

    // UniMAC registers (offset 0x800)
    pub const UMAC_BASE: usize = 0x800;
    pub const UMAC_CMD: usize = UMAC_BASE + 0x008;
    pub const UMAC_MAC0: usize = UMAC_BASE + 0x00c;
    pub const UMAC_MAC1: usize = UMAC_BASE + 0x010;
    pub const UMAC_MAX_FRAME_LEN: usize = UMAC_BASE + 0x014;
    pub const UMAC_TX_FLUSH: usize = UMAC_BASE + 0x334;

    // UMAC_CMD bits (Linux CMD_*)
    pub const CMD_TX_EN: u32 = 1 << 0;
    pub const CMD_RX_EN: u32 = 1 << 1;
    pub const CMD_SW_RESET: u32 = 1 << 13;
    pub const CMD_LCL_LOOP_EN: u32 = 1 << 15;

    // MDIO registers (offset 0xe00)
    pub const MDIO_BASE: usize = 0xe00;
    pub const MDIO_CMD: usize = MDIO_BASE + 0x00;

    // ------------------------------------------------------------------
    // DMA geometry (GENET v3+ / v5)
    // ------------------------------------------------------------------

    /// Number of descriptors per ring (Linux TOTAL_DESC)
    pub const TOTAL_DESC: usize = 256;
    /// Words per buffer descriptor for GENET v3+ (length_status, addr_lo, addr_hi)
    pub const WORDS_PER_BD: usize = 3;
    /// Descriptor size in bytes
    pub const DMA_DESC_SIZE: usize = WORDS_PER_BD * 4;

    // Descriptor word offsets
    pub const DMA_DESC_LENGTH_STATUS: usize = 0x00;
    pub const DMA_DESC_ADDRESS_LO: usize = 0x04;
    pub const DMA_DESC_ADDRESS_HI: usize = 0x08;

    /// RX DMA block (descriptor SRAM at the start, ring regs after)
    pub const RDMA_REG_OFF: usize = 0x2000;
    /// TX DMA block
    pub const TDMA_REG_OFF: usize = 0x4000;

    /// Ring registers follow the descriptor SRAM
    /// (Linux GENET_RDMA_REG_OFF/GENET_TDMA_REG_OFF = offset + TOTAL_DESC * DMA_DESC_SIZE)
    pub const RDMA_RING_REG_BASE: usize = RDMA_REG_OFF + TOTAL_DESC * DMA_DESC_SIZE; // 0x2c00
    pub const TDMA_RING_REG_BASE: usize = TDMA_REG_OFF + TOTAL_DESC * DMA_DESC_SIZE; // 0x4c00

    /// Bytes of register space per ring (Linux DMA_RING_SIZE)
    pub const DMA_RING_REGS_SIZE: usize = 0x40;
    /// The default descriptor ring used by Linux on GENET v4+ (DESC_INDEX)
    pub const DEFAULT_RING: usize = 16;
    /// Total ring register space: rings 0..=16 (Linux DMA_RINGS_SIZE)
    pub const DMA_RINGS_SIZE: usize = DMA_RING_REGS_SIZE * (DEFAULT_RING + 1); // 0x440

    // Per-ring register offsets, GENET v4+ map (Linux genet_dma_ring_regs_v4).
    // The same numeric offsets serve both TDMA and RDMA; only the meaning of
    // the prod/cons pair is swapped between the two blocks.
    pub const TDMA_READ_PTR: usize = 0x00;
    pub const TDMA_CONS_INDEX: usize = 0x08;
    pub const TDMA_PROD_INDEX: usize = 0x0c;
    pub const DMA_RING_BUF_SIZE: usize = 0x10;
    pub const DMA_START_ADDR: usize = 0x14;
    pub const DMA_END_ADDR: usize = 0x1c;
    pub const DMA_MBUF_DONE_THRESH: usize = 0x24;
    pub const TDMA_FLOW_PERIOD: usize = 0x28;
    pub const TDMA_WRITE_PTR: usize = 0x2c;
    // RDMA aliases (Linux maps RDMA_* enum values onto the same v4 offsets)
    pub const RDMA_WRITE_PTR: usize = 0x00;
    pub const RDMA_PROD_INDEX: usize = 0x08;
    pub const RDMA_CONS_INDEX: usize = 0x0c;
    pub const RDMA_XON_XOFF_THRESH: usize = 0x28;
    pub const RDMA_READ_PTR: usize = 0x2c;

    // DMA control registers, after the ring register block
    // (Linux bcmgenet_dma_regs_v3plus, accessed at reg_off + DMA_RINGS_SIZE)
    pub const DMA_RING_CFG: usize = 0x00;
    pub const DMA_CTRL: usize = 0x04;
    pub const DMA_SCB_BURST_SIZE: usize = 0x0c;

    // DMA_CTRL bits
    pub const DMA_EN: u32 = 1 << 0;
    /// Ring buffer enable bits start at bit 1 (Linux DMA_RING_BUF_EN_SHIFT = 1)
    pub const DMA_RING_BUF_EN_SHIFT: u32 = 1;

    /// Default SCB burst size used by Linux (DMA_MAX_BURST_LENGTH = 8)
    pub const DMA_MAX_BURST_LENGTH: u32 = 0x08;

    // length_status fields (Linux DMA_* descriptor flags)
    pub const DMA_BUFLENGTH_SHIFT: u32 = 16;
    pub const DMA_BUFLENGTH_MASK: u32 = 0x0fff;
    pub const DMA_OWN: u32 = 0x8000;
    pub const DMA_EOP: u32 = 0x4000;
    pub const DMA_SOP: u32 = 0x2000;
    pub const DMA_WRAP: u32 = 0x1000;
    // TX flags
    pub const DMA_TX_APPEND_CRC: u32 = 0x0040;
    pub const DMA_TX_QTAG_SHIFT: u32 = 7;
    // RX status flags (low bits of length_status)
    pub const DMA_RX_OV: u32 = 0x0001;
    pub const DMA_RX_CRC_ERROR: u32 = 0x0002;
    pub const DMA_RX_RXER: u32 = 0x0004;
    pub const DMA_RX_NO: u32 = 0x0008;
    pub const DMA_RX_LG: u32 = 0x0010;
    pub const DMA_RX_ERRORS: u32 =
        DMA_RX_OV | DMA_RX_CRC_ERROR | DMA_RX_RXER | DMA_RX_NO | DMA_RX_LG;

    /// Producer/consumer index registers are 16-bit free-running counters
    pub const DMA_INDEX_MASK: u32 = 0xffff;

    /// RX/TX buffer size carved out of the DMA region (Linux RX_BUF_LENGTH)
    pub const BUF_LENGTH: usize = 2048;

    // Flow control thresholds for RDMA_XON_XOFF_THRESH.
    // Values from Linux bcmgenet.c: DMA_FC_THRESH_LO = 5,
    // DMA_FC_THRESH_HI = TOTAL_DESC >> 4, XOFF threshold in bits 16+.
    // (If these differ from the exact Linux values the ring still works;
    // they only tune pause-frame generation.)
    pub const DMA_FC_THRESH_LO: u32 = 5;
    pub const DMA_FC_THRESH_HI: u32 = (TOTAL_DESC as u32) >> 4;
    pub const DMA_XOFF_THRESHOLD_SHIFT: u32 = 16;
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

/// Minimum Ethernet frame length excluding FCS (the hardware appends the
/// CRC; short frames are zero-padded to this length by the driver).
const ETH_ZLEN: usize = 60;

/// Maximum frame length accepted for transmit / programmed into the UniMAC.
const ETH_MAX_FRAME_LEN: usize = 1518;

// GENET layout inside the shared DmaRegion (see super::DmaRegion). For the
// tvdemo network PD this is the `net_dma` region: paddr `0x3e700000`, vaddr
// `0x5_0800_0000`, size 1MiB (>= 256+256 2KiB buffers) — but the driver
// takes whatever it is given and never hardcodes those values.

/// Virtual address of RX buffer `slot`
fn rx_buf_vaddr(region: &DmaRegion, slot: usize) -> usize {
    region.vaddr + slot * regs::BUF_LENGTH
}

/// Physical address of RX buffer `slot`
fn rx_buf_paddr(region: &DmaRegion, slot: usize) -> usize {
    region.paddr + slot * regs::BUF_LENGTH
}

/// Virtual address of TX buffer `slot` (TX half follows the RX half)
fn tx_buf_vaddr(region: &DmaRegion, slot: usize) -> usize {
    region.vaddr + (regs::TOTAL_DESC + slot) * regs::BUF_LENGTH
}

/// Physical address of TX buffer `slot`
fn tx_buf_paddr(region: &DmaRegion, slot: usize) -> usize {
    region.paddr + (regs::TOTAL_DESC + slot) * regs::BUF_LENGTH
}

/// Runtime DMA ring state (present once `attach_dma` has succeeded)
struct DmaState {
    region: DmaRegion,
    /// TX producer index (free-running 16-bit, mirrors TDMA_PROD_INDEX)
    tx_prod: u16,
    /// Last TX consumer index observed from hardware
    tx_cons: u16,
    /// RX consumer index (free-running 16-bit, mirrors RDMA_CONS_INDEX)
    rx_cons: u16,
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
    /// DMA ring state; `None` until `attach_dma` is called
    dma: Option<DmaState>,
}

impl EthernetDriver {
    /// Initialize the driver
    ///
    /// `base` is the *virtual* address of the GENET registers, as mapped
    /// by the Microkit system description (not the physical 0xFD580000).
    ///
    /// This performs link/PHY and UniMAC setup only. TX/RX return
    /// `DriverError::DmaNotAttached` until [`Self::attach_dma`] is called
    /// with a physically-addressable buffer region.
    pub fn init(base: usize) -> Result<Self, DriverError> {
        let mut driver = Self::new(base);

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

    /// Create a new Ethernet driver instance
    fn new(base: usize) -> Self {
        Self {
            base,
            version: GenetVersion::V5,
            mac: MacAddress::new([0; 6]),
            link: LinkStatus::down(),
            stats: DriverStats::default(),
            dma: None,
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

    // ------------------------------------------------------------------
    // DMA register helpers
    // ------------------------------------------------------------------

    /// Byte offset of RX descriptor `index` in the RDMA descriptor SRAM
    #[inline]
    fn rx_desc_off(index: usize) -> usize {
        regs::RDMA_REG_OFF + index * regs::DMA_DESC_SIZE
    }

    /// Byte offset of TX descriptor `index` in the TDMA descriptor SRAM
    #[inline]
    fn tx_desc_off(index: usize) -> usize {
        regs::TDMA_REG_OFF + index * regs::DMA_DESC_SIZE
    }

    /// Read a per-ring RDMA register for the default ring (ring 16)
    #[inline]
    fn rdma_ring_read(&self, reg: usize) -> u32 {
        self.read_reg(
            regs::RDMA_RING_REG_BASE + regs::DEFAULT_RING * regs::DMA_RING_REGS_SIZE + reg,
        )
    }

    /// Write a per-ring RDMA register for the default ring (ring 16)
    #[inline]
    fn rdma_ring_write(&self, reg: usize, value: u32) {
        self.write_reg(
            regs::RDMA_RING_REG_BASE + regs::DEFAULT_RING * regs::DMA_RING_REGS_SIZE + reg,
            value,
        )
    }

    /// Read a per-ring TDMA register for the default ring (ring 16)
    #[inline]
    fn tdma_ring_read(&self, reg: usize) -> u32 {
        self.read_reg(
            regs::TDMA_RING_REG_BASE + regs::DEFAULT_RING * regs::DMA_RING_REGS_SIZE + reg,
        )
    }

    /// Write a per-ring TDMA register for the default ring (ring 16)
    #[inline]
    fn tdma_ring_write(&self, reg: usize, value: u32) {
        self.write_reg(
            regs::TDMA_RING_REG_BASE + regs::DEFAULT_RING * regs::DMA_RING_REGS_SIZE + reg,
            value,
        )
    }

    /// Read an RDMA block control register (DMA_RING_CFG/DMA_CTRL/...)
    #[inline]
    fn rdma_read(&self, reg: usize) -> u32 {
        self.read_reg(regs::RDMA_RING_REG_BASE + regs::DMA_RINGS_SIZE + reg)
    }

    /// Write an RDMA block control register
    #[inline]
    fn rdma_write(&self, reg: usize, value: u32) {
        self.write_reg(regs::RDMA_RING_REG_BASE + regs::DMA_RINGS_SIZE + reg, value)
    }

    /// Read a TDMA block control register
    #[inline]
    fn tdma_read(&self, reg: usize) -> u32 {
        self.read_reg(regs::TDMA_RING_REG_BASE + regs::DMA_RINGS_SIZE + reg)
    }

    /// Write a TDMA block control register
    #[inline]
    fn tdma_write(&self, reg: usize, value: u32) {
        self.write_reg(regs::TDMA_RING_REG_BASE + regs::DMA_RINGS_SIZE + reg, value)
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

    /// Write the cached MAC address back to the UniMAC registers
    fn write_mac_address(&self) {
        let m = self.mac.0;
        let mac0 = ((m[0] as u32) << 24) | ((m[1] as u32) << 16) | ((m[2] as u32) << 8) | m[3] as u32;
        let mac1 = ((m[4] as u32) << 8) | m[5] as u32;
        self.write_reg(regs::UMAC_MAC0, mac0);
        self.write_reg(regs::UMAC_MAC1, mac1);
    }

    /// Initialize UniMAC
    ///
    /// Performs a software reset of the MAC, restores the MAC address
    /// programmed by firmware, and sets the port mode / frame length.
    /// TX/RX and the DMA rings stay disabled until `attach_dma`.
    fn init_umac(&mut self) -> Result<(), DriverError> {
        // Read the MAC address the firmware programmed *before* resetting
        // the UniMAC (the reset clears it).
        self.read_mac_address();

        // Software-reset the UniMAC. Linux (umac_reset / reset_umac) pulses
        // CMD_SW_RESET; Circle additionally sets CMD_LCL_LOOP_EN during the
        // reset pulse, which we follow here.
        self.write_reg(regs::UMAC_CMD, regs::CMD_SW_RESET | regs::CMD_LCL_LOOP_EN);
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
        self.write_reg(regs::UMAC_CMD, 0);

        // Restore the MAC address after the reset
        self.write_mac_address();

        // External gigabit PHY port mode (Linux PORT_MODE_EXT_GPHY = 3).
        // NOTE: full RGMII pad setup (EXT_RGMII_OOB_CTRL: RGMII_MODE_EN,
        // ID_MODE_DIS in the EXT block at offset 0x80) is done by Linux
        // bcmmii.c; the RPi firmware normally leaves this configured, so we
        // only set the port mode here.
        self.write_reg(regs::SYS_PORT_CTRL, regs::PORT_MODE_EXT_GPHY);

        // Set maximum frame length (1518 bytes for standard Ethernet)
        self.write_reg(regs::UMAC_MAX_FRAME_LEN, ETH_MAX_FRAME_LEN as u32);

        // Mask and clear all INTRL2_0 interrupts until DMA is attached
        self.write_reg(regs::INTRL2_CPU_MASK_SET, 0xffff_ffff);
        self.write_reg(regs::INTRL2_CPU_CLEAR, 0xffff_ffff);

        // TX/RX enable and DMA ring setup happen in `attach_dma`.

        Ok(())
    }

    /// Attach a DMA buffer region and bring up the TX/RX rings
    ///
    /// Carves 256 RX + 256 TX buffers of 2KiB each (1MiB total) out of
    /// `region`, programs the RDMA/TDMA descriptors (in on-chip SRAM) and
    /// ring 16 configuration registers, enables both DMA engines, unmasks
    /// the RXDMA/TXDMA done interrupts and finally enables UniMAC TX/RX.
    pub fn attach_dma(&mut self, region: DmaRegion) -> Result<(), DriverError> {
        const REQUIRED: usize = 2 * regs::TOTAL_DESC * regs::BUF_LENGTH; // 1MiB

        if region.size < REQUIRED || region.vaddr == 0 || region.paddr == 0 {
            return Err(DriverError::InvalidConfig);
        }
        // GENET descriptors hold a 64-bit address but this driver assumes
        // buffers live in the lower 4GiB (true for the RPi4 carve-out at
        // 0x3e700000); addr_hi is still programmed for completeness.
        if region.paddr.checked_add(REQUIRED).is_none() {
            return Err(DriverError::InvalidConfig);
        }

        // ------------------------------------------------------------------
        // 1. Quiesce: disable UniMAC RX/TX and both DMA engines
        // ------------------------------------------------------------------
        let cmd = self.read_reg(regs::UMAC_CMD);
        self.write_reg(regs::UMAC_CMD, cmd & !(regs::CMD_TX_EN | regs::CMD_RX_EN));

        self.rdma_write(regs::DMA_CTRL, 0);
        self.tdma_write(regs::DMA_CTRL, 0);

        // Flush the TX FIFO (Linux umac_enable_set/bcmgenet_umac_reset path)
        self.write_reg(regs::UMAC_TX_FLUSH, 1);
        for _ in 0..100 {
            core::hint::spin_loop();
        }
        self.write_reg(regs::UMAC_TX_FLUSH, 0);

        // ------------------------------------------------------------------
        // 2. Program descriptor SRAM
        // ------------------------------------------------------------------
        for i in 0..regs::TOTAL_DESC {
            // RX descriptor i -> RX buffer i
            let rx_paddr = rx_buf_paddr(&region, i) as u64;
            let off = Self::rx_desc_off(i);
            self.write_reg(off + regs::DMA_DESC_ADDRESS_LO, rx_paddr as u32);
            self.write_reg(off + regs::DMA_DESC_ADDRESS_HI, (rx_paddr >> 32) as u32);
            // Buffer length; hardware fills in the real length/status on
            // completion (the ring's DMA_RING_BUF_SIZE also encodes it).
            self.write_reg(
                off + regs::DMA_DESC_LENGTH_STATUS,
                (regs::BUF_LENGTH as u32) << regs::DMA_BUFLENGTH_SHIFT,
            );

            // TX descriptor i -> TX buffer i (address programmed again on
            // every transmit; cleared here for a known-good initial state)
            let tx_paddr = tx_buf_paddr(&region, i) as u64;
            let off = Self::tx_desc_off(i);
            self.write_reg(off + regs::DMA_DESC_ADDRESS_LO, tx_paddr as u32);
            self.write_reg(off + regs::DMA_DESC_ADDRESS_HI, (tx_paddr >> 32) as u32);
            self.write_reg(off + regs::DMA_DESC_LENGTH_STATUS, 0);
        }

        // ------------------------------------------------------------------
        // 3. RX ring 16 configuration (mirrors Linux bcmgenet_init_rx_ring)
        // ------------------------------------------------------------------
        self.rdma_ring_write(regs::RDMA_PROD_INDEX, 0);
        self.rdma_ring_write(regs::RDMA_CONS_INDEX, 0);
        self.rdma_ring_write(
            regs::DMA_RING_BUF_SIZE,
            ((regs::TOTAL_DESC as u32) << 16) | regs::BUF_LENGTH as u32,
        );
        self.rdma_ring_write(
            regs::RDMA_XON_XOFF_THRESH,
            (regs::DMA_FC_THRESH_LO << regs::DMA_XOFF_THRESHOLD_SHIFT) | regs::DMA_FC_THRESH_HI,
        );
        // Start/end/read/write pointers are in units of descriptor words
        let end_ptr = (regs::TOTAL_DESC * regs::WORDS_PER_BD - 1) as u32;
        self.rdma_ring_write(regs::DMA_START_ADDR, 0);
        self.rdma_ring_write(regs::RDMA_READ_PTR, 0);
        self.rdma_ring_write(regs::RDMA_WRITE_PTR, 0);
        self.rdma_ring_write(regs::DMA_END_ADDR, end_ptr);

        // ------------------------------------------------------------------
        // 4. TX ring 16 configuration (mirrors Linux bcmgenet_init_tx_ring)
        // ------------------------------------------------------------------
        self.tdma_ring_write(regs::TDMA_PROD_INDEX, 0);
        self.tdma_ring_write(regs::TDMA_CONS_INDEX, 0);
        self.tdma_ring_write(regs::DMA_MBUF_DONE_THRESH, 1);
        self.tdma_ring_write(regs::TDMA_FLOW_PERIOD, 0); // no rate control
        self.tdma_ring_write(
            regs::DMA_RING_BUF_SIZE,
            ((regs::TOTAL_DESC as u32) << 16) | regs::BUF_LENGTH as u32,
        );
        self.tdma_ring_write(regs::DMA_START_ADDR, 0);
        self.tdma_ring_write(regs::TDMA_READ_PTR, 0);
        self.tdma_ring_write(regs::TDMA_WRITE_PTR, 0);
        self.tdma_ring_write(regs::DMA_END_ADDR, end_ptr);

        // ------------------------------------------------------------------
        // 5. Enable the rings and the DMA engines
        // ------------------------------------------------------------------
        self.rdma_write(regs::DMA_SCB_BURST_SIZE, regs::DMA_MAX_BURST_LENGTH);
        self.tdma_write(regs::DMA_SCB_BURST_SIZE, regs::DMA_MAX_BURST_LENGTH);

        let ring_cfg = 1u32 << regs::DEFAULT_RING;
        self.rdma_write(regs::DMA_RING_CFG, ring_cfg);
        self.tdma_write(regs::DMA_RING_CFG, ring_cfg);

        let ring_en = 1u32 << (regs::DEFAULT_RING as u32 + regs::DMA_RING_BUF_EN_SHIFT);
        self.rdma_write(regs::DMA_CTRL, regs::DMA_EN | ring_en);
        self.tdma_write(regs::DMA_CTRL, regs::DMA_EN | ring_en);

        // ------------------------------------------------------------------
        // 6. Interrupts: clear everything, unmask RX/TX done + link events
        // ------------------------------------------------------------------
        self.write_reg(regs::INTRL2_CPU_MASK_SET, 0xffff_ffff);
        self.write_reg(regs::INTRL2_CPU_CLEAR, 0xffff_ffff);
        self.write_reg(
            regs::INTRL2_CPU_MASK_CLEAR,
            regs::IRQ_RXDMA_DONE | regs::IRQ_TXDMA_DONE | regs::IRQ_LINK_EVENT,
        );

        // ------------------------------------------------------------------
        // 7. Enable UniMAC TX/RX
        // ------------------------------------------------------------------
        let cmd = self.read_reg(regs::UMAC_CMD);
        self.write_reg(regs::UMAC_CMD, cmd | regs::CMD_TX_EN | regs::CMD_RX_EN);

        self.dma = Some(DmaState {
            region,
            tx_prod: 0,
            tx_cons: 0,
            rx_cons: 0,
        });
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

        if packet.is_empty() || packet.len() > ETH_MAX_FRAME_LEN {
            return Err(DriverError::InvalidConfig);
        }

        let (region, tx_prod) = match &self.dma {
            Some(d) => (d.region, d.tx_prod),
            None => return Err(DriverError::DmaNotAttached),
        };

        // Ring-full check: producer/consumer indices are free-running 16-bit
        // counters; the difference is the number of in-flight descriptors.
        let tx_cons = (self.tdma_ring_read(regs::TDMA_CONS_INDEX) & regs::DMA_INDEX_MASK) as u16;

        let in_flight = tx_prod.wrapping_sub(tx_cons);
        if in_flight as usize >= regs::TOTAL_DESC {
            self.stats.dropped += 1;
            return Err(DriverError::BufferAllocation);
        }

        let slot = (tx_prod as usize) % regs::TOTAL_DESC;

        // Copy the frame into the DMA buffer, zero-padding runts to the
        // 60-byte minimum (the hardware appends the 4-byte FCS).
        let buf = tx_buf_vaddr(&region, slot) as *mut u8;
        let len = packet.len().max(ETH_ZLEN);
        // Safety: `buf` points into the attached, mapped DMA region and
        // `len <= 1518 < BUF_LENGTH`.
        unsafe {
            core::ptr::copy_nonoverlapping(packet.as_ptr(), buf, packet.len());
            if packet.len() < ETH_ZLEN {
                core::ptr::write_bytes(buf.add(packet.len()), 0, ETH_ZLEN - packet.len());
            }
        }

        // Fill the TX descriptor: single-fragment frame, append CRC.
        // QTAG 0x3F at shift 7 matches Linux/Circle for the default ring.
        let paddr = tx_buf_paddr(&region, slot) as u64;
        let length_status = ((len as u32) << regs::DMA_BUFLENGTH_SHIFT)
            | (0x3f << regs::DMA_TX_QTAG_SHIFT)
            | regs::DMA_TX_APPEND_CRC
            | regs::DMA_SOP
            | regs::DMA_EOP;

        let next_prod = tx_prod.wrapping_add(1);
        if let Some(dma) = self.dma.as_mut() {
            dma.tx_prod = next_prod;
            dma.tx_cons = tx_cons;
        }

        let off = Self::tx_desc_off(slot);
        self.write_reg(off + regs::DMA_DESC_ADDRESS_LO, paddr as u32);
        self.write_reg(off + regs::DMA_DESC_ADDRESS_HI, (paddr >> 32) as u32);
        self.write_reg(off + regs::DMA_DESC_LENGTH_STATUS, length_status);

        // Make sure the buffer contents are visible to the device before
        // the producer-index write kicks off the DMA (region is mapped
        // uncached, so a barrier against store reordering is all we need).
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Advancing the producer index hands the descriptor to hardware
        self.tdma_ring_write(regs::TDMA_PROD_INDEX, next_prod as u32);

        self.stats.tx_packets += 1;
        self.stats.tx_bytes += packet.len() as u64;

        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, DriverError> {
        if !self.link.up {
            return Err(DriverError::NoLink);
        }

        let (region, rx_cons) = match &self.dma {
            Some(d) => (d.region, d.rx_cons),
            None => return Err(DriverError::DmaNotAttached),
        };

        // Anything new? Compare the hardware producer index with our
        // consumer index (both free-running 16-bit counters).
        let prod = (self.rdma_ring_read(regs::RDMA_PROD_INDEX) & regs::DMA_INDEX_MASK) as u16;
        if prod == rx_cons {
            return Ok(0); // no frame available
        }

        let slot = (rx_cons as usize) % regs::TOTAL_DESC;
        let off = Self::rx_desc_off(slot);
        let length_status = self.read_reg(off + regs::DMA_DESC_LENGTH_STATUS);

        let len =
            ((length_status >> regs::DMA_BUFLENGTH_SHIFT) & regs::DMA_BUFLENGTH_MASK) as usize;

        // Validate the frame: must be a complete single-descriptor frame
        // (SOP+EOP; 2KiB buffers always fit a 1518-byte frame) with no RX
        // error flags. Note: RX frames do *not* include the FCS because
        // UMAC_CMD.CRC_FWD is left disabled.
        let sop_eop = regs::DMA_SOP | regs::DMA_EOP;
        let mut result = 0usize;

        if (length_status & sop_eop) != sop_eop
            || (length_status & regs::DMA_RX_ERRORS) != 0
            || len == 0
        {
            self.stats.rx_errors += 1;
        } else if len > buffer.len() {
            // Caller's buffer is too small: drop the frame but still
            // recycle the descriptor so the ring keeps flowing.
            self.stats.dropped += 1;
        } else {
            // Safety: `src` points into the attached, mapped DMA region and
            // `len <= BUF_LENGTH`; `buffer` bounds were checked above.
            let src = rx_buf_vaddr(&region, slot) as *const u8;
            unsafe {
                core::ptr::copy_nonoverlapping(src, buffer.as_mut_ptr(), len);
            }
            self.stats.rx_packets += 1;
            self.stats.rx_bytes += len as u64;
            result = len;
        }

        // Restore the descriptor for reuse by hardware
        let paddr = rx_buf_paddr(&region, slot) as u64;
        let rx_cons = rx_cons.wrapping_add(1);
        if let Some(dma) = self.dma.as_mut() {
            dma.rx_cons = rx_cons;
        }

        self.write_reg(off + regs::DMA_DESC_ADDRESS_LO, paddr as u32);
        self.write_reg(off + regs::DMA_DESC_ADDRESS_HI, (paddr >> 32) as u32);
        self.write_reg(
            off + regs::DMA_DESC_LENGTH_STATUS,
            (regs::BUF_LENGTH as u32) << regs::DMA_BUFLENGTH_SHIFT,
        );

        // Advance the consumer index to hand the descriptor back
        self.rdma_ring_write(regs::RDMA_CONS_INDEX, rx_cons as u32);

        Ok(result)
    }

    fn handle_irq(&mut self) {
        // Read pending, unmasked interrupts and acknowledge them
        let status =
            self.read_reg(regs::INTRL2_CPU_STAT) & !self.read_reg(regs::INTRL2_CPU_MASK_STATUS);
        if status != 0 {
            self.write_reg(regs::INTRL2_CPU_CLEAR, status);
        }

        // TX completion: reap finished descriptors by syncing the consumer
        // index. Buffers are recycled in-place, so updating the cached
        // consumer index is all that is needed to free ring slots.
        if (status & regs::IRQ_TXDMA_DONE) != 0 {
            let tx_cons = self.tdma_ring_read(regs::TDMA_CONS_INDEX) & regs::DMA_INDEX_MASK;
            if let Some(dma) = self.dma.as_mut() {
                dma.tx_cons = tx_cons as u16;
            }
        }

        // RX done: nothing to do here; the driver is polling-friendly and
        // `receive` reaps frames directly from the ring.

        // Link change (or any other cause): refresh the PHY link status
        if (status & regs::IRQ_LINK_EVENT) != 0 || status == 0 {
            let _ = self.update_link_status();
        }
    }

    fn stats(&self) -> DriverStats {
        self.stats
    }
}
