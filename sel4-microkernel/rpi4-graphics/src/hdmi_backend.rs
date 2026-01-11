//! HDMI Display Backend
//!
//! Implements the DisplayBackend trait for the HDMI framebuffer

use crate::framebuffer::Framebuffer;
use rpi4_tvdemo::backend::{Color, DisplayBackend};

/// HDMI display backend wrapper
pub struct HdmiBackend<'a> {
    fb: &'a mut Framebuffer,
}

impl<'a> HdmiBackend<'a> {
    /// Create a new HDMI backend wrapping a framebuffer
    pub fn new(fb: &'a mut Framebuffer) -> Self {
        Self { fb }
    }
}

impl<'a> DisplayBackend for HdmiBackend<'a> {
    fn width(&self) -> u32 {
        self.fb.info().width
    }

    fn height(&self) -> u32 {
        self.fb.info().height
    }

    fn set_pixel(&mut self, x: u32, y: u32, color: Color) -> bool {
        // Convert tvdemo Color to graphics Color
        let c = crate::graphics::Color::rgba(color.r, color.g, color.b, color.a);
        self.fb.put_pixel(x, y, c)
    }

    fn clear(&mut self, color: Color) {
        let c = crate::graphics::Color::rgba(color.r, color.g, color.b, color.a);
        self.fb.clear(c);
    }

    fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) -> bool {
        let c = crate::graphics::Color::rgba(color.r, color.g, color.b, color.a);
        self.fb.fill_rect(x, y, w, h, c);
        true
    }

    fn hline(&mut self, x: u32, y: u32, len: u32, color: Color) {
        let c = crate::graphics::Color::rgba(color.r, color.g, color.b, color.a);
        self.fb.hline(x, y, len, c);
    }

    fn vline(&mut self, x: u32, y: u32, len: u32, color: Color) {
        let c = crate::graphics::Color::rgba(color.r, color.g, color.b, color.a);
        self.fb.vline(x, y, len, c);
    }

    fn draw_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        let c = crate::graphics::Color::rgba(color.r, color.g, color.b, color.a);
        self.fb.draw_rect(x, y, w, h, c);
    }
}
