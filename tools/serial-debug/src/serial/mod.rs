//! Serial port communication module for embedded device debugging
//!
//! This module provides functionality for:
//! - Listing available serial ports (USB-to-serial adapters)
//! - Reading serial output from a device's boot process
//! - Logging and analyzing boot messages

pub mod monitor;
pub mod port;

pub use monitor::{run_monitor, MonitorConfig};
pub use port::{PortConfig, SerialConnection};
