//! Network drivers
//!
//! Hardware drivers for the network interfaces supported by the Network PD:
//! - `ethernet`: BCM54213PE Gigabit Ethernet (RPi4 native SoC)
//! - `wifi`: CYW43455 WiFi/Bluetooth (RPi4, SDIO)
//! - `virtio_net`: virtio-net over MMIO (QEMU virt machine, for CI testing)

#[cfg(feature = "net-ethernet")]
pub mod ethernet;

#[cfg(feature = "net-virtio")]
pub mod virtio_net;

#[cfg(feature = "net-wifi")]
pub mod wifi;

/// A physically-contiguous, uncached memory region used for DMA packet
/// buffers and (for virtio) virtqueue rings.
///
/// The region must be mapped into the protection domain (giving `vaddr`)
/// and backed by a fixed physical address (`paddr`) declared in the
/// Microkit system description. Each driver defines its own layout inside
/// the region.
#[cfg(any(feature = "net-ethernet", feature = "net-virtio"))]
#[derive(Debug, Clone, Copy)]
pub struct DmaRegion {
    /// Virtual address of the region as mapped into this PD
    pub vaddr: usize,
    /// Physical address of the region (what the device sees)
    pub paddr: usize,
    /// Size of the region in bytes
    pub size: usize,
}

/// Common error types for network drivers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    /// Hardware not found or not responding
    HardwareNotFound,
    /// Hardware initialization failed
    InitializationFailed,
    /// Invalid configuration
    InvalidConfig,
    /// Timeout waiting for hardware
    Timeout,
    /// Buffer allocation failed
    BufferAllocation,
    /// DMA buffer region has not been attached (Ethernet: call `attach_dma`)
    DmaNotAttached,
    /// Link not established
    NoLink,
    /// Firmware loading failed (WiFi only)
    FirmwareError,
    /// SDIO communication error (WiFi only)
    SdioError,
}

/// MAC address (6 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    /// Create a new MAC address
    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    /// Check if this is a broadcast address
    pub fn is_broadcast(&self) -> bool {
        self.0 == [0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
    }

    /// Check if this is a multicast address
    pub fn is_multicast(&self) -> bool {
        (self.0[0] & 0x01) != 0
    }
}

/// Link speed enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkSpeed {
    /// 10 Mbps
    Speed10,
    /// 100 Mbps
    Speed100,
    /// 1000 Mbps (Gigabit)
    Speed1000,
}

/// Link status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkStatus {
    /// Whether link is up
    pub up: bool,
    /// Current speed (if link is up)
    pub speed: Option<LinkSpeed>,
    /// Full duplex mode
    pub full_duplex: bool,
}

impl LinkStatus {
    /// Create a "link down" status
    pub const fn down() -> Self {
        Self {
            up: false,
            speed: None,
            full_duplex: false,
        }
    }
}

/// Network driver statistics
#[derive(Debug, Default, Clone, Copy)]
pub struct DriverStats {
    /// Packets transmitted
    pub tx_packets: u64,
    /// Packets received
    pub rx_packets: u64,
    /// Bytes transmitted
    pub tx_bytes: u64,
    /// Bytes received
    pub rx_bytes: u64,
    /// TX errors
    pub tx_errors: u64,
    /// RX errors
    pub rx_errors: u64,
    /// Packets dropped
    pub dropped: u64,
}

/// Common trait for network drivers
///
/// Initialization is not part of this trait because each driver requires
/// different mapped MMIO regions (see each driver's inherent `init`).
pub trait NetworkDriver {
    /// Get the MAC address
    fn mac_address(&self) -> MacAddress;

    /// Get current link status
    fn link_status(&self) -> LinkStatus;

    /// Transmit a packet
    fn transmit(&mut self, packet: &[u8]) -> Result<(), DriverError>;

    /// Receive a packet (returns number of bytes received)
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, DriverError>;

    /// Handle interrupt
    fn handle_irq(&mut self);

    /// Get driver statistics
    fn stats(&self) -> DriverStats;
}
