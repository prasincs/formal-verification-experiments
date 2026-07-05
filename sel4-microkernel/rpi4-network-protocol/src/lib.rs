//! Shared network IPC protocol between the Network PD and client PDs.
//!
//! The existing `NetSharedMemory` layout is retained. Restart-aware generation
//! APIs are additive and live alongside the legacy TX/RX rings.

#![no_std]

pub const NET_CLIENT_CHANNEL_ID: usize = 2;
pub const MAX_PACKET_SIZE: usize = 1518;
pub const RING_SIZE: usize = 64;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetRequestType {
    Nop = 0,
    Init = 1,
    Transmit = 2,
    GetMac = 3,
    GetLinkStatus = 4,
    GetStats = 5,
    ConfigureIp = 6,
    WifiConnect = 7,
    WifiDisconnect = 8,
    WifiScan = 9,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetResponseType {
    Ok = 0,
    Error = 1,
    MacAddress = 2,
    LinkStatus = 3,
    Stats = 4,
    WifiScanResults = 5,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetEventType {
    PacketReceived = 0,
    LinkChanged = 1,
    WifiConnected = 2,
    WifiDisconnected = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NetRequestHeader {
    pub request_type: NetRequestType,
    pub request_id: u16,
    pub payload_len: u16,
    pub _reserved: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NetResponseHeader {
    pub response_type: NetResponseType,
    pub request_id: u16,
    pub payload_len: u16,
    pub error_code: u8,
}

#[repr(C)]
pub struct TxRingEntry {
    pub flags: u32,
    pub length: u16,
    pub _reserved: u16,
    pub data: [u8; MAX_PACKET_SIZE],
}

#[repr(C)]
pub struct RxRingEntry {
    pub flags: u32,
    pub length: u16,
    pub _reserved: u16,
    pub data: [u8; MAX_PACKET_SIZE],
}

pub mod ring_flags {
    /// Producer has finished the entry and transferred ownership to consumer.
    pub const VALID: u32 = 1 << 0;

    /// Reserved ABI bit. The SPSC protocol does not use this as a lock: index
    /// ownership already grants exclusive access to the producer's current
    /// free slot and the consumer's current valid slot. New code must not set
    /// `IN_USE`; it remains defined only for source/ABI compatibility.
    #[deprecated(note = "SPSC index ownership makes IN_USE unnecessary; do not set it")]
    pub const IN_USE: u32 = 1 << 1;

    pub const ERROR: u32 = 1 << 2;
}

#[repr(C)]
pub struct NetSharedMemory {
    pub tx_ring: [TxRingEntry; RING_SIZE],
    pub tx_write_idx: u32,
    pub tx_read_idx: u32,
    pub rx_ring: [RxRingEntry; RING_SIZE],
    pub rx_write_idx: u32,
    pub rx_read_idx: u32,
    pub mac_address: [u8; 6],
    pub link_up: u8,
    pub _reserved: u8,
}

impl NetSharedMemory {
    pub const fn size() -> usize {
        core::mem::size_of::<Self>()
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IpConfig {
    pub ip_addr: [u8; 4],
    pub netmask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns: [u8; 4],
    pub use_dhcp: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WifiConnectRequest {
    pub ssid: [u8; 32],
    pub ssid_len: u8,
    pub security: u8,
    pub password: [u8; 64],
    pub password_len: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LinkStatusResponse {
    pub up: u8,
    pub speed: u8,
    pub full_duplex: u8,
    pub _reserved: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsResponse {
    pub tx_packets: u64,
    pub rx_packets: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_errors: u64,
    pub rx_errors: u64,
}

impl NetRequestHeader {
    pub const fn new(request_type: NetRequestType, request_id: u16, payload_len: u16) -> Self {
        Self {
            request_type,
            request_id,
            payload_len,
            _reserved: 0,
        }
    }
}

impl NetResponseHeader {
    pub const fn ok(request_id: u16, payload_len: u16) -> Self {
        Self {
            response_type: NetResponseType::Ok,
            request_id,
            payload_len,
            error_code: 0,
        }
    }

    pub const fn error(request_id: u16, error_code: u8) -> Self {
        Self {
            response_type: NetResponseType::Error,
            request_id,
            payload_len: 0,
            error_code,
        }
    }
}

impl Default for TxRingEntry {
    fn default() -> Self {
        Self {
            flags: 0,
            length: 0,
            _reserved: 0,
            data: [0; MAX_PACKET_SIZE],
        }
    }
}

impl Default for RxRingEntry {
    fn default() -> Self {
        Self {
            flags: 0,
            length: 0,
            _reserved: 0,
            data: [0; MAX_PACKET_SIZE],
        }
    }
}

mod generation_contract;
mod generation;
pub use generation::*;

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    #[test]
    fn legacy_shared_layout_remains_available() {
        assert!(NetSharedMemory::size() > 0);
        assert_eq!(RING_SIZE, 64);
        assert_eq!(ring_flags::VALID, 1);
    }
}
