//! Touch controller drivers
//!
//! Provides verified drivers for resistive touch controllers.

pub mod xpt2046;

pub use xpt2046::Xpt2046;

/// Touch point with screen coordinates
#[derive(Clone, Copy, Debug)]
pub struct TouchPoint {
    /// X coordinate (0-319)
    pub x: u16,
    /// Y coordinate (0-239)
    pub y: u16,
    /// Pressure (0 = no touch, higher = more pressure)
    pub pressure: u16,
}

/// Touch event types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchEvent {
    /// Finger touched the screen
    Down(TouchPoint),
    /// Finger moved while touching
    Move(TouchPoint),
    /// Finger lifted from screen
    Up,
}

/// Touch controller trait
pub trait TouchController {
    /// Check if screen is currently being touched
    fn is_touched(&self) -> bool;

    /// Read current touch point (if touched)
    fn read_point(&mut self) -> Option<TouchPoint>;

    /// Poll for touch events
    fn poll_event(&mut self) -> Option<TouchEvent>;
}
