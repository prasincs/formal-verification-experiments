//! TV Demo Application
//!
//! A demonstration application showing:
//! - Menu navigation using keyboard/remote/touch
//! - Animated content playback
//! - TV-like user interface
//!
//! # Usage
//!
//! ```no_run
//! use rpi4_spi_display::demo::{TvDemo, DemoState};
//! use rpi4_spi_display::{InputManager, RemoteOptions};
//!
//! let mut demo = TvDemo::new();
//! let mut input = InputManager::new(RemoteOptions::all());
//!
//! loop {
//!     if let Some(event) = input.poll() {
//!         demo.handle_input(event);
//!     }
//!     demo.update();
//!     demo.render(&mut framebuffer);
//! }
//! ```

pub mod animation;
pub mod menu;
pub mod tv_app;

pub use animation::{Animation, AnimationPlayer, BouncingBall, ColorCycle, Spinner};
pub use menu::{Menu, MenuItem, MenuStyle};
pub use tv_app::{TvDemo, DemoState, Screen};
