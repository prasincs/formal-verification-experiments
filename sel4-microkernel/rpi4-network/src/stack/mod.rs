//! Minimal no_std IPv4 stack for the QEMU network milestone.

mod device;

pub use device::{DriverDevice, FrameIo};

use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet, SocketStorage};
use smoltcp::phy::Device;
use smoltcp::socket::{dhcpv4, icmp};
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, Icmpv4Packet, Icmpv4Repr, IpAddress, IpCidr, Ipv4Address, Ipv4Cidr,
};

const PING_IDENT: u16 = 0x5341;
const PING_SEQUENCE: u16 = 1;
const PING_TARGET: Ipv4Address = Ipv4Address::new(10, 0, 2, 2);
const PING_PAYLOAD: &[u8] = b"SAOSPING";

pub struct StackResources<'a> {
    pub sockets: &'a mut [SocketStorage<'a>],
    pub icmp_rx_metadata: &'a mut [icmp::PacketMetadata],
    pub icmp_rx_payload: &'a mut [u8],
    pub icmp_tx_metadata: &'a mut [icmp::PacketMetadata],
    pub icmp_tx_payload: &'a mut [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackEvent {
    DhcpConfigured(Ipv4Cidr),
    DhcpDeconfigured,
    PingSent,
    PingReply,
    TransmitError,
}

pub struct NetworkStack<'a, D: FrameIo> {
    iface: Interface,
    device: DriverDevice<D>,
    sockets: SocketSet<'a>,
    dhcp_handle: SocketHandle,
    icmp_handle: SocketHandle,
    configured: bool,
    ping_sent: bool,
    ping_reply: bool,
}

impl<'a, D: FrameIo> NetworkStack<'a, D> {
    pub fn new(
        io: D,
        mac: [u8; 6],
        resources: StackResources<'a>,
        now: Instant,
    ) -> Self {
        let mut device = DriverDevice::new(io);
        let mut config = Config::new(EthernetAddress(mac).into());
        config.random_seed = u64::from_le_bytes([
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], 0x53, 0x41,
        ]);
        let iface = Interface::new(config, &mut device, now);

        let mut sockets = SocketSet::new(resources.sockets);
        let dhcp_handle = sockets.add(dhcpv4::Socket::new());
        let icmp_rx = icmp::PacketBuffer::new(
            resources.icmp_rx_metadata,
            resources.icmp_rx_payload,
        );
        let icmp_tx = icmp::PacketBuffer::new(
            resources.icmp_tx_metadata,
            resources.icmp_tx_payload,
        );
        let mut icmp_socket = icmp::Socket::new(icmp_rx, icmp_tx);
        icmp_socket
            .bind(icmp::Endpoint::Ident(PING_IDENT))
            .expect("fixed ICMP endpoint is valid");
        let icmp_handle = sockets.add(icmp_socket);

        Self {
            iface,
            device,
            sockets,
            dhcp_handle,
            icmp_handle,
            configured: false,
            ping_sent: false,
            ping_reply: false,
        }
    }

    pub fn io_mut(&mut self) -> &mut D {
        self.device.io_mut()
    }

    pub fn poll(&mut self, now: Instant) -> Option<StackEvent> {
        let _ = self.iface.poll(now, &mut self.device, &mut self.sockets);

        let mut event = self.poll_dhcp();
        if self.configured && !self.ping_sent {
            if self.queue_ping() {
                self.ping_sent = true;
                event = event.or(Some(StackEvent::PingSent));
                let _ = self.iface.poll(now, &mut self.device, &mut self.sockets);
            }
        }

        if !self.ping_reply && self.take_ping_reply() {
            self.ping_reply = true;
            event = Some(StackEvent::PingReply);
        }

        if self.device.take_tx_failure() {
            event = Some(StackEvent::TransmitError);
        }
        event
    }

    fn poll_dhcp(&mut self) -> Option<StackEvent> {
        match self
            .sockets
            .get_mut::<dhcpv4::Socket>(self.dhcp_handle)
            .poll()
        {
            None => None,
            Some(dhcpv4::Event::Configured(config)) => {
                let address = config.address;
                self.iface.update_ip_addrs(|addresses| {
                    addresses.clear();
                    addresses
                        .push(IpCidr::Ipv4(address))
                        .expect("one IPv4 address fits fixed storage");
                });
                if let Some(router) = config.router {
                    let _ = self.iface.routes_mut().add_default_ipv4_route(router);
                } else {
                    self.iface.routes_mut().remove_default_ipv4_route();
                }
                self.configured = true;
                self.ping_sent = false;
                self.ping_reply = false;
                Some(StackEvent::DhcpConfigured(address))
            }
            Some(dhcpv4::Event::Deconfigured) => {
                self.iface.update_ip_addrs(|addresses| addresses.clear());
                self.iface.routes_mut().remove_default_ipv4_route();
                self.configured = false;
                self.ping_sent = false;
                self.ping_reply = false;
                Some(StackEvent::DhcpDeconfigured)
            }
        }
    }

    fn queue_ping(&mut self) -> bool {
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp_handle);
        if !socket.can_send() {
            return false;
        }
        let repr = Icmpv4Repr::EchoRequest {
            ident: PING_IDENT,
            seq_no: PING_SEQUENCE,
            data: PING_PAYLOAD,
        };
        let Ok(payload) = socket.send(repr.buffer_len(), IpAddress::Ipv4(PING_TARGET)) else {
            return false;
        };
        let mut packet = Icmpv4Packet::new_unchecked(payload);
        repr.emit(&mut packet, &self.device.capabilities().checksum);
        true
    }

    fn take_ping_reply(&mut self) -> bool {
        let socket = self.sockets.get_mut::<icmp::Socket>(self.icmp_handle);
        while socket.can_recv() {
            let Ok((payload, source)) = socket.recv() else {
                return false;
            };
            if source != IpAddress::Ipv4(PING_TARGET) {
                continue;
            }
            let Ok(packet) = Icmpv4Packet::new_checked(payload) else {
                continue;
            };
            let Ok(repr) = Icmpv4Repr::parse(&packet, &self.device.capabilities().checksum) else {
                continue;
            };
            if matches!(
                repr,
                Icmpv4Repr::EchoReply {
                    ident: PING_IDENT,
                    seq_no: PING_SEQUENCE,
                    data: PING_PAYLOAD,
                }
            ) {
                return true;
            }
        }
        false
    }
}
