//! Verified SPI LCD + Touchscreen for Raspberry Pi 4
//!
//! This crate provides formally verified drivers for SPI-connected displays,
//! touch controllers, and remote input devices, bypassing VideoCore for
//! end-to-end verification.
//!
//! # Architecture
//!
//! ```text
//! Application (TV Demo)
//!     │
//!     ▼
//! ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
//! │  Display    │  │   Touch     │  │  Keyboard   │  │  IR Remote  │
//! │  (ILI9341)  │  │  (XPT2046)  │  │  (USB HID)  │  │   (NEC)     │
//! └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘
//!        │                │                │                │
//!        └────────┬───────┴────────────────┴────────────────┘
//!                 ▼
//!          ┌─────────────┐
//!          │  SPI/GPIO   │
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
//!
//! # Remote Input Options
//!
//! The crate supports multiple input sources:
//! - **Touch**: XPT2046 resistive touchscreen
//! - **Keyboard**: USB HID keyboard for navigation/media control
//! - **IR Remote**: Infrared remote (NEC, RC5, RC6 protocols)
//!
//! # TV Demo
//!
//! Includes a demo application with:
//! - Navigable menu system
//! - Multiple animations (bouncing ball, color cycle, spinner)
//! - Support for keyboard, IR remote, and touch input

#![no_std]
#![allow(dead_code)]
#![allow(unused_variables)]

pub mod hal;
pub mod display;
pub mod touch;
pub mod input;
pub mod demo;

// Re-export main types
pub use display::{Display, Framebuffer, Rgb565};
pub use touch::{TouchEvent, TouchPoint};
pub use input::{
    InputEvent, InputManager, InputSource, RemoteOptions,
    KeyCode, KeyEvent, KeyState, Keyboard,
    IrButton, IrEvent, IrProtocol, IrRemote,
};
pub use demo::{TvDemo, DemoState, Screen, Animation, AnimationPlayer};
