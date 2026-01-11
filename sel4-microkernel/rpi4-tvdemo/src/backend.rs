//! Unified display backend trait
//!
//! This trait allows code to work with different display backends
//! (SPI LCD, HDMI framebuffer, etc.) through a common interface.

/// Unified color type that can convert between RGB565 and ARGB
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Create a color with full opacity
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Create a color with alpha
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Convert to RGB565 (16-bit, for SPI displays)
    #[inline]
    pub const fn to_rgb565(&self) -> u16 {
        let r5 = (self.r >> 3) as u16;
        let g6 = (self.g >> 2) as u16;
        let b5 = (self.b >> 3) as u16;
        (r5 << 11) | (g6 << 5) | b5
    }

    /// Convert to ARGB (32-bit, for HDMI)
    #[inline]
    pub const fn to_argb(&self) -> u32 {
        ((self.a as u32) << 24)
            | ((self.r as u32) << 16)
            | ((self.g as u32) << 8)
            | (self.b as u32)
    }

    /// Create from RGB565
    pub const fn from_rgb565(rgb565: u16) -> Self {
        let r = ((rgb565 >> 11) & 0x1F) as u8;
        let g = ((rgb565 >> 5) & 0x3F) as u8;
        let b = (rgb565 & 0x1F) as u8;
        Self {
            r: (r << 3) | (r >> 2),
            g: (g << 2) | (g >> 4),
            b: (b << 3) | (b >> 2),
            a: 255,
        }
    }

    /// Create from ARGB
    pub const fn from_argb(argb: u32) -> Self {
        Self {
            a: ((argb >> 24) & 0xFF) as u8,
            r: ((argb >> 16) & 0xFF) as u8,
            g: ((argb >> 8) & 0xFF) as u8,
            b: (argb & 0xFF) as u8,
        }
    }

    // Predefined colors
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const RED: Color = Color::rgb(255, 0, 0);
    pub const GREEN: Color = Color::rgb(0, 255, 0);
    pub const BLUE: Color = Color::rgb(0, 0, 255);
    pub const YELLOW: Color = Color::rgb(255, 255, 0);
    pub const CYAN: Color = Color::rgb(0, 255, 255);
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);
    pub const GRAY: Color = Color::rgb(128, 128, 128);
    pub const DARK_GRAY: Color = Color::rgb(64, 64, 64);
    pub const LIGHT_GRAY: Color = Color::rgb(192, 192, 192);
}

/// Display backend trait for portable graphics code
pub trait DisplayBackend {
    /// Get display width in pixels
    fn width(&self) -> u32;

    /// Get display height in pixels
    fn height(&self) -> u32;

    /// Set a pixel at (x, y)
    /// Returns false if out of bounds
    fn set_pixel(&mut self, x: u32, y: u32, color: Color) -> bool;

    /// Clear the entire display to a color
    fn clear(&mut self, color: Color);

    /// Fill a rectangle
    fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) -> bool;

    /// Draw a horizontal line
    fn hline(&mut self, x: u32, y: u32, len: u32, color: Color) {
        for i in 0..len {
            self.set_pixel(x + i, y, color);
        }
    }

    /// Draw a vertical line
    fn vline(&mut self, x: u32, y: u32, len: u32, color: Color) {
        for i in 0..len {
            self.set_pixel(x, y + i, color);
        }
    }

    /// Draw a rectangle outline
    fn draw_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if w == 0 || h == 0 {
            return;
        }
        self.hline(x, y, w, color);
        if h > 1 {
            self.hline(x, y + h - 1, w, color);
        }
        if h > 2 {
            self.vline(x, y + 1, h - 2, color);
            if w > 1 {
                self.vline(x + w - 1, y + 1, h - 2, color);
            }
        }
    }

    /// Draw a filled circle
    fn fill_circle(&mut self, cx: i32, cy: i32, radius: u32, color: Color) {
        let r = radius as i32;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    let px = cx + dx;
                    let py = cy + dy;
                    if px >= 0 && py >= 0 {
                        self.set_pixel(px as u32, py as u32, color);
                    }
                }
            }
        }
    }
}

/// Scaled display wrapper that maps a virtual resolution to a physical display
/// Useful for running 320x240 content on 1280x720 HDMI
pub struct ScaledDisplay<D: DisplayBackend> {
    inner: D,
    virtual_width: u32,
    virtual_height: u32,
    scale_x: u32,
    scale_y: u32,
}

impl<D: DisplayBackend> ScaledDisplay<D> {
    /// Create a scaled display wrapper
    /// Virtual dimensions are what the application sees
    /// Physical dimensions are the actual display
    pub fn new(inner: D, virtual_width: u32, virtual_height: u32) -> Self {
        let phys_w = inner.width();
        let phys_h = inner.height();

        // Calculate integer scale factors
        let scale_x = phys_w / virtual_width;
        let scale_y = phys_h / virtual_height;

        Self {
            inner,
            virtual_width,
            virtual_height,
            scale_x: scale_x.max(1),
            scale_y: scale_y.max(1),
        }
    }

    /// Get the underlying display
    pub fn inner(&self) -> &D {
        &self.inner
    }

    /// Get mutable access to underlying display
    pub fn inner_mut(&mut self) -> &mut D {
        &mut self.inner
    }

    /// Get the scale factors
    pub fn scale(&self) -> (u32, u32) {
        (self.scale_x, self.scale_y)
    }
}

impl<D: DisplayBackend> DisplayBackend for ScaledDisplay<D> {
    fn width(&self) -> u32 {
        self.virtual_width
    }

    fn height(&self) -> u32 {
        self.virtual_height
    }

    fn set_pixel(&mut self, x: u32, y: u32, color: Color) -> bool {
        if x >= self.virtual_width || y >= self.virtual_height {
            return false;
        }

        // Scale up the pixel to a rectangle
        let px = x * self.scale_x;
        let py = y * self.scale_y;

        for dy in 0..self.scale_y {
            for dx in 0..self.scale_x {
                self.inner.set_pixel(px + dx, py + dy, color);
            }
        }
        true
    }

    fn clear(&mut self, color: Color) {
        self.inner.clear(color);
    }

    fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) -> bool {
        if x + w > self.virtual_width || y + h > self.virtual_height {
            return false;
        }

        let px = x * self.scale_x;
        let py = y * self.scale_y;
        let pw = w * self.scale_x;
        let ph = h * self.scale_y;

        self.inner.fill_rect(px, py, pw, ph, color)
    }
}
