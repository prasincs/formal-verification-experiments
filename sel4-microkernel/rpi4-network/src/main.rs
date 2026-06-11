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
//! # Memory map (must match tvdemo-network.system)
//!
//! | Region      | Virtual address | Physical address |
//! |-------------|-----------------|------------------|
//! | GENET regs  | 0x5_0500_0000   | 0xFD580000       |
//! | SDIO regs   | 0x5_0600_0000   | 0xFE340000       |
//! | Net ring    | 0x5_0700_0000   | (allocated)      |

#![no_std]
#![no_main]

mod drivers;
mod netif;
mod protocol;

use core::fmt;

use sel4_microkit::{debug_println, protection_domain, Channel, ChannelSet, Handler};

use netif::{NetifConfig, NetworkInterface};
use protocol::{ring_flags, NetSharedMemory, RING_SIZE};

/// GENET (Ethernet) registers, mapped by Microkit
#[cfg(feature = "net-ethernet")]
const GENET_VADDR: usize = 0x5_0500_0000;

/// SDIO registers for WiFi, mapped by Microkit
/// NOTE: requires the sdio_regs mapping in the system description
#[cfg(feature = "net-wifi")]
const SDIO_VADDR: usize = 0x5_0600_0000;

/// GPIO registers for WiFi power control (WL_ON on GPIO 41)
/// NOTE: requires a gpio_regs mapping for the network PD
#[cfg(feature = "net-wifi")]
const GPIO_VADDR: usize = 0x5_0610_0000;

/// Shared memory with client PDs, mapped by Microkit
const NET_RING_VADDR: usize = 0x5_0700_0000;

/// Channel IDs (must match tvdemo-network.system)
const NET_IRQ_CHANNEL_ID: usize = 1;
const CLIENT_CHANNEL_ID: usize = 2;

const CLIENT_CHANNEL: Channel = Channel::new(CLIENT_CHANNEL_ID);
const NET_IRQ_CHANNEL: Channel = Channel::new(NET_IRQ_CHANNEL_ID);

/// Network PD handler
struct NetworkPdHandler {
    /// Active network interface (driver abstraction)
    netif: NetworkInterface,
    /// Shared memory with the client PD
    shared: *mut NetSharedMemory,
}

impl NetworkPdHandler {
    /// Drain the TX ring: transmit every valid entry the client queued.
    ///
    /// # Safety
    /// `self.shared` must point to the Microkit-mapped shared memory region.
    unsafe fn process_tx_ring(&mut self) {
        let shared = &mut *self.shared;

        // The client PD writes tx_write_idx; read it volatilely each iteration
        while shared.tx_read_idx != core::ptr::read_volatile(&shared.tx_write_idx) {
            let idx = (shared.tx_read_idx as usize) % RING_SIZE;
            let entry = &mut shared.tx_ring[idx];

            let flags = core::ptr::read_volatile(&entry.flags);
            if flags & ring_flags::VALID != 0 {
                let len = core::ptr::read_volatile(&entry.length) as usize;
                let len = len.min(entry.data.len());
                if self.netif.transmit(&entry.data[..len]).is_err() {
                    core::ptr::write_volatile(&mut entry.flags, ring_flags::ERROR);
                } else {
                    core::ptr::write_volatile(&mut entry.flags, 0);
                }
            }

            shared.tx_read_idx = shared.tx_read_idx.wrapping_add(1);
        }
    }

    /// Pull received packets from the driver into the RX ring and
    /// notify the client if anything arrived.
    ///
    /// # Safety
    /// `self.shared` must point to the Microkit-mapped shared memory region.
    unsafe fn process_rx(&mut self) {
        let shared = &mut *self.shared;
        let mut received = false;

        loop {
            let idx = (shared.rx_write_idx as usize) % RING_SIZE;
            // Stop if the ring is full (client hasn't consumed yet).
            // The client PD writes rx_read_idx; read it volatilely.
            let rx_read = core::ptr::read_volatile(&shared.rx_read_idx);
            if shared.rx_write_idx.wrapping_sub(rx_read) as usize >= RING_SIZE {
                break;
            }
            let entry = &mut shared.rx_ring[idx];

            match self.netif.receive(&mut entry.data) {
                Ok(0) | Err(_) => break,
                Ok(len) => {
                    core::ptr::write_volatile(&mut entry.length, len as u16);
                    core::ptr::write_volatile(&mut entry.flags, ring_flags::VALID);
                    shared.rx_write_idx = shared.rx_write_idx.wrapping_add(1);
                    received = true;
                }
            }
        }

        if received {
            CLIENT_CHANNEL.notify();
        }
    }

    /// Publish interface state (MAC, link) into shared memory for clients.
    ///
    /// # Safety
    /// `self.shared` must point to the Microkit-mapped shared memory region.
    unsafe fn publish_state(&mut self) {
        let shared = &mut *self.shared;

        if let Ok(mac) = self.netif.mac_address() {
            shared.mac_address = mac.0;
        }
        shared.link_up = match self.netif.link_status() {
            Ok(status) if status.up => 1,
            _ => 0,
        };
    }
}

#[protection_domain]
fn init() -> NetworkPdHandler {
    debug_println!("");
    debug_println!("========================================");
    debug_println!("  Network Protection Domain Starting");
    debug_println!("========================================");

    let config = NetifConfig {
        #[cfg(feature = "net-ethernet")]
        ethernet_base: GENET_VADDR,
        #[cfg(feature = "net-wifi")]
        sdio_base: SDIO_VADDR,
        #[cfg(feature = "net-wifi")]
        gpio_base: GPIO_VADDR,
    };

    let mut netif = NetworkInterface::new();
    match netif.init(&config) {
        Ok(()) => debug_println!("Network PD: interface initialized"),
        Err(e) => debug_println!("Network PD: no interface available ({:?})", e),
    }

    let mut handler = NetworkPdHandler {
        netif,
        shared: NET_RING_VADDR as *mut NetSharedMemory,
    };

    // Safety: NET_RING_VADDR is mapped by the system description
    unsafe {
        handler.publish_state();
    }

    debug_println!("Network PD: ready");
    handler
}

#[derive(Debug)]
pub struct HandlerError;

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Network PD handler error")
    }
}

impl Handler for NetworkPdHandler {
    type Error = HandlerError;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if channels.contains(NET_IRQ_CHANNEL) {
            self.netif.handle_irq();
            // Safety: shared region is mapped by the system description
            unsafe {
                self.process_rx();
                self.publish_state();
            }
            NET_IRQ_CHANNEL.irq_ack().map_err(|_| HandlerError)?;
        }

        if channels.contains(CLIENT_CHANNEL) {
            // Safety: shared region is mapped by the system description
            unsafe {
                self.process_tx_ring();
            }
        }

        Ok(())
    }
}
