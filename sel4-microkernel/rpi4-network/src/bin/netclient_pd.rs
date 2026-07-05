//! Network Client Protection Domain (QEMU netdemo)
//!
//! Minimal client that exercises the Network PD's shared-memory ring
//! protocol end to end, for CI boot testing on the QEMU virt machine:
//!
//! 1. At init, reads the interface MAC from the shared header (published
//!    by the higher-priority Network PD, which initializes first) and
//!    queues an ARP request for QEMU's user-network gateway (10.0.2.2)
//!    on the TX ring.
//! 2. QEMU's slirp backend answers the ARP request; the Network PD pulls
//!    the reply from the driver into the RX ring and notifies us.
//! 3. On notification, drains the RX ring and logs each frame — the CI
//!    boot test greps these lines.
//!
//! Together with the Network PD this covers: client -> TX ring -> driver
//! -> QEMU -> driver -> RX ring -> client.

#![no_std]
#![no_main]

use core::fmt;

use sel4_microkit::{debug_println, protection_domain, Channel, ChannelSet, Handler};

use rpi4_network_protocol::arp;
use rpi4_network_protocol::proof::{consumer_permit, producer_permit, slot_for};
use rpi4_network_protocol::{ring_flags, NetSharedMemory, NET_CLIENT_CHANNEL_ID};

/// Shared memory with the Network PD (must match netdemo.system)
const NET_RING_VADDR: usize = 0x5_0700_0000;

/// Channel to the Network PD
const NET_CHANNEL: Channel = Channel::new(NET_CLIENT_CHANNEL_ID);

/// QEMU user-mode networking (slirp) topology. QEMU fixes these addresses
/// for a default `-netdev user` and the CI harness relies on them: the boot
/// test in qemu-mockpi.yml greps the log for the gateway's ARP reply. Change
/// them only together with that workflow. Clients that need a discovered
/// (rather than fixture) configuration should use DHCP like `ipdemo_pd`.
const GUEST_IP: [u8; 4] = [10, 0, 2, 15];
const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];

/// Client state
struct NetClientHandler {
    shared: *mut NetSharedMemory,
    frames_seen: u32,
}

impl NetClientHandler {
    /// Queue an ARP request on the TX ring and notify the Network PD.
    ///
    /// # Safety
    /// `self.shared` must point to the Microkit-mapped shared region.
    unsafe fn send_arp_probe(&mut self) {
        let shared = &mut *self.shared;

        let mac = core::ptr::read_volatile(&shared.mac_address);
        let link_up = core::ptr::read_volatile(&shared.link_up) != 0;
        debug_println!(
            "netclient: link_up={}, mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            link_up,
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5]
        );

        // The Network PD writes tx_read_idx; read it volatilely. The permit
        // refuses a full ring and an entry the consumer has not released.
        let write = shared.tx_write_idx;
        let read = core::ptr::read_volatile(&shared.tx_read_idx);
        let probe_slot = slot_for(write);
        let flags = core::ptr::read_volatile(&shared.tx_ring[probe_slot].flags);
        let permit = match producer_permit(write, read, flags) {
            Ok(permit) => permit,
            Err(err) => {
                debug_println!("netclient: TX ring refused ARP probe: {:?}", err);
                return;
            }
        };

        let entry = &mut shared.tx_ring[permit.slot()];
        let len = arp::build_request(&mut entry.data, &mac, &GUEST_IP, &GATEWAY_IP);
        core::ptr::write_volatile(&mut entry.length, len as u16);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        core::ptr::write_volatile(&mut entry.flags, ring_flags::VALID);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        core::ptr::write_volatile(&mut shared.tx_write_idx, write.wrapping_add(1));

        NET_CHANNEL.notify();
        debug_println!("netclient: ARP probe sent to 10.0.2.2");
    }

    /// Drain the RX ring, logging every frame the Network PD delivered.
    ///
    /// # Safety
    /// `self.shared` must point to the Microkit-mapped shared region.
    unsafe fn drain_rx(&mut self) {
        let shared = &mut *self.shared;

        loop {
            // The Network PD writes rx_write_idx; read it volatilely
            let write = core::ptr::read_volatile(&shared.rx_write_idx);
            let read = shared.rx_read_idx;
            let next_slot = slot_for(read);
            let flags = core::ptr::read_volatile(&shared.rx_ring[next_slot].flags);

            // Empty ring ends the drain. An unpublished entry inside the
            // occupied window is a protocol violation by the producer; stop
            // rather than touch an entry we do not own.
            let permit = match consumer_permit(write, read, flags) {
                Ok(permit) => permit,
                Err(_) => break,
            };

            let entry = &mut shared.rx_ring[permit.slot()];
            let len = core::ptr::read_volatile(&entry.length) as usize;
            let len = len.min(entry.data.len());
            self.frames_seen += 1;
            debug_println!("netclient: received frame ({} bytes)", len);

            // Is it the ARP reply from the gateway?
            if let Some(reply) = arp::parse_reply(&entry.data[..len]) {
                if reply.sender_ip == GATEWAY_IP {
                    let m = reply.sender_mac;
                    debug_println!(
                        "netclient: ARP reply from {}.{}.{}.{} ({:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})",
                        GATEWAY_IP[0], GATEWAY_IP[1], GATEWAY_IP[2], GATEWAY_IP[3],
                        m[0], m[1], m[2], m[3], m[4], m[5]
                    );
                }
            }

            // Hand the entry back to the Network PD before publishing the
            // new read index.
            core::ptr::write_volatile(&mut entry.flags, 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            core::ptr::write_volatile(&mut shared.rx_read_idx, read.wrapping_add(1));
        }
    }
}

#[protection_domain]
fn init() -> NetClientHandler {
    debug_println!("");
    debug_println!("netclient: starting");

    let mut handler = NetClientHandler {
        shared: NET_RING_VADDR as *mut NetSharedMemory,
        frames_seen: 0,
    };

    // Safety: NET_RING_VADDR is mapped by netdemo.system. The Network PD
    // runs at higher priority, so its init (which publishes the MAC and
    // initializes the driver) has already completed.
    unsafe {
        handler.send_arp_probe();
    }

    debug_println!("netclient: ready");
    handler
}

#[derive(Debug)]
pub struct HandlerError;

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "netclient handler error")
    }
}

impl Handler for NetClientHandler {
    type Error = HandlerError;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if channels.contains(NET_CHANNEL) {
            // Safety: shared region is mapped by the system description
            unsafe {
                self.drain_rx();
            }
        }
        Ok(())
    }
}
