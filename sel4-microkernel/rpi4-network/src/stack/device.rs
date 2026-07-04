//! smoltcp device adapter over the repository's network-driver abstraction.

use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;

use crate::netif::NetworkInterface;

pub const FRAME_CAPACITY: usize = rpi4_network_protocol::MAX_PACKET_SIZE;

pub trait FrameIo {
    fn receive_frame(&mut self, buffer: &mut [u8]) -> Result<usize, ()>;
    fn transmit_frame(&mut self, packet: &[u8]) -> Result<(), ()>;
}

impl FrameIo for NetworkInterface {
    fn receive_frame(&mut self, buffer: &mut [u8]) -> Result<usize, ()> {
        self.receive(buffer).map_err(|_| ())
    }

    fn transmit_frame(&mut self, packet: &[u8]) -> Result<(), ()> {
        self.transmit(packet).map_err(|_| ())
    }
}

pub struct DriverDevice<D> {
    io: D,
    rx_buffer: [u8; FRAME_CAPACITY],
    tx_buffer: [u8; FRAME_CAPACITY],
    tx_failed: bool,
}

impl<D> DriverDevice<D> {
    pub const fn new(io: D) -> Self {
        Self {
            io,
            rx_buffer: [0; FRAME_CAPACITY],
            tx_buffer: [0; FRAME_CAPACITY],
            tx_failed: false,
        }
    }

    pub fn io_mut(&mut self) -> &mut D {
        &mut self.io
    }

    pub fn take_tx_failure(&mut self) -> bool {
        core::mem::take(&mut self.tx_failed)
    }
}

pub struct DriverRxToken<'a> {
    frame: &'a [u8],
}

impl RxToken for DriverRxToken<'_> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(self.frame)
    }
}

pub struct DriverTxToken<'a, D> {
    io: &'a mut D,
    buffer: &'a mut [u8; FRAME_CAPACITY],
    failed: &'a mut bool,
}

impl<D: FrameIo> TxToken for DriverTxToken<'_, D> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        assert!(len <= self.buffer.len(), "smoltcp frame exceeds driver MTU");
        let result = f(&mut self.buffer[..len]);
        if self.io.transmit_frame(&self.buffer[..len]).is_err() {
            *self.failed = true;
        }
        result
    }
}

impl<D: FrameIo> Device for DriverDevice<D> {
    type RxToken<'a> = DriverRxToken<'a> where Self: 'a;
    type TxToken<'a> = DriverTxToken<'a, D> where Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let Self {
            io,
            rx_buffer,
            tx_buffer,
            tx_failed,
        } = self;
        let length = io.receive_frame(rx_buffer).ok()?;
        if length == 0 || length > rx_buffer.len() {
            return None;
        }
        Some((
            DriverRxToken {
                frame: &rx_buffer[..length],
            },
            DriverTxToken {
                io,
                buffer: tx_buffer,
                failed: tx_failed,
            },
        ))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(DriverTxToken {
            io: &mut self.io,
            buffer: &mut self.tx_buffer,
            failed: &mut self.tx_failed,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = Medium::Ethernet;
        capabilities.max_transmission_unit = FRAME_CAPACITY;
        capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smoltcp::phy::{Device as _, TxToken as _};

    struct MockIo {
        rx: Option<&'static [u8]>,
        tx: [u8; FRAME_CAPACITY],
        tx_len: usize,
    }

    impl FrameIo for MockIo {
        fn receive_frame(&mut self, buffer: &mut [u8]) -> Result<usize, ()> {
            let frame = self.rx.take().ok_or(())?;
            buffer[..frame.len()].copy_from_slice(frame);
            Ok(frame.len())
        }

        fn transmit_frame(&mut self, packet: &[u8]) -> Result<(), ()> {
            self.tx[..packet.len()].copy_from_slice(packet);
            self.tx_len = packet.len();
            Ok(())
        }
    }

    #[test]
    fn receive_and_reply_share_no_packet_storage() {
        static FRAME: &[u8] = &[1, 2, 3, 4];
        let io = MockIo {
            rx: Some(FRAME),
            tx: [0; FRAME_CAPACITY],
            tx_len: 0,
        };
        let mut device = DriverDevice::new(io);
        let (rx, tx) = device.receive(Instant::from_millis(0)).unwrap();
        rx.consume(|frame| assert_eq!(frame, FRAME));
        tx.consume(3, |frame| frame.copy_from_slice(&[9, 8, 7]));
        assert_eq!(device.io_mut().tx_len, 3);
        assert_eq!(&device.io_mut().tx[..3], &[9, 8, 7]);
    }
}
