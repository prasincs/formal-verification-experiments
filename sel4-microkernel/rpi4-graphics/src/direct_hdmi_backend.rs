//! Direct HDMI Display Backend
//!
//! Implements DisplayBackend for a pre-configured HDMI framebuffer.
//! Uses direct memory access to the framebuffer region mapped by Microkit.
//! This avoids Mailbox calls - the framebuffer is pre-configured by config.txt.

use rpi4_tvdemo::backend::{Color, DisplayBackend};

/// Framebuffer virtual address as mapped by Microkit (tvdemo.system)
const FB_VADDR: usize = 0x5_0001_0000;

/// Screen dimensions from config.txt (hdmi_mode=82 = 1920x1080)
const SCREEN_WIDTH: u32 = 1920;
const SCREEN_HEIGHT: u32 = 1080;

/// Direct HDMI backend using pre-mapped framebuffer
pub struct DirectHdmiBackend {
    width: u32,
    height: u32,
}

impl DirectHdmiBackend {
    /// Create a new direct HDMI backend
    /// Uses the framebuffer pre-configured by config.txt and mapped by Microkit
    pub fn new() -> Self {
        Self {
            width: SCREEN_WIDTH,
            height: SCREEN_HEIGHT,
        }
    }

    /// Get raw framebuffer pointer
    #[inline]
    fn fb_ptr(&self) -> *mut u32 {
        FB_VADDR as *mut u32
    }

    /// Convert Color to ARGB u32
    #[inline]
    fn color_to_argb(color: Color) -> u32 {
        ((color.a as u32) << 24)
            | ((color.r as u32) << 16)
            | ((color.g as u32) << 8)
            | (color.b as u32)
    }
}

impl Default for DirectHdmiBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayBackend for DirectHdmiBackend {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn set_pixel(&mut self, x: u32, y: u32, color: Color) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }

        let offset = y as usize * self.width as usize + x as usize;
        let argb = Self::color_to_argb(color);

        unsafe {
            self.fb_ptr().add(offset).write_volatile(argb);
        }
        true
    }

    fn clear(&mut self, color: Color) {
        let argb = Self::color_to_argb(color);
        let total = (self.width * self.height) as usize;

        unsafe {
            core::arch::asm!("dsb sy");
            let fb = self.fb_ptr();
            for i in 0..total {
                fb.add(i).write_volatile(argb);
            }
            core::arch::asm!("dsb sy");
        }
    }

    fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }

        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        let argb = Self::color_to_argb(color);

        unsafe {
            let fb = self.fb_ptr();
            for py in y..y_end {
                for px in x..x_end {
                    let offset = py as usize * self.width as usize + px as usize;
                    fb.add(offset).write_volatile(argb);
                }
            }
        }
        true
    }

    fn hline(&mut self, x: u32, y: u32, len: u32, color: Color) {
        if y >= self.height {
            return;
        }

        let x_end = (x + len).min(self.width);
        let argb = Self::color_to_argb(color);

        unsafe {
            let fb = self.fb_ptr();
            let row_offset = y as usize * self.width as usize;
            for px in x..x_end {
                fb.add(row_offset + px as usize).write_volatile(argb);
            }
        }
    }

    fn vline(&mut self, x: u32, y: u32, len: u32, color: Color) {
        if x >= self.width {
            return;
        }

        let y_end = (y + len).min(self.height);
        let argb = Self::color_to_argb(color);

        unsafe {
            let fb = self.fb_ptr();
            for py in y..y_end {
                let offset = py as usize * self.width as usize + x as usize;
                fb.add(offset).write_volatile(argb);
            }
        }
    }

    fn draw_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if w == 0 || h == 0 {
            return;
        }

        self.hline(x, y, w, color);
        self.hline(x, y + h - 1, w, color);
        self.vline(x, y, h, color);
        self.vline(x + w - 1, y, h, color);
    }
}
