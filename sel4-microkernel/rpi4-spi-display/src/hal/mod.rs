//! Hardware Abstraction Layer for BCM2711
//!
//! Provides verified drivers for:
//! - SPI0 peripheral
//! - GPIO pin control

pub mod gpio;
pub mod spi;

pub use gpio::{Gpio, Pin, PinMode};
pub use spi::{Spi, SpiConfig, ChipSelect};
