//! Display drivers and graphics primitives
//!
//! Provides verified drivers for ILI9341-based displays and
//! a framebuffer with bounds-checked drawing operations.

pub mod ili9341;
pub mod framebuffer;

pub use ili9341::Ili9341;
pub use framebuffer::{Framebuffer, Rgb565};

/// High-level display interface
pub struct Display {
    controller: Ili9341,
    framebuffer: Framebuffer,
    dirty: bool,
}

impl Display {
    /// Display width in pixels
    pub const WIDTH: u16 = 320;
    /// Display height in pixels
    pub const HEIGHT: u16 = 240;

    /// Create a new display instance
    pub fn new(controller: Ili9341) -> Self {
        Self {
            controller,
            framebuffer: Framebuffer::new(),
            dirty: true,
        }
    }

    /// Get mutable access to the framebuffer
    pub fn framebuffer_mut(&mut self) -> &mut Framebuffer {
        self.dirty = true;
        &mut self.framebuffer
    }

    /// Refresh the display from framebuffer
    pub fn refresh(&mut self) {
        if self.dirty {
            // TODO: Send framebuffer to display
            self.dirty = false;
        }
    }

    /// Clear the display to a solid color
    pub fn clear(&mut self, color: Rgb565) {
        self.framebuffer.clear(color);
        self.dirty = true;
    }
}
