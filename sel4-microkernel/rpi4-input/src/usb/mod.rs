//! USB HID keyboard input over the Raspberry Pi 4 DWC2 host controller
//!
//! This module turns the placeholder keyboard interface in
//! [`crate::keyboard`] into a working input path: it drives the BCM2711 DWC2
//! USB OTG core ([`dwc2`]), enumerates a boot-protocol HID keyboard on the root
//! port ([`hid`]), and delivers the resulting 8-byte reports to the existing
//! [`Keyboard`](crate::keyboard::Keyboard) decoder, which produces
//! [`KeyEvent`](crate::keyboard::KeyEvent)s.
//!
//! ```no_run
//! use rpi4_input::usb::{UsbKeyboard, DmaRegion};
//!
//! // Addresses are provided by the Microkit system description.
//! let dma = DmaRegion { vaddr: 0x5_0500_0000, paddr: 0x3e60_0000, size: 0x1000 };
//! let mut kb = unsafe { UsbKeyboard::new(0x5_0700_0000, dma) };
//! let _ = kb.init();          // core + host-mode bring-up
//! loop {
//!     if let Some(event) = kb.poll() {
//!         // handle key event
//!         let _ = event;
//!     }
//! }
//! ```
//!
//! # Scope and honesty about validation
//!
//! The register map, enumeration sequence, and transfer encoding follow the
//! DWC2 databook and the Linux/Circle drivers, and the pure logic is covered by
//! host unit tests. On-hardware bring-up (real timing, a specific keyboard,
//! the Pi's internal USB hub) still requires validation on a device; nothing
//! here has been exercised against physical hardware in CI. Split transactions
//! (low-speed device behind a high-speed hub) are not implemented — the
//! keyboard is assumed to be on the root port.

pub mod dwc2;
pub mod hid;

use crate::keyboard::{KeyEvent, Keyboard};
use dwc2::{ChannelParams, Dwc2, TransferResult};
use hid::{find_boot_keyboard_endpoint, BootKeyboardEndpoint, SetupPacket};

/// A physically-contiguous, uncached DMA region for USB transfer buffers.
///
/// Mirrors the network driver's region abstraction: `vaddr` is the address the
/// CPU uses (mapped by Microkit), `paddr` is what the DWC2 DMA engine sees. The
/// region must live in the low 4GiB so its physical address fits the 32-bit
/// `HCDMA` register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaRegion {
    /// Virtual address as mapped into this protection domain.
    pub vaddr: usize,
    /// Physical (device-visible) address.
    pub paddr: usize,
    /// Size of the region in bytes.
    pub size: usize,
}

/// USB endpoint transfer type (matches the DWC2 `HCCHAR.EPType` encoding).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum EpType {
    /// Control endpoint (EP0).
    Control = 0,
    /// Isochronous endpoint.
    Isochronous = 1,
    /// Bulk endpoint.
    Bulk = 2,
    /// Interrupt endpoint (HID keyboards use this for reports).
    Interrupt = 3,
}

/// Data toggle / packet identifier for a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pid {
    /// DATA0.
    Data0,
    /// DATA1.
    Data1,
    /// DATA2 (high-bandwidth isoc; unused here).
    Data2,
    /// SETUP / MDATA.
    Setup,
}

impl Pid {
    /// Flip DATA0 ↔ DATA1 for the next transfer on a toggling endpoint.
    fn toggled(self) -> Pid {
        match self {
            Pid::Data0 => Pid::Data1,
            _ => Pid::Data0,
        }
    }
}

/// Negotiated USB bus speed on the root port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbSpeed {
    /// Low speed (1.5 Mbps) — most cheap keyboards.
    Low,
    /// Full speed (12 Mbps).
    Full,
    /// High speed (480 Mbps).
    High,
}

impl UsbSpeed {
    /// Is this a low-speed device (sets `HCCHAR.LSPDDEV`)?
    fn is_low(self) -> bool {
        matches!(self, UsbSpeed::Low)
    }
}

/// How a host-channel transfer terminated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    /// Transfer completed successfully.
    Completed,
    /// Device returned NAK/NYET (no data ready — normal for an idle keyboard).
    Nak,
    /// Endpoint stalled (protocol error / unsupported request).
    Stall,
    /// Transaction error (CRC, babble, AHB, toggle mismatch, …).
    Error,
    /// Channel never halted within the poll budget.
    Timeout,
}

/// Errors from USB bring-up and enumeration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbError {
    /// Core reset or FIFO flush never completed.
    ResetTimeout,
    /// Core did not enter host mode.
    NotHostMode,
    /// No device connected to the root port.
    NoDevice,
    /// A control transfer failed (stall/error/timeout) during enumeration.
    ControlTransferFailed,
    /// Device descriptors did not describe a boot-protocol HID keyboard.
    NotAKeyboard,
    /// The provided DMA region is too small for the transfer buffers.
    DmaTooSmall,
}

// --- DMA buffer layout (offsets within the provided DmaRegion) ---
const OFF_SETUP: usize = 0x000; // 8-byte SETUP packet
const OFF_DATA: usize = 0x040; // control-transfer data stage (descriptors)
const OFF_REPORT: usize = 0x140; // interrupt-IN boot report
const DMA_MIN_SIZE: usize = 0x200;
const DATA_CAP: usize = OFF_REPORT - OFF_DATA; // 256 bytes

/// Fixed device address assigned to the keyboard during enumeration.
const KEYBOARD_ADDRESS: u8 = 1;
/// Control endpoint (EP0) transfers use host channel 0.
const CONTROL_CHANNEL: usize = 0;
/// Interrupt-IN transfers use host channel 1.
const INTERRUPT_CHANNEL: usize = 1;
/// Control endpoints may NAK while busy; retry a bounded number of times.
const CONTROL_RETRIES: u32 = 200;
/// Poll cycles to wait before retrying enumeration after a failure.
const ENUM_BACKOFF: u32 = 64;

/// Enumeration / operational state of the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Core initialized; no device enumerated yet.
    Idle,
    /// Enumerated and in boot protocol; polling for reports.
    Running,
    /// Enumeration failed; wait `ENUM_BACKOFF` polls before retrying.
    Backoff(u32),
}

/// A USB boot-protocol HID keyboard on the DWC2 root port.
pub struct UsbKeyboard {
    hcd: Dwc2,
    decoder: Keyboard,
    state: State,
    speed: UsbSpeed,
    ep0_max_packet: u16,
    /// Address transfers are directed at while enumerating (0, then 1 after
    /// SET_ADDRESS). Distinct from the running state's fixed address so a
    /// mid-enumeration transfer targets the correct address.
    pending_addr: u8,
    endpoint: Option<BootKeyboardEndpoint>,
    int_toggle: Pid,
}

impl UsbKeyboard {
    /// Create a keyboard driver over the DWC2 MMIO window at `base` and the
    /// transfer-buffer region `dma`.
    ///
    /// The controller is not touched until [`init`](Self::init) is called.
    ///
    /// # Safety
    /// `base` must be the Microkit-mapped DWC2 register window, and `dma` must
    /// describe an uncached, physically-contiguous region mapped into this PD.
    pub unsafe fn new(base: usize, dma: DmaRegion) -> Self {
        Self {
            hcd: Dwc2::new(base, dma),
            decoder: Keyboard::with_base(base),
            state: State::Idle,
            speed: UsbSpeed::Full,
            ep0_max_packet: 8,
            pending_addr: 0,
            endpoint: None,
            int_toggle: Pid::Data0,
        }
    }

    /// Reset and configure the DWC2 core for host mode.
    ///
    /// Call once after the MMIO/DMA regions are mapped. Enumeration of an
    /// attached keyboard happens lazily on the first [`poll`](Self::poll).
    pub fn init(&mut self) -> Result<(), UsbError> {
        if self.hcd.dma().size < DMA_MIN_SIZE {
            return Err(UsbError::DmaTooSmall);
        }
        self.hcd.init_host()?;
        self.state = State::Idle;
        Ok(())
    }

    /// Poll the keyboard for the next key event.
    ///
    /// Non-blocking in steady state: enumerates on demand, then issues one
    /// interrupt-IN read per call. Returns `None` when the keyboard is idle
    /// (NAK), still enumerating, or absent.
    pub fn poll(&mut self) -> Option<KeyEvent> {
        match self.state {
            State::Backoff(0) => {
                self.state = State::Idle;
                None
            }
            State::Backoff(n) => {
                self.state = State::Backoff(n - 1);
                None
            }
            State::Idle => {
                match self.enumerate() {
                    Ok(()) => {
                        self.state = State::Running;
                        self.int_toggle = Pid::Data0;
                    }
                    Err(_) => {
                        self.state = State::Backoff(ENUM_BACKOFF);
                    }
                }
                None
            }
            State::Running => self.poll_report(),
        }
    }

    /// Current negotiated bus speed (valid once running).
    pub fn speed(&self) -> UsbSpeed {
        self.speed
    }

    /// Is a keyboard enumerated and being polled?
    pub fn is_running(&self) -> bool {
        self.state == State::Running
    }

    // --- DMA buffer helpers -------------------------------------------------

    fn dma_paddr(&self, off: usize) -> u32 {
        (self.hcd.dma().paddr + off) as u32
    }

    /// Write bytes into the DMA region (uncached; byte-wise volatile).
    fn dma_write(&self, off: usize, bytes: &[u8]) {
        let base = self.hcd.dma().vaddr + off;
        for (i, &b) in bytes.iter().enumerate() {
            unsafe { core::ptr::write_volatile((base + i) as *mut u8, b) };
        }
    }

    /// Read `len` bytes out of the DMA region into `buf`.
    fn dma_read(&self, off: usize, buf: &mut [u8]) {
        let base = self.hcd.dma().vaddr + off;
        for (i, b) in buf.iter_mut().enumerate() {
            unsafe { *b = core::ptr::read_volatile((base + i) as *const u8) };
        }
    }

    // --- Control transfers --------------------------------------------------

    /// Execute a control transfer on EP0 (SETUP → optional DATA-IN → STATUS).
    ///
    /// For an IN request, up to `in_buf.len()` bytes of the data stage are read
    /// back into `in_buf`; returns the number of bytes received. Only IN and
    /// no-data control transfers are needed for enumeration, so a data-OUT stage
    /// is not implemented.
    fn control_in(&mut self, setup: SetupPacket, in_buf: &mut [u8]) -> Result<usize, UsbError> {
        let low = self.speed.is_low();
        let mps = self.ep0_max_packet;

        // SETUP stage.
        self.dma_write(OFF_SETUP, &setup.to_bytes());
        self.run_control_stage(&ChannelParams {
            dev_addr: self.dev_addr(),
            ep_num: 0,
            ep_dir_in: false,
            ep_type: EpType::Control,
            max_packet: mps,
            low_speed: low,
            pid: Pid::Setup,
            dma_addr: self.dma_paddr(OFF_SETUP),
            length: 8,
        })?;

        let want = setup.length as usize;
        let mut received = 0usize;
        if want > 0 && (setup.request_type & hid::DIR_IN) != 0 {
            let len = want.min(in_buf.len()).min(DATA_CAP);
            // DATA stage (IN, starts at DATA1).
            let res = self.run_control_stage(&ChannelParams {
                dev_addr: self.dev_addr(),
                ep_num: 0,
                ep_dir_in: true,
                ep_type: EpType::Control,
                max_packet: mps,
                low_speed: low,
                pid: Pid::Data1,
                dma_addr: self.dma_paddr(OFF_DATA),
                length: len as u32,
            })?;
            received = (res.bytes as usize).min(len);
            self.dma_read(OFF_DATA, &mut in_buf[..received]);
        }

        // STATUS stage: opposite direction, zero length, DATA1. For an IN (or
        // no-data) request the status stage is an OUT.
        self.run_control_stage(&ChannelParams {
            dev_addr: self.dev_addr(),
            ep_num: 0,
            ep_dir_in: false,
            ep_type: EpType::Control,
            max_packet: mps,
            low_speed: low,
            pid: Pid::Data1,
            dma_addr: self.dma_paddr(OFF_DATA),
            length: 0,
        })?;

        Ok(received)
    }

    /// A no-data control transfer (SET_ADDRESS / SET_CONFIGURATION / HID
    /// class requests): SETUP → STATUS-IN.
    fn control_out_nodata(&mut self, setup: SetupPacket) -> Result<(), UsbError> {
        let low = self.speed.is_low();
        let mps = self.ep0_max_packet;

        self.dma_write(OFF_SETUP, &setup.to_bytes());
        self.run_control_stage(&ChannelParams {
            dev_addr: self.dev_addr(),
            ep_num: 0,
            ep_dir_in: false,
            ep_type: EpType::Control,
            max_packet: mps,
            low_speed: low,
            pid: Pid::Setup,
            dma_addr: self.dma_paddr(OFF_SETUP),
            length: 8,
        })?;

        // STATUS stage is an IN for a host→device request with no data.
        self.run_control_stage(&ChannelParams {
            dev_addr: self.dev_addr(),
            ep_num: 0,
            ep_dir_in: true,
            ep_type: EpType::Control,
            max_packet: mps,
            low_speed: low,
            pid: Pid::Data1,
            dma_addr: self.dma_paddr(OFF_DATA),
            length: 0,
        })?;
        Ok(())
    }

    /// Run one stage of a control transfer, retrying on NAK.
    fn run_control_stage(&mut self, params: &ChannelParams) -> Result<TransferResult, UsbError> {
        let mut tries = CONTROL_RETRIES;
        loop {
            let res = self.hcd.transfer(CONTROL_CHANNEL, params);
            match res.status {
                TransferStatus::Completed => return Ok(res),
                TransferStatus::Nak if tries > 0 => {
                    tries -= 1;
                    continue;
                }
                _ => return Err(UsbError::ControlTransferFailed),
            }
        }
    }

    /// Device address to address transfers at: 0 until SET_ADDRESS lands.
    fn dev_addr(&self) -> u8 {
        match self.state {
            State::Running => KEYBOARD_ADDRESS,
            _ => self.pending_addr,
        }
    }

    // --- Enumeration --------------------------------------------------------

    /// Enumerate the attached device and place it in boot protocol.
    fn enumerate(&mut self) -> Result<(), UsbError> {
        // Reset the root port and learn the device speed.
        self.speed = self.hcd.reset_port()?;
        self.pending_addr = 0;
        // Low/full-speed EP0 starts at 8 bytes until we read bMaxPacketSize0.
        self.ep0_max_packet = 8;

        // GET_DESCRIPTOR(device, 8) — enough to read bMaxPacketSize0 (offset 7).
        let mut dev = [0u8; 18];
        let n = self.control_in(
            SetupPacket::get_descriptor(hid::DESC_DEVICE, 0, 8),
            &mut dev[..8],
        )?;
        if n >= 8 && dev[7] != 0 {
            self.ep0_max_packet = dev[7] as u16;
        }

        // SET_ADDRESS(1).
        self.control_out_nodata(SetupPacket::set_address(KEYBOARD_ADDRESS))?;
        self.pending_addr = KEYBOARD_ADDRESS;

        // GET_DESCRIPTOR(config, 9) header to read wTotalLength.
        let mut cfg_hdr = [0u8; 9];
        self.control_in(
            SetupPacket::get_descriptor(hid::DESC_CONFIGURATION, 0, 9),
            &mut cfg_hdr,
        )?;
        let total = hid::config_total_length(&cfg_hdr).ok_or(UsbError::NotAKeyboard)?;
        let total = (total as usize).min(DATA_CAP);

        // GET_DESCRIPTOR(config, total) — full configuration with endpoints.
        let mut cfg = [0u8; DATA_CAP];
        let got = self.control_in(
            SetupPacket::get_descriptor(hid::DESC_CONFIGURATION, 0, total as u16),
            &mut cfg[..total],
        )?;

        let endpoint =
            find_boot_keyboard_endpoint(&cfg[..got.min(total)]).ok_or(UsbError::NotAKeyboard)?;

        // Byte 5 of the configuration descriptor is bConfigurationValue.
        let config_value = cfg_hdr[5];
        self.control_out_nodata(SetupPacket::set_configuration(config_value))?;

        // Enter boot protocol and disable idle reporting (report on change only).
        self.control_out_nodata(SetupPacket::set_boot_protocol(endpoint.interface))?;
        self.control_out_nodata(SetupPacket::set_idle(endpoint.interface, 0, 0))?;

        self.endpoint = Some(endpoint);
        Ok(())
    }

    // --- Interrupt-IN report polling ---------------------------------------

    /// Issue one interrupt-IN transfer and decode any boot report.
    fn poll_report(&mut self) -> Option<KeyEvent> {
        let ep = self.endpoint?;
        let mps = ep.max_packet.clamp(8, 64);
        let params = ChannelParams {
            dev_addr: KEYBOARD_ADDRESS,
            ep_num: ep.endpoint,
            ep_dir_in: true,
            ep_type: EpType::Interrupt,
            max_packet: mps,
            low_speed: self.speed.is_low(),
            pid: self.int_toggle,
            dma_addr: self.dma_paddr(OFF_REPORT),
            length: 8,
        };

        let res = self.hcd.transfer(INTERRUPT_CHANNEL, &params);
        match res.status {
            TransferStatus::Completed => {
                // Successful data phase: advance the toggle for next time.
                self.int_toggle = self.int_toggle.toggled();
                let mut report = [0u8; 8];
                self.dma_read(OFF_REPORT, &mut report);
                self.decoder.process_hid_report(&report)
            }
            // Idle keyboard NAKs; nothing to report.
            TransferStatus::Nak | TransferStatus::Timeout => None,
            // Stall or hard error: assume the device fell off; re-enumerate.
            TransferStatus::Stall | TransferStatus::Error => {
                self.state = State::Backoff(ENUM_BACKOFF);
                self.endpoint = None;
                None
            }
        }
    }
}
