//! Serial port communication module for Raspberry Pi 4 debugging
//!
//! This module provides functionality for:
//! - Listing available serial ports (USB-to-serial adapters)
//! - Reading serial output from Raspberry Pi boot process
//! - Logging and analyzing boot messages

pub mod monitor;
pub mod port;

pub use monitor::SerialMonitor;
pub use port::{PortConfig, SerialConnection};
