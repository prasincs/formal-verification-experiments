//! Network Interface Abstraction
//!
//! This module provides a unified interface for network operations,
//! abstracting over the underlying driver (Ethernet or WiFi).

use crate::drivers::{DriverStats, LinkStatus, MacAddress};

#[cfg(feature = "net-ethernet")]
use crate::drivers::ethernet::EthernetDriver;

#[cfg(feature = "net-wifi")]
use crate::drivers::wifi::WifiDriver;

/// Network interface error
#[derive(Debug, Clone, Copy)]
pub enum NetifError {
    /// No interface available
    NoInterface,
    /// Interface not ready
    NotReady,
    /// Transmission failed
    TransmitFailed,
    /// Reception failed
    ReceiveFailed,
    /// Buffer too small
    BufferTooSmall,
    /// Link down
    LinkDown,
}

/// Abstract network interface
pub struct NetworkInterface {
    /// Ethernet driver (if enabled)
    #[cfg(feature = "net-ethernet")]
    pub ethernet: Option<EthernetDriver>,

    /// WiFi driver (if enabled)
    #[cfg(feature = "net-wifi")]
    pub wifi: Option<WifiDriver>,

    /// Currently active interface
    active: ActiveInterface,
}

/// Which interface is currently active
#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveInterface {
    None,
    #[cfg(feature = "net-ethernet")]
    Ethernet,
    #[cfg(feature = "net-wifi")]
    Wifi,
}

impl NetworkInterface {
    /// Create a new network interface manager
    pub const fn new() -> Self {
        Self {
            #[cfg(feature = "net-ethernet")]
            ethernet: None,
            #[cfg(feature = "net-wifi")]
            wifi: None,
            active: ActiveInterface::None,
        }
    }

    /// Initialize the network interface
    ///
    /// Tries Ethernet first (if enabled), then falls back to WiFi.
    pub fn init(&mut self) -> Result<(), NetifError> {
        // Try Ethernet first (preferred)
        #[cfg(feature = "net-ethernet")]
        {
            use crate::drivers::NetworkDriver;
            match EthernetDriver::init() {
                Ok(driver) => {
                    self.ethernet = Some(driver);
                    self.active = ActiveInterface::Ethernet;
                    return Ok(());
                }
                Err(_) => {
                    // Ethernet failed, try WiFi if available
                }
            }
        }

        // Try WiFi
        #[cfg(feature = "net-wifi")]
        {
            use crate::drivers::NetworkDriver;
            match WifiDriver::init() {
                Ok(driver) => {
                    self.wifi = Some(driver);
                    self.active = ActiveInterface::Wifi;
                    return Ok(());
                }
                Err(_) => {
                    // WiFi also failed
                }
            }
        }

        Err(NetifError::NoInterface)
    }

    /// Get the MAC address of the active interface
    pub fn mac_address(&self) -> Result<MacAddress, NetifError> {
        use crate::drivers::NetworkDriver;

        match self.active {
            ActiveInterface::None => Err(NetifError::NoInterface),
            #[cfg(feature = "net-ethernet")]
            ActiveInterface::Ethernet => {
                self.ethernet.as_ref().map(|d| d.mac_address()).ok_or(NetifError::NoInterface)
            }
            #[cfg(feature = "net-wifi")]
            ActiveInterface::Wifi => {
                self.wifi.as_ref().map(|d| d.mac_address()).ok_or(NetifError::NoInterface)
            }
        }
    }

    /// Get the link status of the active interface
    pub fn link_status(&self) -> Result<LinkStatus, NetifError> {
        use crate::drivers::NetworkDriver;

        match self.active {
            ActiveInterface::None => Err(NetifError::NoInterface),
            #[cfg(feature = "net-ethernet")]
            ActiveInterface::Ethernet => {
                self.ethernet.as_ref().map(|d| d.link_status()).ok_or(NetifError::NoInterface)
            }
            #[cfg(feature = "net-wifi")]
            ActiveInterface::Wifi => {
                self.wifi.as_ref().map(|d| d.link_status()).ok_or(NetifError::NoInterface)
            }
        }
    }

    /// Transmit a packet
    pub fn transmit(&mut self, packet: &[u8]) -> Result<(), NetifError> {
        use crate::drivers::NetworkDriver;

        match self.active {
            ActiveInterface::None => Err(NetifError::NoInterface),
            #[cfg(feature = "net-ethernet")]
            ActiveInterface::Ethernet => {
                self.ethernet
                    .as_mut()
                    .ok_or(NetifError::NoInterface)?
                    .transmit(packet)
                    .map_err(|_| NetifError::TransmitFailed)
            }
            #[cfg(feature = "net-wifi")]
            ActiveInterface::Wifi => {
                self.wifi
                    .as_mut()
                    .ok_or(NetifError::NoInterface)?
                    .transmit(packet)
                    .map_err(|_| NetifError::TransmitFailed)
            }
        }
    }

    /// Receive a packet
    pub fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, NetifError> {
        use crate::drivers::NetworkDriver;

        match self.active {
            ActiveInterface::None => Err(NetifError::NoInterface),
            #[cfg(feature = "net-ethernet")]
            ActiveInterface::Ethernet => {
                self.ethernet
                    .as_mut()
                    .ok_or(NetifError::NoInterface)?
                    .receive(buffer)
                    .map_err(|_| NetifError::ReceiveFailed)
            }
            #[cfg(feature = "net-wifi")]
            ActiveInterface::Wifi => {
                self.wifi
                    .as_mut()
                    .ok_or(NetifError::NoInterface)?
                    .receive(buffer)
                    .map_err(|_| NetifError::ReceiveFailed)
            }
        }
    }

    /// Handle IRQ for the active interface
    pub fn handle_irq(&mut self) {
        use crate::drivers::NetworkDriver;

        match self.active {
            ActiveInterface::None => {}
            #[cfg(feature = "net-ethernet")]
            ActiveInterface::Ethernet => {
                if let Some(ref mut eth) = self.ethernet {
                    eth.handle_irq();
                }
            }
            #[cfg(feature = "net-wifi")]
            ActiveInterface::Wifi => {
                if let Some(ref mut wifi) = self.wifi {
                    wifi.handle_irq();
                }
            }
        }
    }

    /// Get statistics for the active interface
    pub fn stats(&self) -> Result<DriverStats, NetifError> {
        use crate::drivers::NetworkDriver;

        match self.active {
            ActiveInterface::None => Err(NetifError::NoInterface),
            #[cfg(feature = "net-ethernet")]
            ActiveInterface::Ethernet => {
                self.ethernet.as_ref().map(|d| d.stats()).ok_or(NetifError::NoInterface)
            }
            #[cfg(feature = "net-wifi")]
            ActiveInterface::Wifi => {
                self.wifi.as_ref().map(|d| d.stats()).ok_or(NetifError::NoInterface)
            }
        }
    }

    /// Check if any interface is available
    pub fn is_available(&self) -> bool {
        self.active != ActiveInterface::None
    }

    /// Check if using Ethernet
    #[cfg(feature = "net-ethernet")]
    pub fn is_ethernet(&self) -> bool {
        self.active == ActiveInterface::Ethernet
    }

    /// Check if using WiFi
    #[cfg(feature = "net-wifi")]
    pub fn is_wifi(&self) -> bool {
        self.active == ActiveInterface::Wifi
    }
}

impl Default for NetworkInterface {
    fn default() -> Self {
        Self::new()
    }
}
