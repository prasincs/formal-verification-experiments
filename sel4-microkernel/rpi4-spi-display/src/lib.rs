//! Verified SPI LCD + Touchscreen for Raspberry Pi 4
//!
//! This crate provides formally verified drivers for SPI-connected displays
//! and touch controllers, bypassing VideoCore for end-to-end verification.
//!
//! # Architecture
//!
//! ```text
//! Application
//!     │
//!     ▼
//! ┌─────────────┐    ┌─────────────┐
//! │  Display    │    │   Touch     │
//! │  (ILI9341)  │    │  (XPT2046)  │
//! └──────┬──────┘    └──────┬──────┘
//!        │                  │
//!        └────────┬─────────┘
//!                 ▼
//!          ┌─────────────┐
//!          │  SPI Driver │
//!          └──────┬──────┘
//!                 ▼
//!          ┌─────────────┐
//!          │ GPIO Driver │
//!          └──────┬──────┘
//!                 ▼
//!            BCM2711 HW
//! ```
//!
//! # Verification
//!
//! All drivers are verified with Verus to ensure:
//! - Bounds checking on all pixel operations
//! - Correct SPI protocol sequences
//! - Touch coordinates within display bounds
//! - Memory safety throughout

#![no_std]
#![allow(dead_code)]
#![allow(unused_variables)]

pub mod hal;
pub mod display;
pub mod touch;

// Re-export main types
pub use display::{Display, Framebuffer, Rgb565};
pub use touch::{TouchEvent, TouchPoint};
