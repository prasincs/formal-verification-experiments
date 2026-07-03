//! Virtio-net driver (virtio-mmio, legacy interface)
//!
//! Network driver for the virtio-net device provided by QEMU's `virt`
//! machine. This exists so the Network PD and its ring-buffer protocol can
//! be exercised in CI, where the RPi4's GENET hardware cannot be emulated.
//!
//! # Transport
//!
//! QEMU's `virt` machine provides 32 virtio-mmio transports at
//! `0x0a000000 + slot * 0x200` with interrupt `GIC SPI 16 + slot`
//! (IRQ `48 + slot`). QEMU assigns the first `-device virtio-net-device`
//! to the *last* transport, slot 31: base `0x0a003e00`, IRQ 79 (verified
//! empirically with `info qtree`). The driver still probes all slots in
//! the mapped window for a network device (DeviceID 1) so it does not
//! depend on that detail, but the IRQ number in the system description
//! must match the slot the device lands in.
//!
//! This implements the *legacy* (version 1) virtio-mmio interface, which
//! is what QEMU uses for virtio-mmio by default (`force-legacy=true`).
//! No feature bits are negotiated, so the virtio-net header is 10 bytes
//! and the device uses one receive buffer per packet.
//!
//! # DMA layout
//!
//! Virtqueues and packet buffers live in the shared [`DmaRegion`]
//! (physically addressed, mapped uncached):
//!
//! ```text
//! offset 0x00000: RX virtqueue ring (page-aligned, 8KiB reserved)
//! offset 0x02000: TX virtqueue ring (page-aligned, 8KiB reserved)
//! offset 0x04000: 128 RX buffers * 2KiB = 256KiB
//! offset 0x44000: 128 TX buffers * 2KiB = 256KiB
//! offset 0x84000: end (fits comfortably in a 1MiB region)
//! ```
//!
//! # References
//!
//! - Virtio 1.0 spec, section 4.2 (MMIO) and 2.4 (legacy virtqueue layout)
//! - Linux drivers/virtio/virtio_mmio.c

use super::{DmaRegion, DriverError, DriverStats, LinkSpeed, LinkStatus, MacAddress, NetworkDriver};

/// virtio-mmio register offsets (legacy, version 1)
#[allow(dead_code)]
mod regs {
    pub const MAGIC_VALUE: usize = 0x000; // "virt" = 0x74726976
    pub const VERSION: usize = 0x004; // 1 = legacy
    pub const DEVICE_ID: usize = 0x008; // 1 = network card
    pub const VENDOR_ID: usize = 0x00c;
    pub const HOST_FEATURES: usize = 0x010;
    pub const HOST_FEATURES_SEL: usize = 0x014;
    pub const GUEST_FEATURES: usize = 0x020;
    pub const GUEST_FEATURES_SEL: usize = 0x024;
    pub const GUEST_PAGE_SIZE: usize = 0x028; // legacy only
    pub const QUEUE_SEL: usize = 0x030;
    pub const QUEUE_NUM_MAX: usize = 0x034;
    pub const QUEUE_NUM: usize = 0x038;
    pub const QUEUE_ALIGN: usize = 0x03c; // legacy only
    pub const QUEUE_PFN: usize = 0x040; // legacy only
    pub const QUEUE_NOTIFY: usize = 0x050;
    pub const INTERRUPT_STATUS: usize = 0x060;
    pub const INTERRUPT_ACK: usize = 0x064;
    pub const STATUS: usize = 0x070;
    pub const CONFIG: usize = 0x100; // device config; net: MAC at +0

    pub const MAGIC: u32 = 0x7472_6976;
    pub const VERSION_LEGACY: u32 = 1;
    pub const DEVICE_ID_NET: u32 = 1;

    // Device status bits
    pub const STATUS_ACKNOWLEDGE: u32 = 1;
    pub const STATUS_DRIVER: u32 = 2;
    pub const STATUS_DRIVER_OK: u32 = 4;

    // Virtqueue indices for virtio-net
    pub const RX_QUEUE: u32 = 0;
    pub const TX_QUEUE: u32 = 1;

    // Descriptor flags
    pub const VRING_DESC_F_WRITE: u16 = 2;

    /// Transport stride on the QEMU virt machine
    pub const TRANSPORT_STRIDE: usize = 0x200;
    /// Legacy queue alignment / guest page size
    pub const PAGE_SIZE: usize = 4096;
    /// Queue size used by this driver (QEMU offers up to 1024)
    pub const QUEUE_SIZE: usize = 128;
    /// Packet buffer size
    pub const BUF_LENGTH: usize = 2048;
    /// Legacy virtio-net header size (no MRG_RXBUF negotiated)
    pub const NET_HDR_LEN: usize = 10;
}

// DMA region layout offsets
const RX_RING_OFF: usize = 0x0000;
const TX_RING_OFF: usize = 0x2000;
const RX_BUFS_OFF: usize = 0x4000;
const TX_BUFS_OFF: usize = RX_BUFS_OFF + regs::QUEUE_SIZE * regs::BUF_LENGTH;
const DMA_REQUIRED: usize = TX_BUFS_OFF + regs::QUEUE_SIZE * regs::BUF_LENGTH;

// Legacy vring layout (queue size N, align 4096):
//   desc:  N * 16 bytes at +0
//   avail: flags u16, idx u16, ring[N] u16, used_event u16
//   used:  (page-aligned) flags u16, idx u16, ring[N] {id u32, len u32}, avail_event u16
const fn vring_avail_off(qsize: usize) -> usize {
    qsize * 16
}
const fn vring_used_off(qsize: usize) -> usize {
    let end = vring_avail_off(qsize) + 4 + 2 * qsize + 2;
    (end + regs::PAGE_SIZE - 1) & !(regs::PAGE_SIZE - 1)
}

#[inline]
unsafe fn read16(addr: usize) -> u16 {
    core::ptr::read_volatile(addr as *const u16)
}
#[inline]
unsafe fn write16(addr: usize, v: u16) {
    core::ptr::write_volatile(addr as *mut u16, v)
}
#[inline]
unsafe fn read32(addr: usize) -> u32 {
    core::ptr::read_volatile(addr as *const u32)
}
#[inline]
unsafe fn write32(addr: usize, v: u32) {
    core::ptr::write_volatile(addr as *mut u32, v)
}
#[inline]
unsafe fn write64(addr: usize, v: u64) {
    core::ptr::write_volatile(addr as *mut u64, v)
}

#[inline]
fn fence() {
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

/// One side (RX or TX) of the virtqueue state
struct Queue {
    /// Vring base (virtual)
    ring_vaddr: usize,
    /// Actual queue size negotiated with the device (<= QUEUE_SIZE)
    qsize: u16,
    /// Next free-running available index (mirrors avail.idx)
    avail_idx: u16,
    /// Last seen used index (mirrors used.idx)
    last_used: u16,
}

impl Queue {
    fn desc_addr(&self, i: usize) -> usize {
        self.ring_vaddr + i * 16
    }
    fn avail_base(&self) -> usize {
        self.ring_vaddr + vring_avail_off(self.qsize as usize)
    }
    fn used_base(&self) -> usize {
        self.ring_vaddr + vring_used_off(self.qsize as usize)
    }

    /// Write descriptor `i`
    unsafe fn set_desc(&self, i: usize, paddr: u64, len: u32, flags: u16) {
        let d = self.desc_addr(i);
        write64(d, paddr);
        write32(d + 8, len);
        write16(d + 12, flags);
        write16(d + 14, 0); // next (unused, no chaining)
    }

    /// Publish descriptor `desc_idx` on the available ring
    unsafe fn push_avail(&mut self, desc_idx: u16) {
        let slot = (self.avail_idx % self.qsize) as usize;
        write16(self.avail_base() + 4 + 2 * slot, desc_idx);
        fence();
        self.avail_idx = self.avail_idx.wrapping_add(1);
        write16(self.avail_base() + 2, self.avail_idx);
    }

    /// Device-side used index
    unsafe fn used_idx(&self) -> u16 {
        read16(self.used_base() + 2)
    }

    /// Read used ring entry for `used_slot` -> (descriptor id, written length)
    unsafe fn used_entry(&self, used_slot: usize) -> (u32, u32) {
        let e = self.used_base() + 4 + 8 * used_slot;
        (read32(e), read32(e + 4))
    }
}

/// Virtio-net driver state
pub struct VirtioNetDriver {
    /// Transport base (virtual) of the probed device
    transport: usize,
    /// DMA region holding vrings and packet buffers
    dma: DmaRegion,
    /// Transport slot the device was found in (for diagnostics/IRQ check)
    slot: usize,
    rx: Queue,
    tx: Queue,
    mac: MacAddress,
    stats: DriverStats,
}

impl VirtioNetDriver {
    #[inline]
    fn reg_read(&self, off: usize) -> u32 {
        unsafe { read32(self.transport + off) }
    }
    #[inline]
    fn reg_write(&self, off: usize, v: u32) {
        unsafe { write32(self.transport + off, v) }
    }

    /// Probe the mapped virtio-mmio window for a legacy network device.
    ///
    /// Returns the slot index of the first virtio-net transport found.
    fn probe(scan_base: usize, scan_size: usize) -> Result<usize, DriverError> {
        let slots = scan_size / regs::TRANSPORT_STRIDE;
        for slot in 0..slots {
            let base = scan_base + slot * regs::TRANSPORT_STRIDE;
            let magic = unsafe { read32(base + regs::MAGIC_VALUE) };
            if magic != regs::MAGIC {
                continue;
            }
            let device_id = unsafe { read32(base + regs::DEVICE_ID) };
            if device_id != regs::DEVICE_ID_NET {
                continue;
            }
            let version = unsafe { read32(base + regs::VERSION) };
            if version != regs::VERSION_LEGACY {
                // Modern (version 2) interface not supported; QEMU uses
                // legacy for virtio-mmio unless force-legacy=false.
                return Err(DriverError::InitializationFailed);
            }
            return Ok(slot);
        }
        Err(DriverError::HardwareNotFound)
    }

    /// Configure one virtqueue (legacy interface) and return its state
    fn setup_queue(&self, queue: u32, ring_off: usize) -> Result<Queue, DriverError> {
        self.reg_write(regs::QUEUE_SEL, queue);
        let max = self.reg_read(regs::QUEUE_NUM_MAX);
        if max == 0 {
            return Err(DriverError::InitializationFailed);
        }
        let qsize = (max as usize).min(regs::QUEUE_SIZE) as u16;

        let ring_vaddr = self.dma.vaddr + ring_off;
        let ring_paddr = self.dma.paddr + ring_off;

        // Zero the whole reserved ring window
        unsafe {
            core::ptr::write_bytes(ring_vaddr as *mut u8, 0, TX_RING_OFF - RX_RING_OFF);
        }

        self.reg_write(regs::QUEUE_NUM, qsize as u32);
        self.reg_write(regs::QUEUE_ALIGN, regs::PAGE_SIZE as u32);
        self.reg_write(regs::QUEUE_PFN, (ring_paddr / regs::PAGE_SIZE) as u32);

        Ok(Queue {
            ring_vaddr,
            qsize,
            avail_idx: 0,
            last_used: 0,
        })
    }

    /// Initialize the driver.
    ///
    /// `scan_base`/`scan_size` describe the mapped virtio-mmio window
    /// (all transports); `dma` is the physically-addressed buffer region.
    /// Returns the driver with RX buffers posted and the device live.
    pub fn init(scan_base: usize, scan_size: usize, dma: DmaRegion) -> Result<Self, DriverError> {
        if dma.size < DMA_REQUIRED || dma.vaddr == 0 || dma.paddr == 0 {
            return Err(DriverError::InvalidConfig);
        }
        // Legacy QueuePFN is a 32-bit page frame number
        if (dma.paddr + DMA_REQUIRED) >> 12 > u32::MAX as usize {
            return Err(DriverError::InvalidConfig);
        }

        let slot = Self::probe(scan_base, scan_size)?;

        let mut driver = Self {
            transport: scan_base + slot * regs::TRANSPORT_STRIDE,
            dma,
            slot,
            rx: Queue {
                ring_vaddr: 0,
                qsize: 0,
                avail_idx: 0,
                last_used: 0,
            },
            tx: Queue {
                ring_vaddr: 0,
                qsize: 0,
                avail_idx: 0,
                last_used: 0,
            },
            mac: MacAddress::new([0; 6]),
            stats: DriverStats::default(),
        };

        // Device initialization (virtio 1.0 spec 3.1, legacy variant)
        driver.reg_write(regs::STATUS, 0); // reset
        driver.reg_write(regs::STATUS, regs::STATUS_ACKNOWLEDGE);
        driver.reg_write(regs::STATUS, regs::STATUS_ACKNOWLEDGE | regs::STATUS_DRIVER);

        // Negotiate no feature bits: 10-byte header, one buffer per packet
        driver.reg_write(regs::GUEST_FEATURES_SEL, 0);
        driver.reg_write(regs::GUEST_FEATURES, 0);

        driver.reg_write(regs::GUEST_PAGE_SIZE, regs::PAGE_SIZE as u32);

        driver.rx = driver.setup_queue(regs::RX_QUEUE, RX_RING_OFF)?;
        driver.tx = driver.setup_queue(regs::TX_QUEUE, TX_RING_OFF)?;

        // Post every RX buffer to the device
        unsafe {
            for i in 0..driver.rx.qsize {
                let paddr = (driver.dma.paddr + RX_BUFS_OFF + i as usize * regs::BUF_LENGTH) as u64;
                driver.rx.set_desc(
                    i as usize,
                    paddr,
                    regs::BUF_LENGTH as u32,
                    regs::VRING_DESC_F_WRITE,
                );
                driver.rx.push_avail(i);
            }
        }

        driver.reg_write(
            regs::STATUS,
            regs::STATUS_ACKNOWLEDGE | regs::STATUS_DRIVER | regs::STATUS_DRIVER_OK,
        );
        fence();
        driver.reg_write(regs::QUEUE_NOTIFY, regs::RX_QUEUE);

        // Read the MAC address from device config space (QEMU always
        // provides one, e.g. 52:54:00:12:34:56)
        let mut mac = [0u8; 6];
        for (i, b) in mac.iter_mut().enumerate() {
            *b = unsafe { core::ptr::read_volatile((driver.transport + regs::CONFIG + i) as *const u8) };
        }
        driver.mac = MacAddress::new(mac);

        Ok(driver)
    }

    /// Transport slot the device was found in (IRQ is `48 + slot` on the
    /// QEMU virt machine; the system description must declare that IRQ)
    pub fn slot(&self) -> usize {
        self.slot
    }
}

impl NetworkDriver for VirtioNetDriver {
    fn mac_address(&self) -> MacAddress {
        self.mac
    }

    fn link_status(&self) -> LinkStatus {
        // VIRTIO_NET_F_STATUS is not negotiated: link is always up
        LinkStatus {
            up: true,
            speed: Some(LinkSpeed::Speed1000),
            full_duplex: true,
        }
    }

    fn transmit(&mut self, packet: &[u8]) -> Result<(), DriverError> {
        if packet.is_empty() || packet.len() + regs::NET_HDR_LEN > regs::BUF_LENGTH {
            return Err(DriverError::InvalidConfig);
        }

        // Reap completed TX buffers to free ring slots
        unsafe {
            self.tx.last_used = self.tx.used_idx();
        }
        let in_flight = self.tx.avail_idx.wrapping_sub(self.tx.last_used);
        if in_flight >= self.tx.qsize {
            self.stats.dropped += 1;
            return Err(DriverError::BufferAllocation);
        }

        let slot = (self.tx.avail_idx % self.tx.qsize) as usize;
        let buf_vaddr = self.dma.vaddr + TX_BUFS_OFF + slot * regs::BUF_LENGTH;
        let buf_paddr = self.dma.paddr + TX_BUFS_OFF + slot * regs::BUF_LENGTH;

        // 10-byte legacy virtio-net header (all zero: no csum/gso) + frame
        unsafe {
            core::ptr::write_bytes(buf_vaddr as *mut u8, 0, regs::NET_HDR_LEN);
            core::ptr::copy_nonoverlapping(
                packet.as_ptr(),
                (buf_vaddr + regs::NET_HDR_LEN) as *mut u8,
                packet.len(),
            );
            self.tx.set_desc(
                slot,
                buf_paddr as u64,
                (regs::NET_HDR_LEN + packet.len()) as u32,
                0,
            );
            self.tx.push_avail(slot as u16);
        }
        fence();
        self.reg_write(regs::QUEUE_NOTIFY, regs::TX_QUEUE);

        self.stats.tx_packets += 1;
        self.stats.tx_bytes += packet.len() as u64;
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, DriverError> {
        let used = unsafe { self.rx.used_idx() };
        if used == self.rx.last_used {
            return Ok(0);
        }
        fence();

        let used_slot = (self.rx.last_used % self.rx.qsize) as usize;
        let (id, total_len) = unsafe { self.rx.used_entry(used_slot) };
        let id = id as usize % self.rx.qsize as usize;
        self.rx.last_used = self.rx.last_used.wrapping_add(1);

        let mut result = 0usize;
        let total_len = total_len as usize;
        if total_len > regs::NET_HDR_LEN {
            let len = total_len - regs::NET_HDR_LEN;
            if len > buffer.len() {
                self.stats.dropped += 1;
            } else {
                let buf_vaddr = self.dma.vaddr + RX_BUFS_OFF + id * regs::BUF_LENGTH;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        (buf_vaddr + regs::NET_HDR_LEN) as *const u8,
                        buffer.as_mut_ptr(),
                        len,
                    );
                }
                self.stats.rx_packets += 1;
                self.stats.rx_bytes += len as u64;
                result = len;
            }
        } else {
            self.stats.rx_errors += 1;
        }

        // Hand the buffer back to the device
        unsafe {
            self.rx.push_avail(id as u16);
        }
        fence();
        self.reg_write(regs::QUEUE_NOTIFY, regs::RX_QUEUE);

        Ok(result)
    }

    fn handle_irq(&mut self) {
        let status = self.reg_read(regs::INTERRUPT_STATUS);
        if status != 0 {
            self.reg_write(regs::INTERRUPT_ACK, status);
        }
    }

    fn stats(&self) -> DriverStats {
        self.stats
    }
}
