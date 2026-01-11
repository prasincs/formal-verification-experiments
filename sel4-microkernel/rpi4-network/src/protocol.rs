//! Network IPC Protocol
//!
//! This module defines the IPC protocol between the Network PD and client PDs.
//! It uses shared memory ring buffers for efficient zero-copy packet transfer.
//!
//! # Protocol Overview
//!
//! ```text
//! ┌────────────┐                    ┌────────────┐
//! │ Client PD  │                    │ Network PD │
//! │            │                    │            │
//! │ ┌────────┐ │   TX Ring Buffer   │ ┌────────┐ │
//! │ │ TX Buf ├─┼───────────────────►│ │ TX Buf │ │
//! │ └────────┘ │                    │ └────┬───┘ │
//! │            │                    │      │     │
//! │ ┌────────┐ │   RX Ring Buffer   │ ┌────┴───┐ │
//! │ │ RX Buf │◄┼────────────────────┼─┤ Driver │ │
//! │ └────────┘ │                    │ └────────┘ │
//! │            │                    │            │
//! │  Notify ───┼───────────────────►│            │
//! │            │◄───────────────────┼── Notify   │
//! └────────────┘                    └────────────┘
//! ```
//!
//! # Message Types
//!
//! - `NetRequest`: Client → Network PD requests
//! - `NetResponse`: Network PD → Client responses
//! - `NetEvent`: Network PD → Client async events

use crate::drivers::MacAddress;

/// Maximum packet size (MTU + headers)
pub const MAX_PACKET_SIZE: usize = 1518;

/// Ring buffer size (number of entries)
pub const RING_SIZE: usize = 64;

/// Network request types (Client → Network PD)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetRequestType {
    /// No operation
    Nop = 0,
    /// Initialize interface
    Init = 1,
    /// Transmit packet
    Transmit = 2,
    /// Get MAC address
    GetMac = 3,
    /// Get link status
    GetLinkStatus = 4,
    /// Get statistics
    GetStats = 5,
    /// Configure IP address (if IP stack is in Network PD)
    ConfigureIp = 6,
    /// Connect to WiFi network
    WifiConnect = 7,
    /// Disconnect from WiFi
    WifiDisconnect = 8,
    /// Scan for WiFi networks
    WifiScan = 9,
}

/// Network response types (Network PD → Client)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetResponseType {
    /// Success
    Ok = 0,
    /// Error occurred
    Error = 1,
    /// MAC address response
    MacAddress = 2,
    /// Link status response
    LinkStatus = 3,
    /// Statistics response
    Stats = 4,
    /// WiFi scan results
    WifiScanResults = 5,
}

/// Network event types (async notifications)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetEventType {
    /// Packet received
    PacketReceived = 0,
    /// Link state changed
    LinkChanged = 1,
    /// WiFi connected
    WifiConnected = 2,
    /// WiFi disconnected
    WifiDisconnected = 3,
}

/// Network request header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NetRequestHeader {
    /// Request type
    pub request_type: NetRequestType,
    /// Request ID (for matching responses)
    pub request_id: u16,
    /// Payload length
    pub payload_len: u16,
    /// Reserved for alignment
    pub _reserved: u8,
}

/// Network response header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NetResponseHeader {
    /// Response type
    pub response_type: NetResponseType,
    /// Request ID this is responding to
    pub request_id: u16,
    /// Payload length
    pub payload_len: u16,
    /// Error code (if response_type is Error)
    pub error_code: u8,
}

/// Ring buffer entry for TX
#[repr(C)]
pub struct TxRingEntry {
    /// Flags (valid, in-use, etc.)
    pub flags: u32,
    /// Packet length
    pub length: u16,
    /// Reserved
    pub _reserved: u16,
    /// Packet data
    pub data: [u8; MAX_PACKET_SIZE],
}

/// Ring buffer entry for RX
#[repr(C)]
pub struct RxRingEntry {
    /// Flags (valid, in-use, etc.)
    pub flags: u32,
    /// Packet length
    pub length: u16,
    /// Reserved
    pub _reserved: u16,
    /// Packet data
    pub data: [u8; MAX_PACKET_SIZE],
}

/// Ring buffer flags
pub mod ring_flags {
    /// Entry is valid and contains data
    pub const VALID: u32 = 1 << 0;
    /// Entry is being processed
    pub const IN_USE: u32 = 1 << 1;
    /// Error occurred processing this entry
    pub const ERROR: u32 = 1 << 2;
}

/// Shared memory layout for Network IPC
///
/// This structure is placed in shared memory between Network PD and clients.
#[repr(C)]
pub struct NetSharedMemory {
    /// TX ring buffer (Client → Network)
    pub tx_ring: [TxRingEntry; RING_SIZE],
    /// TX write index (updated by client)
    pub tx_write_idx: u32,
    /// TX read index (updated by Network PD)
    pub tx_read_idx: u32,

    /// RX ring buffer (Network → Client)
    pub rx_ring: [RxRingEntry; RING_SIZE],
    /// RX write index (updated by Network PD)
    pub rx_write_idx: u32,
    /// RX read index (updated by client)
    pub rx_read_idx: u32,

    /// Interface MAC address
    pub mac_address: [u8; 6],
    /// Interface is up
    pub link_up: u8,
    /// Reserved
    pub _reserved: u8,
}

impl NetSharedMemory {
    /// Calculate size of shared memory region
    pub const fn size() -> usize {
        core::mem::size_of::<Self>()
    }
}

/// IP configuration request payload
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IpConfig {
    /// IPv4 address (network byte order)
    pub ip_addr: [u8; 4],
    /// Subnet mask (network byte order)
    pub netmask: [u8; 4],
    /// Gateway address (network byte order)
    pub gateway: [u8; 4],
    /// DNS server (network byte order)
    pub dns: [u8; 4],
    /// Use DHCP
    pub use_dhcp: u8,
}

/// WiFi connection request payload
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WifiConnectRequest {
    /// SSID
    pub ssid: [u8; 32],
    /// SSID length
    pub ssid_len: u8,
    /// Security type (0=open, 1=WPA, 2=WPA2)
    pub security: u8,
    /// Password (if security != open)
    pub password: [u8; 64],
    /// Password length
    pub password_len: u8,
}

/// Link status response payload
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LinkStatusResponse {
    /// Link is up
    pub up: u8,
    /// Speed (0=10, 1=100, 2=1000 Mbps)
    pub speed: u8,
    /// Full duplex
    pub full_duplex: u8,
    /// Reserved
    pub _reserved: u8,
}

/// Statistics response payload
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsResponse {
    /// Packets transmitted
    pub tx_packets: u64,
    /// Packets received
    pub rx_packets: u64,
    /// Bytes transmitted
    pub tx_bytes: u64,
    /// Bytes received
    pub rx_bytes: u64,
    /// TX errors
    pub tx_errors: u64,
    /// RX errors
    pub rx_errors: u64,
}

/// Helper functions for working with the protocol
impl NetRequestHeader {
    /// Create a new request header
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
    /// Create a success response
    pub const fn ok(request_id: u16, payload_len: u16) -> Self {
        Self {
            response_type: NetResponseType::Ok,
            request_id,
            payload_len,
            error_code: 0,
        }
    }

    /// Create an error response
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
