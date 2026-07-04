#![no_std]
#![no_main]

#[path = "../drivers/mod.rs"]
mod drivers;
#[path = "../netif.rs"]
mod netif;
#[path = "../stack/mod.rs"]
mod stack;
#[path = "../time.rs"]
mod time;

use core::fmt;

use netif::{NetifConfig, NetworkInterface};
use sel4_microkit::{debug_println, protection_domain, Channel, ChannelSet, Handler};
use smoltcp::iface::SocketStorage;
use smoltcp::socket::icmp;
use stack::{NetworkStack, StackEvent, StackResources};

const VIRTIO_MMIO_VADDR: usize = 0x5_0900_0000;
const VIRTIO_MMIO_SIZE: usize = 0x4000;
const VIRTIO_DMA_VADDR: usize = 0x5_0800_0000;
const VIRTIO_DMA_PADDR: usize = 0x5000_0000;
const VIRTIO_DMA_SIZE: usize = 0x10_0000;
const NET_IRQ_CHANNEL_ID: usize = 1;
const NET_IRQ_CHANNEL: Channel = Channel::new(NET_IRQ_CHANNEL_ID);

static mut SOCKET_STORAGE: [SocketStorage<'static>; 2] =
    [SocketStorage::EMPTY, SocketStorage::EMPTY];
static mut ICMP_RX_METADATA: [icmp::PacketMetadata; 1] = [icmp::PacketMetadata::EMPTY];
static mut ICMP_TX_METADATA: [icmp::PacketMetadata; 1] = [icmp::PacketMetadata::EMPTY];
static mut ICMP_RX_PAYLOAD: [u8; 128] = [0; 128];
static mut ICMP_TX_PAYLOAD: [u8; 128] = [0; 128];

struct IpDemoHandler {
    stack: NetworkStack<'static, NetworkInterface>,
}

impl IpDemoHandler {
    fn log_event(event: StackEvent) {
        match event {
            StackEvent::DhcpConfigured(address) => {
                debug_println!("DHCP OK {}", address);
            }
            StackEvent::DhcpDeconfigured => debug_println!("DHCP LOST"),
            StackEvent::PingSent => debug_println!("PING SENT 10.0.2.2"),
            StackEvent::PingReply => debug_println!("PING OK"),
            StackEvent::TransmitError => debug_println!("NETWORK TX ERROR"),
        }
    }

    fn poll(&mut self) {
        // More than one logical event can become ready after one virtio IRQ
        // (for example DHCP configuration followed by the queued ping).
        for _ in 0..4 {
            let Some(event) = self.stack.poll(time::instant()) else {
                break;
            };
            Self::log_event(event);
        }
    }
}

#[protection_domain]
fn init() -> IpDemoHandler {
    debug_println!("IPDEMO START");

    let config = NetifConfig {
        virtio_scan_base: VIRTIO_MMIO_VADDR,
        virtio_scan_size: VIRTIO_MMIO_SIZE,
        virtio_dma: drivers::DmaRegion {
            vaddr: VIRTIO_DMA_VADDR,
            paddr: VIRTIO_DMA_PADDR,
            size: VIRTIO_DMA_SIZE,
        },
    };

    let mut netif = NetworkInterface::new();
    netif.init(&config).expect("virtio-net must initialize in ipdemo");
    let mac = netif
        .mac_address()
        .expect("virtio-net must expose a MAC address")
        .0;

    let resources = unsafe {
        StackResources {
            sockets: &mut *core::ptr::addr_of_mut!(SOCKET_STORAGE),
            icmp_rx_metadata: &mut *core::ptr::addr_of_mut!(ICMP_RX_METADATA),
            icmp_rx_payload: &mut *core::ptr::addr_of_mut!(ICMP_RX_PAYLOAD),
            icmp_tx_metadata: &mut *core::ptr::addr_of_mut!(ICMP_TX_METADATA),
            icmp_tx_payload: &mut *core::ptr::addr_of_mut!(ICMP_TX_PAYLOAD),
        }
    };

    let stack = NetworkStack::new(netif, mac, resources, time::instant());
    let mut handler = IpDemoHandler { stack };
    // The first poll emits DHCP DISCOVER; later virtio interrupts drive the
    // offer/ack and ICMP exchange.
    handler.poll();
    handler
}

#[derive(Debug)]
struct HandlerError;

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ipdemo handler error")
    }
}

impl Handler for IpDemoHandler {
    type Error = HandlerError;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if channels.contains(NET_IRQ_CHANNEL) {
            self.stack.io_mut().handle_irq();
            self.poll();
            NET_IRQ_CHANNEL.irq_ack().map_err(|_| HandlerError)?;
        }
        Ok(())
    }
}
