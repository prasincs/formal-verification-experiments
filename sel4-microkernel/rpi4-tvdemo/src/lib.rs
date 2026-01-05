//! # Raspberry Pi 4 Demo Application
//!
//! A portable TV demo application that works with different display backends:
//! - SPI LCD (320×240, RGB565)
//! - HDMI (1280×720, ARGB)
//!
//! # Usage
//!
//! ```no_run
//! use rpi4_demo::{TvDemo, DisplayBackend, Color};
//! use rpi4_input::{InputManager, RemoteOptions};
//!
//! // With your display backend
//! let mut demo = TvDemo::new(320, 240);
//! let mut input = InputManager::new(RemoteOptions::all());
//!
//! loop {
//!     if let Some(event) = input.poll() {
//!         demo.handle_input(event);
//!     }
//!     demo.update();
//!     demo.render(&mut display);
//! }
//! ```

#![no_std]
#![allow(dead_code)]

pub mod backend;
pub mod animation;
pub mod menu;
pub mod tv_app;

pub use backend::{DisplayBackend, Color, ScaledDisplay};
pub use animation::{Animation, AnimationPlayer, AnimationType, BouncingBall, ColorCycle, Spinner};
pub use menu::{Menu, MenuItem, MenuStyle};
pub use tv_app::{TvDemo, DemoState, Screen};

// Re-export input types for convenience
pub use rpi4_input::{
    InputEvent, InputManager, InputSource, RemoteOptions,
    KeyCode, KeyEvent, KeyState, Keyboard,
    IrButton, IrEvent, IrProtocol, IrRemote,
    TouchEvent, TouchPoint,
};
