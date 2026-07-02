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

use rpi4_network_protocol::{ring_flags, NetSharedMemory, NET_CLIENT_CHANNEL_ID, RING_SIZE};

/// Shared memory with the Network PD (must match netdemo.system)
const NET_RING_VADDR: usize = 0x5_0700_0000;

/// Channel to the Network PD
const NET_CHANNEL: Channel = Channel::new(NET_CLIENT_CHANNEL_ID);

/// QEMU user-networking guest address (slirp default)
const GUEST_IP: [u8; 4] = [10, 0, 2, 15];
/// QEMU user-networking gateway address (slirp default)
const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];

/// Client state
struct NetClientHandler {
    shared: *mut NetSharedMemory,
    frames_seen: u32,
}

/// Build an ARP request for the QEMU gateway into `buf`; returns length
fn build_arp_request(buf: &mut [u8], mac: &[u8; 6]) -> usize {
    // Ethernet header: broadcast dst, our src, EtherType 0x0806 (ARP)
    buf[0..6].fill(0xff);
    buf[6..12].copy_from_slice(mac);
    buf[12] = 0x08;
    buf[13] = 0x06;
    // ARP: htype 1 (Ethernet), ptype 0x0800 (IPv4), hlen 6, plen 4, op 1
    buf[14] = 0x00;
    buf[15] = 0x01;
    buf[16] = 0x08;
    buf[17] = 0x00;
    buf[18] = 6;
    buf[19] = 4;
    buf[20] = 0x00;
    buf[21] = 0x01; // request
    buf[22..28].copy_from_slice(mac); // sender MAC
    buf[28..32].copy_from_slice(&GUEST_IP); // sender IP
    buf[32..38].fill(0x00); // target MAC (unknown)
    buf[38..42].copy_from_slice(&GATEWAY_IP); // target IP
    42
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

        let idx = (shared.tx_write_idx as usize) % RING_SIZE;
        let entry = &mut shared.tx_ring[idx];
        let len = build_arp_request(&mut entry.data, &mac);
        core::ptr::write_volatile(&mut entry.length, len as u16);
        core::ptr::write_volatile(&mut entry.flags, ring_flags::VALID);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        core::ptr::write_volatile(&mut shared.tx_write_idx, shared.tx_write_idx.wrapping_add(1));

        NET_CHANNEL.notify();
        debug_println!("netclient: ARP probe sent to 10.0.2.2");
    }

    /// Drain the RX ring, logging every frame the Network PD delivered.
    ///
    /// # Safety
    /// `self.shared` must point to the Microkit-mapped shared region.
    unsafe fn drain_rx(&mut self) {
        let shared = &mut *self.shared;

        // The Network PD writes rx_write_idx; read it volatilely
        while shared.rx_read_idx != core::ptr::read_volatile(&shared.rx_write_idx) {
            let idx = (shared.rx_read_idx as usize) % RING_SIZE;
            let entry = &mut shared.rx_ring[idx];

            let flags = core::ptr::read_volatile(&entry.flags);
            if flags & ring_flags::VALID != 0 {
                let len = core::ptr::read_volatile(&entry.length) as usize;
                self.frames_seen += 1;
                debug_println!("netclient: received frame ({} bytes)", len);

                // Is it the ARP reply from the gateway?
                let d = &entry.data;
                if len >= 42
                    && d[12] == 0x08
                    && d[13] == 0x06
                    && d[20] == 0x00
                    && d[21] == 0x02
                    && d[28..32] == GATEWAY_IP
                {
                    debug_println!(
                        "netclient: ARP reply from 10.0.2.2 ({:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})",
                        d[22], d[23], d[24], d[25], d[26], d[27]
                    );
                }

                // Hand the entry back to the Network PD
                core::ptr::write_volatile(&mut entry.flags, 0);
            }

            core::ptr::write_volatile(
                &mut shared.rx_read_idx,
                shared.rx_read_idx.wrapping_add(1),
            );
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
