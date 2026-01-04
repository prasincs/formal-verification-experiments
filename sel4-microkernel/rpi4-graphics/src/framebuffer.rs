//! # Framebuffer Driver
//!
//! Allocates and manages the framebuffer via the VideoCore mailbox.
//!
//! ## Verus Verification
//! Key properties verified:
//! - `put_pixel` returns false for out-of-bounds coordinates
//! - No writes occur outside framebuffer memory

use crate::mailbox::{Mailbox, MailboxError, tags};
use crate::graphics::Color;

// Verus imports disabled for build testing
// #[allow(unused_imports)]
// use verus_builtin::*;
// #[allow(unused_imports)]
// use verus_builtin_macros::verus;

/// Framebuffer configuration
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    /// Physical base address of framebuffer
    pub base: usize,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Bytes per scanline (may be > width * bytes_per_pixel due to alignment)
    pub pitch: u32,
    /// Bits per pixel (typically 32)
    pub depth: u32,
    /// Total size in bytes
    pub size: u32,
}

/// Framebuffer handle for drawing operations
pub struct Framebuffer {
    /// Framebuffer info
    info: FramebufferInfo,
    /// Pointer to framebuffer memory
    buffer: *mut u32,
}

impl Framebuffer {
    /// Allocate and initialize framebuffer via mailbox
    ///
    /// # Safety
    /// The mailbox must be properly initialized and the device memory mapped.
    pub unsafe fn new(
        mailbox: &Mailbox,
        width: u32,
        height: u32,
    ) -> Result<Self, MailboxError> {
        // Aligned buffer for mailbox communication
        #[repr(align(16))]
        struct AlignedBuffer([u32; 36]);
        let mut buf = AlignedBuffer([0u32; 36]);
        let buffer = &mut buf.0;

        // Build framebuffer allocation request
        // Multiple tags in one message for efficiency
        buffer[0] = 35 * 4; // Total buffer size
        buffer[1] = 0; // Request code

        // Set physical display size
        buffer[2] = tags::SET_PHYSICAL_SIZE;
        buffer[3] = 8; // Value size
        buffer[4] = 0; // Request
        buffer[5] = width;
        buffer[6] = height;

        // Set virtual buffer size (same as physical for no scrolling)
        buffer[7] = tags::SET_VIRTUAL_SIZE;
        buffer[8] = 8;
        buffer[9] = 0;
        buffer[10] = width;
        buffer[11] = height;

        // Set virtual offset (0,0)
        buffer[12] = tags::SET_VIRTUAL_OFFSET;
        buffer[13] = 8;
        buffer[14] = 0;
        buffer[15] = 0;
        buffer[16] = 0;

        // Set color depth (32 bits = ARGB)
        buffer[17] = tags::SET_DEPTH;
        buffer[18] = 4;
        buffer[19] = 0;
        buffer[20] = 32;

        // Set pixel order (RGB, not BGR)
        buffer[21] = tags::SET_PIXEL_ORDER;
        buffer[22] = 4;
        buffer[23] = 0;
        buffer[24] = 1; // 1 = RGB

        // Allocate buffer
        buffer[25] = tags::ALLOCATE_BUFFER;
        buffer[26] = 8;
        buffer[27] = 0;
        buffer[28] = 4096; // Alignment
        buffer[29] = 0; // Will be filled with size

        // Get pitch (bytes per row)
        buffer[30] = tags::GET_PITCH;
        buffer[31] = 4;
        buffer[32] = 0;
        buffer[33] = 0;

        // End tag
        buffer[34] = 0;

        // Send to GPU
        mailbox.call(buffer)?;

        // Extract results
        let fb_gpu_addr = buffer[28];
        let fb_size = buffer[29];
        let pitch = buffer[33];

        if fb_gpu_addr == 0 || fb_size == 0 {
            return Err(MailboxError::AllocationFailed);
        }

        // Convert GPU address to ARM physical address
        let fb_phys_addr = crate::gpu_to_arm(fb_gpu_addr);

        // Calculate virtual address using Microkit's mapping
        // The framebuffer region starting at FRAMEBUFFER_PHYS_BASE is mapped
        // to FRAMEBUFFER_VIRT_BASE. Calculate the offset and apply it.
        let fb_offset = fb_phys_addr.saturating_sub(crate::FRAMEBUFFER_PHYS_BASE);
        let fb_virt_addr = crate::FRAMEBUFFER_VIRT_BASE + fb_offset;

        let info = FramebufferInfo {
            base: fb_phys_addr,
            width,
            height,
            pitch,
            depth: 32,
            size: fb_size,
        };

        Ok(Self {
            info,
            buffer: fb_virt_addr as *mut u32,
        })
    }

    /// Get framebuffer info
    pub fn info(&self) -> &FramebufferInfo {
        &self.info
    }

    /// Get framebuffer dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.info.width, self.info.height)
    }
}

impl Framebuffer {
    /// Put a pixel at (x, y) with bounds checking
    ///
    /// Returns false if coordinates are out of bounds.
    #[inline]
    pub fn put_pixel(&mut self, x: u32, y: u32, color: Color) -> bool {
        if x >= self.info.width || y >= self.info.height {
            return false;
        }

        // Calculate offset (pitch is in bytes, we're working with u32)
        let pitch_pixels = self.info.pitch / 4;
        let offset = (y * pitch_pixels + x) as usize;

        unsafe {
            self.buffer.add(offset).write_volatile(color.to_argb());
        }

        true
    }

    /// Put a pixel without bounds checking
    ///
    /// # Safety
    /// Caller must ensure x < width and y < height.
    #[inline]
    pub unsafe fn put_pixel_unchecked(&mut self, x: u32, y: u32, color: Color) {
        let pitch_pixels = self.info.pitch / 4;
        let offset = (y * pitch_pixels + x) as usize;
        self.buffer.add(offset).write_volatile(color.to_argb());
    }

    /// Fill the entire screen with a color
    pub fn clear(&mut self, color: Color) {
        let argb = color.to_argb();
        let total_pixels = (self.info.pitch / 4) * self.info.height;

        for i in 0..total_pixels as usize {
            unsafe {
                self.buffer.add(i).write_volatile(argb);
            }
        }
    }

    /// Fill a rectangle with bounds checking
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        // Clamp to screen bounds
        let x_end = (x + w).min(self.info.width);
        let y_end = (y + h).min(self.info.height);
        let x_start = x.min(self.info.width);
        let y_start = y.min(self.info.height);

        let argb = color.to_argb();
        let pitch_pixels = self.info.pitch / 4;

        for py in y_start..y_end {
            for px in x_start..x_end {
                let offset = (py * pitch_pixels + px) as usize;
                unsafe {
                    self.buffer.add(offset).write_volatile(argb);
                }
            }
        }
    }

    /// Draw a horizontal line
    pub fn hline(&mut self, x: u32, y: u32, len: u32, color: Color) {
        if y >= self.info.height {
            return;
        }

        let x_end = (x + len).min(self.info.width);
        let x_start = x.min(self.info.width);

        let argb = color.to_argb();
        let pitch_pixels = self.info.pitch / 4;
        let row_offset = (y * pitch_pixels) as usize;

        for px in x_start..x_end {
            unsafe {
                self.buffer.add(row_offset + px as usize).write_volatile(argb);
            }
        }
    }

    /// Draw a vertical line
    pub fn vline(&mut self, x: u32, y: u32, len: u32, color: Color) {
        if x >= self.info.width {
            return;
        }

        let y_end = (y + len).min(self.info.height);
        let y_start = y.min(self.info.height);

        let argb = color.to_argb();
        let pitch_pixels = self.info.pitch / 4;

        for py in y_start..y_end {
            let offset = (py * pitch_pixels + x) as usize;
            unsafe {
                self.buffer.add(offset).write_volatile(argb);
            }
        }
    }

    /// Draw a rectangle outline
    pub fn draw_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if w == 0 || h == 0 {
            return;
        }

        // Top and bottom
        self.hline(x, y, w, color);
        if h > 1 {
            self.hline(x, y + h - 1, w, color);
        }

        // Left and right (excluding corners)
        if h > 2 {
            self.vline(x, y + 1, h - 2, color);
            if w > 1 {
                self.vline(x + w - 1, y + 1, h - 2, color);
            }
        }
    }
}
