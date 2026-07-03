//! Synopsys DesignWare USB 2.0 OTG (DWC2) host-controller driver
//!
//! The Raspberry Pi 4 (BCM2711) exposes a DWC2 OTG core at physical
//! `0xFE98_0000`. On a stock Pi the USB-A ports hang off the VL805 xHCI
//! controller behind PCIe, while the DWC2 core drives the USB-C / OTG port
//! (and, on earlier Pis, the on-board LAN9514 hub). For a bare-metal HID
//! keyboard the DWC2 core is the simplest usable host: it needs no PCIe
//! bring-up and is the controller bare-metal projects target.
//!
//! This module implements only the slice of DWC2 needed to talk to a single
//! low/full-speed boot-protocol HID keyboard on the root port:
//!
//! - core soft reset and host-mode configuration,
//! - root-port power / reset / speed detection,
//! - single host-channel control and interrupt-IN transfers using the core's
//!   *internal DMA* engine.
//!
//! Split transactions (a low-speed device behind a high-speed hub) are **not**
//! implemented; the keyboard is assumed to be on the root port. Enumeration
//! and the HID layer live in [`crate::usb::hid`] and [`crate::usb`].
//!
//! # References
//!
//! - Linux `drivers/usb/dwc2/` (`hw.h` register map, `hcd.c` channel setup)
//! - Circle bare-metal: `lib/usb/dwhcidevice.cpp` / `dwhci.h`
//! - Synopsys DWC_otg databook (host mode, "internal DMA")

use core::ptr::{read_volatile, write_volatile};

use super::{DmaRegion, EpType, Pid, TransferStatus, UsbError, UsbSpeed};

/// DWC2 physical base address on the BCM2711 (Raspberry Pi 4).
pub const DWC2_BASE: usize = 0xFE98_0000;

/// Register offsets, following the Linux `dwc2/hw.h` naming.
#[allow(dead_code)]
mod reg {
    // --- Core global registers ---
    pub const GOTGCTL: usize = 0x000;
    pub const GAHBCFG: usize = 0x008;
    pub const GUSBCFG: usize = 0x00C;
    pub const GRSTCTL: usize = 0x010;
    pub const GINTSTS: usize = 0x014;
    pub const GINTMSK: usize = 0x018;
    pub const GRXFSIZ: usize = 0x024;
    pub const GNPTXFSIZ: usize = 0x028;
    pub const GHWCFG2: usize = 0x048;
    pub const HPTXFSIZ: usize = 0x100;

    // --- Host mode registers ---
    pub const HCFG: usize = 0x400;
    pub const HFIR: usize = 0x404;
    pub const HFNUM: usize = 0x408;
    pub const HPRT0: usize = 0x440;

    // --- Host channel register block ---
    /// First channel register block.
    pub const HC_BASE: usize = 0x500;
    /// Stride between consecutive channel register blocks.
    pub const HC_STRIDE: usize = 0x20;

    // Per-channel register offsets (added to the channel block base).
    pub const HCCHAR: usize = 0x00;
    pub const HCSPLT: usize = 0x04;
    pub const HCINT: usize = 0x08;
    pub const HCINTMSK: usize = 0x0C;
    pub const HCTSIZ: usize = 0x10;
    pub const HCDMA: usize = 0x14;

    // --- Power and clock gating ---
    pub const PCGCR: usize = 0xE00;

    /// Base of channel `n`'s register block.
    #[inline]
    pub const fn hc(n: usize) -> usize {
        HC_BASE + n * HC_STRIDE
    }
}

// GAHBCFG bits
const GAHBCFG_GLBL_INTR_EN: u32 = 1 << 0;
const GAHBCFG_DMA_EN: u32 = 1 << 5;

// GUSBCFG bits
const GUSBCFG_PHYSEL_FS: u32 = 1 << 6;
const GUSBCFG_FORCE_HOST: u32 = 1 << 29;
const GUSBCFG_FORCE_DEV: u32 = 1 << 30;

// GRSTCTL bits
const GRSTCTL_CSFTRST: u32 = 1 << 0;
const GRSTCTL_RXFFLSH: u32 = 1 << 4;
const GRSTCTL_TXFFLSH: u32 = 1 << 5;
const GRSTCTL_TXFNUM_ALL: u32 = 0x10 << 6;
const GRSTCTL_AHBIDLE: u32 = 1 << 31;

// GINTSTS / GINTMSK bit for "current mode" (1 = host)
const GINTSTS_CURMODE_HOST: u32 = 1 << 0;

// HCFG FS/LS PHY clock select (48MHz) in bits [1:0]
const HCFG_FSLSPCLKSEL_48MHZ: u32 = 1;
const HCFG_FSLSSUPP: u32 = 1 << 2;

// HPRT0 bits. Note: HPRT0 has "write-1-to-clear" change bits (CONNDET,
// ENCHNG, OVRCURRCHNG); a read-modify-write must mask these off before
// writing other bits back, or it clears the change flags as a side effect.
const HPRT0_CONNSTS: u32 = 1 << 0;
const HPRT0_CONNDET: u32 = 1 << 1;
const HPRT0_ENA: u32 = 1 << 2;
const HPRT0_ENCHNG: u32 = 1 << 3;
const HPRT0_OVRCURRCHNG: u32 = 1 << 5;
const HPRT0_RST: u32 = 1 << 8;
const HPRT0_PWR: u32 = 1 << 12;
const HPRT0_SPD_SHIFT: u32 = 17;
const HPRT0_SPD_MASK: u32 = 0b11 << HPRT0_SPD_SHIFT;
/// Change/clear bits that must be masked off on a read-modify-write.
const HPRT0_WC_BITS: u32 = HPRT0_CONNDET | HPRT0_ENCHNG | HPRT0_OVRCURRCHNG;

// HCCHAR bits
const HCCHAR_MPS_MASK: u32 = 0x7FF;
const HCCHAR_EPNUM_SHIFT: u32 = 11;
const HCCHAR_EPDIR_IN: u32 = 1 << 15;
const HCCHAR_LSPDDEV: u32 = 1 << 17;
const HCCHAR_EPTYPE_SHIFT: u32 = 18;
const HCCHAR_MC_SHIFT: u32 = 20;
const HCCHAR_DEVADDR_SHIFT: u32 = 22;
const HCCHAR_ODDFRM: u32 = 1 << 29;
const HCCHAR_CHDIS: u32 = 1 << 30;
const HCCHAR_CHENA: u32 = 1 << 31;

// HCTSIZ bits
const HCTSIZ_XFERSIZE_MASK: u32 = 0x7FFFF;
const HCTSIZ_PKTCNT_SHIFT: u32 = 19;
const HCTSIZ_PKTCNT_MASK: u32 = 0x3FF;
const HCTSIZ_PID_SHIFT: u32 = 29;

// HCINT bits
const HCINT_XFERCOMPL: u32 = 1 << 0;
const HCINT_CHHLTD: u32 = 1 << 1;
const HCINT_AHBERR: u32 = 1 << 2;
const HCINT_STALL: u32 = 1 << 3;
const HCINT_NAK: u32 = 1 << 4;
const HCINT_ACK: u32 = 1 << 5;
const HCINT_NYET: u32 = 1 << 6;
const HCINT_XACTERR: u32 = 1 << 7;
const HCINT_BBLERR: u32 = 1 << 8;
const HCINT_FRMOVRUN: u32 = 1 << 9;
const HCINT_DATATGLERR: u32 = 1 << 10;
const HCINT_ALL: u32 = 0x7FF;

/// Spin-loop iteration budget for polling a hardware bit before giving up.
const POLL_BUDGET: u32 = 2_000_000;

/// Encoded PID value for the HCTSIZ `PID` field.
#[inline]
fn pid_code(pid: Pid) -> u32 {
    match pid {
        Pid::Data0 => 0,
        Pid::Data2 => 1,
        Pid::Data1 => 2,
        Pid::Setup => 3, // also MDATA
    }
}

/// Parameters describing a single host-channel transfer.
#[derive(Clone, Copy, Debug)]
pub struct ChannelParams {
    /// Device address (0 before SET_ADDRESS, assigned address afterwards).
    pub dev_addr: u8,
    /// Endpoint number.
    pub ep_num: u8,
    /// `true` for an IN transfer (device→host).
    pub ep_dir_in: bool,
    /// Endpoint transfer type.
    pub ep_type: EpType,
    /// Endpoint max packet size in bytes.
    pub max_packet: u16,
    /// `true` if the device operates at low speed (1.5 Mbps).
    pub low_speed: bool,
    /// Data toggle / packet id to start the transfer with.
    pub pid: Pid,
    /// Physical DMA address of the transfer buffer.
    pub dma_addr: u32,
    /// Transfer length in bytes.
    pub length: u32,
}

/// Result of a completed (or failed) host-channel transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferResult {
    /// How the transfer terminated.
    pub status: TransferStatus,
    /// Bytes actually transferred (valid for IN transfers on success).
    pub bytes: u32,
}

/// Compute the number of USB packets needed for `length` bytes at
/// `max_packet`. A zero-length transfer still costs one packet.
#[inline]
pub fn packet_count(length: u32, max_packet: u16) -> u32 {
    if length == 0 {
        1
    } else {
        length.div_ceil(max_packet as u32)
    }
}

/// Encode the `HCCHAR` register value for a transfer.
///
/// `odd_frame` selects the micro-frame parity for periodic (interrupt)
/// transfers and is ignored by the hardware for control transfers.
pub fn encode_hcchar(p: &ChannelParams, odd_frame: bool) -> u32 {
    let mut v = (p.max_packet as u32) & HCCHAR_MPS_MASK;
    v |= ((p.ep_num as u32) & 0xF) << HCCHAR_EPNUM_SHIFT;
    if p.ep_dir_in {
        v |= HCCHAR_EPDIR_IN;
    }
    if p.low_speed {
        v |= HCCHAR_LSPDDEV;
    }
    v |= (p.ep_type as u32) << HCCHAR_EPTYPE_SHIFT;
    // Multi-count / error-count field: 1 transaction per frame.
    v |= 1 << HCCHAR_MC_SHIFT;
    v |= ((p.dev_addr as u32) & 0x7F) << HCCHAR_DEVADDR_SHIFT;
    if odd_frame {
        v |= HCCHAR_ODDFRM;
    }
    v |= HCCHAR_CHENA;
    v
}

/// Encode the `HCTSIZ` register value for a transfer.
pub fn encode_hctsiz(length: u32, max_packet: u16, pid: Pid) -> u32 {
    let pkts = packet_count(length, max_packet) & HCTSIZ_PKTCNT_MASK;
    (length & HCTSIZ_XFERSIZE_MASK)
        | (pkts << HCTSIZ_PKTCNT_SHIFT)
        | (pid_code(pid) << HCTSIZ_PID_SHIFT)
}

/// Decode a halted channel's `HCINT` bits into a [`TransferStatus`].
///
/// Priority matches the DWC2 host interrupt handling order: a transfer-complete
/// bit wins, then the terminal error conditions, then flow-control (NAK/NYET).
pub fn decode_hcint(hcint: u32) -> TransferStatus {
    if hcint & HCINT_XFERCOMPL != 0 {
        TransferStatus::Completed
    } else if hcint & HCINT_STALL != 0 {
        TransferStatus::Stall
    } else if hcint
        & (HCINT_XACTERR | HCINT_BBLERR | HCINT_AHBERR | HCINT_DATATGLERR | HCINT_FRMOVRUN)
        != 0
    {
        TransferStatus::Error
    } else if hcint & (HCINT_NAK | HCINT_NYET) != 0 {
        TransferStatus::Nak
    } else {
        // Channel halted with no meaningful completion bit set.
        TransferStatus::Error
    }
}

/// Decode the `HPRT0` speed field into a [`UsbSpeed`].
pub fn decode_port_speed(hprt0: u32) -> UsbSpeed {
    match (hprt0 & HPRT0_SPD_MASK) >> HPRT0_SPD_SHIFT {
        0 => UsbSpeed::High,
        1 => UsbSpeed::Full,
        2 => UsbSpeed::Low,
        _ => UsbSpeed::Full,
    }
}

/// DWC2 host controller bound to a mapped MMIO base and a DMA buffer region.
pub struct Dwc2 {
    base: usize,
    dma: DmaRegion,
    odd_frame: bool,
}

impl Dwc2 {
    /// Create a driver over the MMIO region at `base` (a Microkit-mapped
    /// virtual address) using `dma` for transfer buffers.
    ///
    /// # Safety
    /// `base` must be the mapped DWC2 register window and `dma` must describe a
    /// physically-contiguous, uncached region mapped into this PD.
    pub const unsafe fn new(base: usize, dma: DmaRegion) -> Self {
        Self {
            base,
            dma,
            odd_frame: false,
        }
    }

    #[inline]
    fn read(&self, off: usize) -> u32 {
        unsafe { read_volatile((self.base + off) as *const u32) }
    }

    #[inline]
    fn write(&self, off: usize, val: u32) {
        unsafe { write_volatile((self.base + off) as *mut u32, val) }
    }

    /// Read the root-port control/status register.
    #[inline]
    pub fn hprt0(&self) -> u32 {
        self.read(reg::HPRT0)
    }

    /// Write `HPRT0`, preserving the write-1-to-clear change bits (they are
    /// only cleared when a caller explicitly ORs them into `val`).
    #[inline]
    fn write_hprt0(&self, val: u32) {
        self.write(reg::HPRT0, val & !HPRT0_WC_BITS);
    }

    /// Perform a DWC2 core soft reset and place the core in host mode.
    ///
    /// Returns [`UsbError::ResetTimeout`] if the core never idles / releases the
    /// reset within the poll budget.
    pub fn reset_core(&self) -> Result<(), UsbError> {
        // Wait for the AHB master to go idle before resetting.
        let mut budget = POLL_BUDGET;
        while self.read(reg::GRSTCTL) & GRSTCTL_AHBIDLE == 0 {
            budget -= 1;
            if budget == 0 {
                return Err(UsbError::ResetTimeout);
            }
        }

        // Assert the core soft reset and wait for it to self-clear.
        self.write(reg::GRSTCTL, GRSTCTL_CSFTRST);
        budget = POLL_BUDGET;
        while self.read(reg::GRSTCTL) & GRSTCTL_CSFTRST != 0 {
            budget -= 1;
            if budget == 0 {
                return Err(UsbError::ResetTimeout);
            }
        }
        spin(100_000);
        Ok(())
    }

    /// Configure the core for host mode with internal DMA and power the port.
    pub fn init_host(&self) -> Result<(), UsbError> {
        self.reset_core()?;

        // Select the full-speed serial PHY and force host mode. Forcing host
        // mode avoids depending on the OTG ID pin (the USB-C port floats it).
        let mut usbcfg = self.read(reg::GUSBCFG);
        usbcfg &= !GUSBCFG_FORCE_DEV;
        usbcfg |= GUSBCFG_PHYSEL_FS | GUSBCFG_FORCE_HOST;
        self.write(reg::GUSBCFG, usbcfg);
        // The core samples the mode-force bits over several milliseconds.
        spin(300_000);

        // Enable internal DMA and unmask the global interrupt line (we poll the
        // per-channel HCINT bits, but DMA still requires GLBL_INTR_EN).
        self.write(reg::GAHBCFG, GAHBCFG_DMA_EN | GAHBCFG_GLBL_INTR_EN);

        // FIFO layout (in 32-bit words). Values follow Circle's DWC2 setup for
        // the BCM2711: RX 0x306, non-periodic TX 0x100, periodic TX 0x200.
        self.write(reg::GRXFSIZ, 0x0306);
        self.write(reg::GNPTXFSIZ, (0x0100 << 16) | 0x0306);
        self.write(reg::HPTXFSIZ, (0x0200 << 16) | 0x0406);
        self.flush_fifos()?;

        // 48MHz FS/LS PHY clock, allow FS/LS-only devices on the port.
        self.write(reg::HCFG, HCFG_FSLSPCLKSEL_48MHZ | HCFG_FSLSSUPP);

        // Confirm the core actually entered host mode.
        if self.read(reg::GINTSTS) & GINTSTS_CURMODE_HOST == 0 {
            return Err(UsbError::NotHostMode);
        }

        // Power the root port.
        let hprt0 = self.hprt0();
        self.write_hprt0(hprt0 | HPRT0_PWR);
        spin(200_000);
        Ok(())
    }

    /// Flush all TX FIFOs and the RX FIFO.
    fn flush_fifos(&self) -> Result<(), UsbError> {
        self.write(reg::GRSTCTL, GRSTCTL_TXFFLSH | GRSTCTL_TXFNUM_ALL);
        let mut budget = POLL_BUDGET;
        while self.read(reg::GRSTCTL) & GRSTCTL_TXFFLSH != 0 {
            budget -= 1;
            if budget == 0 {
                return Err(UsbError::ResetTimeout);
            }
        }
        self.write(reg::GRSTCTL, GRSTCTL_RXFFLSH);
        budget = POLL_BUDGET;
        while self.read(reg::GRSTCTL) & GRSTCTL_RXFFLSH != 0 {
            budget -= 1;
            if budget == 0 {
                return Err(UsbError::ResetTimeout);
            }
        }
        Ok(())
    }

    /// Is a device currently connected to the root port?
    #[inline]
    pub fn port_connected(&self) -> bool {
        self.hprt0() & HPRT0_CONNSTS != 0
    }

    /// Drive a USB reset on the root port and return the negotiated speed.
    ///
    /// Returns [`UsbError::NoDevice`] if nothing is connected.
    pub fn reset_port(&self) -> Result<UsbSpeed, UsbError> {
        if !self.port_connected() {
            return Err(UsbError::NoDevice);
        }

        // Acknowledge any pending connect-detect, then assert reset.
        let hprt0 = self.hprt0() & !HPRT0_WC_BITS;
        self.write(reg::HPRT0, hprt0 | HPRT0_RST);
        // USB 2.0 requires the reset be held for at least 10ms; be generous.
        spin(600_000);
        self.write(reg::HPRT0, self.hprt0() & !HPRT0_WC_BITS & !HPRT0_RST);
        // Recovery interval after reset before the port is usable.
        spin(200_000);

        let mut budget = POLL_BUDGET;
        while self.hprt0() & HPRT0_ENA == 0 {
            budget -= 1;
            if budget == 0 {
                return Err(UsbError::ResetTimeout);
            }
        }
        Ok(decode_port_speed(self.hprt0()))
    }

    /// The DMA region backing this controller's transfer buffers.
    #[inline]
    pub fn dma(&self) -> DmaRegion {
        self.dma
    }

    /// Run a single host-channel transfer to completion (or terminal failure)
    /// on channel `ch`, blocking until the channel halts or the poll budget is
    /// exhausted.
    pub fn transfer(&mut self, ch: usize, params: &ChannelParams) -> TransferResult {
        let block = reg::hc(ch);

        // Clear any stale interrupt state on the channel.
        self.write(block + reg::HCINT, HCINT_ALL);

        // Program the transfer size and DMA address.
        self.write(
            block + reg::HCTSIZ,
            encode_hctsiz(params.length, params.max_packet, params.pid),
        );
        self.write(block + reg::HCDMA, params.dma_addr);

        // For interrupt transfers, alternate the micro-frame parity so the core
        // schedules the packet in the next frame.
        let odd = if params.ep_type == EpType::Interrupt {
            self.odd_frame = !self.odd_frame;
            self.odd_frame
        } else {
            false
        };

        // Launch the channel.
        self.write(block + reg::HCCHAR, encode_hcchar(params, odd));

        // Poll HCINT until the channel halts.
        let mut budget = POLL_BUDGET;
        loop {
            let hcint = self.read(block + reg::HCINT);
            if hcint & HCINT_CHHLTD != 0 {
                let status = decode_hcint(hcint);
                // On an IN transfer the core decrements HCTSIZ.XferSize by the
                // number of bytes received.
                let remaining = self.read(block + reg::HCTSIZ) & HCTSIZ_XFERSIZE_MASK;
                let bytes = params.length.saturating_sub(remaining);
                // Acknowledge the interrupt bits we just consumed.
                self.write(block + reg::HCINT, HCINT_ALL);
                return TransferResult { status, bytes };
            }
            budget -= 1;
            if budget == 0 {
                // Try to halt the channel so it is reusable.
                self.write(block + reg::HCCHAR, HCCHAR_CHENA | HCCHAR_CHDIS);
                return TransferResult {
                    status: TransferStatus::Timeout,
                    bytes: 0,
                };
            }
        }
    }
}

/// Busy-wait for approximately `iterations` loop passes.
///
/// Uses a volatile read so the loop is not optimized away. This is a coarse
/// delay; DWC2 bring-up only needs "wait at least N milliseconds" semantics,
/// not calibrated timing.
#[inline]
fn spin(iterations: u32) {
    let mut i = 0u32;
    while i < iterations {
        unsafe {
            read_volatile(&i as *const u32);
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_count_rounds_up() {
        assert_eq!(packet_count(0, 8), 1);
        assert_eq!(packet_count(1, 8), 1);
        assert_eq!(packet_count(8, 8), 1);
        assert_eq!(packet_count(9, 8), 2);
        assert_eq!(packet_count(64, 8), 8);
        assert_eq!(packet_count(18, 64), 1);
    }

    #[test]
    fn hctsiz_packs_fields() {
        // 18-byte IN with 64-byte max packet, DATA1: 1 packet.
        let v = encode_hctsiz(18, 64, Pid::Data1);
        assert_eq!(v & HCTSIZ_XFERSIZE_MASK, 18);
        assert_eq!((v >> HCTSIZ_PKTCNT_SHIFT) & HCTSIZ_PKTCNT_MASK, 1);
        assert_eq!(v >> HCTSIZ_PID_SHIFT, 2); // DATA1 == 2

        // SETUP packet uses PID code 3.
        let s = encode_hctsiz(8, 8, Pid::Setup);
        assert_eq!(s >> HCTSIZ_PID_SHIFT, 3);
    }

    #[test]
    fn hcchar_encodes_direction_and_address() {
        let p = ChannelParams {
            dev_addr: 5,
            ep_num: 1,
            ep_dir_in: true,
            ep_type: EpType::Interrupt,
            max_packet: 8,
            low_speed: true,
            pid: Pid::Data0,
            dma_addr: 0,
            length: 8,
        };
        let v = encode_hcchar(&p, true);
        assert_eq!(v & HCCHAR_MPS_MASK, 8);
        assert_eq!((v >> HCCHAR_EPNUM_SHIFT) & 0xF, 1);
        assert_ne!(v & HCCHAR_EPDIR_IN, 0);
        assert_ne!(v & HCCHAR_LSPDDEV, 0);
        assert_eq!((v >> HCCHAR_EPTYPE_SHIFT) & 0x3, EpType::Interrupt as u32);
        assert_eq!((v >> HCCHAR_DEVADDR_SHIFT) & 0x7F, 5);
        assert_ne!(v & HCCHAR_ODDFRM, 0);
        assert_ne!(v & HCCHAR_CHENA, 0);
        assert_eq!(v & HCCHAR_CHDIS, 0);

        // A control OUT to device 0, endpoint 0: no IN/LS/ODD bits.
        let ctrl = ChannelParams {
            dev_addr: 0,
            ep_num: 0,
            ep_dir_in: false,
            ep_type: EpType::Control,
            max_packet: 64,
            low_speed: false,
            pid: Pid::Setup,
            dma_addr: 0,
            length: 8,
        };
        let cv = encode_hcchar(&ctrl, false);
        assert_eq!(cv & HCCHAR_EPDIR_IN, 0);
        assert_eq!(cv & HCCHAR_LSPDDEV, 0);
        assert_eq!(cv & HCCHAR_ODDFRM, 0);
        assert_eq!((cv >> HCCHAR_EPTYPE_SHIFT) & 0x3, EpType::Control as u32);
    }

    #[test]
    fn hcint_priority_decode() {
        assert_eq!(
            decode_hcint(HCINT_CHHLTD | HCINT_XFERCOMPL | HCINT_ACK),
            TransferStatus::Completed
        );
        assert_eq!(
            decode_hcint(HCINT_CHHLTD | HCINT_STALL),
            TransferStatus::Stall
        );
        assert_eq!(decode_hcint(HCINT_CHHLTD | HCINT_NAK), TransferStatus::Nak);
        assert_eq!(
            decode_hcint(HCINT_CHHLTD | HCINT_XACTERR),
            TransferStatus::Error
        );
        // Completion wins over a simultaneously-latched NAK.
        assert_eq!(
            decode_hcint(HCINT_CHHLTD | HCINT_XFERCOMPL | HCINT_NAK),
            TransferStatus::Completed
        );
        // Halt with nothing meaningful set is treated as an error.
        assert_eq!(decode_hcint(HCINT_CHHLTD), TransferStatus::Error);
    }

    #[test]
    fn port_speed_decode() {
        assert_eq!(decode_port_speed(0 << HPRT0_SPD_SHIFT), UsbSpeed::High);
        assert_eq!(decode_port_speed(1 << HPRT0_SPD_SHIFT), UsbSpeed::Full);
        assert_eq!(decode_port_speed(2 << HPRT0_SPD_SHIFT), UsbSpeed::Low);
    }
}
