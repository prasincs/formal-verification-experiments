//! Network Protection Domain for seL4 Microkit
//!
//! This protection domain handles all network I/O for the system,
//! providing isolation between network-facing code and trusted components.
//!
//! # Compile-time Configuration
//!
//! Enable drivers and stack via Cargo features:
//! - `net-ethernet`: BCM54213PE Gigabit Ethernet
//! - `net-wifi`: CYW43455 WiFi (SDIO)
//! - `net-stack-lwip`: lwIP TCP/IP stack
//! - `net-stack-picotcp`: picoTCP stack
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │            Network PD                    │
//! │  ┌─────────────────────────────────┐   │
//! │  │         IP Stack                 │   │
//! │  │    (lwIP / picoTCP)             │   │
//! │  └─────────────┬───────────────────┘   │
//! │                │                        │
//! │  ┌─────────────┴───────────────────┐   │
//! │  │      Network Interface          │   │
//! │  │         Abstraction             │   │
//! │  └──────┬─────────────┬────────────┘   │
//! │         │             │                 │
//! │  ┌──────┴──────┐ ┌────┴─────┐         │
//! │  │  Ethernet   │ │   WiFi   │         │
//! │  │ BCM54213PE  │ │ CYW43455 │         │
//! │  └─────────────┘ └──────────┘         │
//! └─────────────────────────────────────────┘
//! ```

#![no_std]
#![no_main]

mod drivers;
mod netif;
mod protocol;

#[cfg(feature = "net-ethernet")]
use drivers::ethernet;

#[cfg(feature = "net-wifi")]
use drivers::wifi;

use core::panic::PanicInfo;

/// Microkit channel IDs
mod channels {
    /// Channel for IPC with client PDs (e.g., Graphics PD)
    pub const CLIENT_CHANNEL: u64 = 0;

    /// Channel for network IRQ notifications
    pub const NET_IRQ_CHANNEL: u64 = 1;
}

/// Network PD state
struct NetworkPd {
    /// Ethernet driver state (if enabled)
    #[cfg(feature = "net-ethernet")]
    ethernet: Option<ethernet::EthernetDriver>,

    /// WiFi driver state (if enabled)
    #[cfg(feature = "net-wifi")]
    wifi: Option<wifi::WifiDriver>,

    /// Active network interface
    active_interface: NetInterface,
}

/// Available network interfaces
#[derive(Clone, Copy, PartialEq, Eq)]
enum NetInterface {
    None,
    #[cfg(feature = "net-ethernet")]
    Ethernet,
    #[cfg(feature = "net-wifi")]
    Wifi,
}

impl NetworkPd {
    /// Create a new Network PD instance
    const fn new() -> Self {
        Self {
            #[cfg(feature = "net-ethernet")]
            ethernet: None,
            #[cfg(feature = "net-wifi")]
            wifi: None,
            active_interface: NetInterface::None,
        }
    }

    /// Initialize the network subsystem
    fn init(&mut self) {
        // Initialize Ethernet if enabled (preferred)
        #[cfg(feature = "net-ethernet")]
        {
            match ethernet::EthernetDriver::init() {
                Ok(driver) => {
                    self.ethernet = Some(driver);
                    self.active_interface = NetInterface::Ethernet;
                    // Log: Ethernet initialized
                }
                Err(_e) => {
                    // Log: Ethernet init failed
                }
            }
        }

        // Initialize WiFi if enabled and Ethernet not active
        #[cfg(feature = "net-wifi")]
        {
            if self.active_interface == NetInterface::None {
                match wifi::WifiDriver::init() {
                    Ok(driver) => {
                        self.wifi = Some(driver);
                        self.active_interface = NetInterface::Wifi;
                        // Log: WiFi initialized
                    }
                    Err(_e) => {
                        // Log: WiFi init failed
                    }
                }
            }
        }

        // Initialize IP stack
        #[cfg(feature = "net-stack-lwip")]
        {
            // TODO: Initialize lwIP with the active interface
        }

        #[cfg(feature = "net-stack-picotcp")]
        {
            // TODO: Initialize picoTCP with the active interface
        }
    }

    /// Handle incoming IRQ notification
    fn handle_irq(&mut self) {
        match self.active_interface {
            NetInterface::None => {}
            #[cfg(feature = "net-ethernet")]
            NetInterface::Ethernet => {
                if let Some(ref mut eth) = self.ethernet {
                    eth.handle_irq();
                }
            }
            #[cfg(feature = "net-wifi")]
            NetInterface::Wifi => {
                if let Some(ref mut wifi) = self.wifi {
                    wifi.handle_irq();
                }
            }
        }
    }

    /// Handle IPC message from client
    fn handle_client_message(&mut self, _badge: u64) {
        // TODO: Parse IPC message and perform network operation
        // Operations: send packet, receive packet, get status, etc.
    }
}

/// Global Network PD state
static mut NETWORK_PD: NetworkPd = NetworkPd::new();

/// Microkit entry point - called on PD initialization
#[no_mangle]
pub extern "C" fn init() {
    // Safety: Single-threaded initialization
    unsafe {
        NETWORK_PD.init();
    }
}

/// Microkit notification handler - called on channel notification
#[no_mangle]
pub extern "C" fn notified(channel: u64) {
    // Safety: Single-threaded event loop
    unsafe {
        match channel {
            channels::NET_IRQ_CHANNEL => {
                NETWORK_PD.handle_irq();
            }
            channels::CLIENT_CHANNEL => {
                NETWORK_PD.handle_client_message(channel);
            }
            _ => {
                // Unknown channel, ignore
            }
        }
    }
}

/// Panic handler
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
