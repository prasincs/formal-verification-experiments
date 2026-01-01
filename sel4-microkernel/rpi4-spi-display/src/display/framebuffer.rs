//! RGB565 Framebuffer with Verified Operations
//!
//! Provides a bounds-checked framebuffer for the 320×240 display.

use verus_builtin::*;
use verus_builtin_macros::*;

use super::ili9341::{WIDTH, HEIGHT};

/// RGB565 color (16-bit: 5 red, 6 green, 5 blue)
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Rgb565(pub u16);

impl Rgb565 {
    pub const BLACK: Self = Self(0x0000);
    pub const WHITE: Self = Self(0xFFFF);
    pub const RED: Self = Self(0xF800);
    pub const GREEN: Self = Self(0x07E0);
    pub const BLUE: Self = Self(0x001F);

    /// Create RGB565 from RGB888 components
    #[verus_verify]
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        let r5 = (r >> 3) as u16;
        let g6 = (g >> 2) as u16;
        let b5 = (b >> 3) as u16;
        Self((r5 << 11) | (g6 << 5) | b5)
    }
}

/// Framebuffer for 320×240 RGB565 display
pub struct Framebuffer {
    buffer: [u16; (WIDTH as usize) * (HEIGHT as usize)],
}

impl Framebuffer {
    /// Create a new framebuffer initialized to black
    pub const fn new() -> Self {
        Self {
            buffer: [0; (WIDTH as usize) * (HEIGHT as usize)],
        }
    }

    /// Get pixel at coordinates
    #[verus_verify]
    pub fn get_pixel(&self, x: u16, y: u16) -> Option<Rgb565>
        ensures
            result.is_some() <==> (x < WIDTH && y < HEIGHT),
    {
        if x < WIDTH && y < HEIGHT {
            let idx = (y as usize) * (WIDTH as usize) + (x as usize);
            Some(Rgb565(self.buffer[idx]))
        } else {
            None
        }
    }

    /// Set pixel at coordinates (bounds-checked)
    #[verus_verify]
    pub fn set_pixel(&mut self, x: u16, y: u16, color: Rgb565) -> bool
        ensures
            result == (x < WIDTH && y < HEIGHT),
    {
        if x < WIDTH && y < HEIGHT {
            let idx = (y as usize) * (WIDTH as usize) + (x as usize);
            self.buffer[idx] = color.0;
            true
        } else {
            false
        }
    }

    /// Clear framebuffer to a solid color
    pub fn clear(&mut self, color: Rgb565) {
        self.buffer.fill(color.0);
    }

    /// Get raw buffer for DMA transfer
    pub fn as_slice(&self) -> &[u16] {
        &self.buffer
    }

    /// Fill a rectangle (bounds-checked)
    #[verus_verify]
    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: Rgb565) -> bool
        ensures
            result == (x + w <= WIDTH && y + h <= HEIGHT),
    {
        if x + w > WIDTH || y + h > HEIGHT {
            return false;
        }

        for row in y..(y + h) {
            for col in x..(x + w) {
                let idx = (row as usize) * (WIDTH as usize) + (col as usize);
                self.buffer[idx] = color.0;
            }
        }
        true
    }
}
