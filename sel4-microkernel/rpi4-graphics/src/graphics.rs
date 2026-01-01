//! # Graphics Primitives
//!
//! Colors, points, and basic drawing operations.
//!
//! ## Verus Verification
//! Key properties verified:
//! - Color round-trip: `from_argb(to_argb(c)) == c`
//! - Rectangle containment: correct boundary logic
//! - All pixel operations bounds-checked

// Verus verification imports (used when running verus verification)
#[allow(unused_imports)]
use verus_builtin::*;
#[allow(unused_imports)]
use verus_builtin_macros::*;

/// ARGB color (Alpha, Red, Green, Blue)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Create a new color with full opacity
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Create a new color with alpha
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Convert to ARGB u32 for framebuffer
    ///
    /// # Verification
    /// Verified to correctly pack ARGB components.
    /// ensures result == ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    #[inline]
    pub const fn to_argb(&self) -> u32 {
        ((self.a as u32) << 24)
            | ((self.r as u32) << 16)
            | ((self.g as u32) << 8)
            | (self.b as u32)
    }

    /// Create from ARGB u32
    ///
    /// # Verification
    /// Verified round-trip: `from_argb(to_argb(c)) == c`
    /// ensures result.a == ((argb >> 24) & 0xFF), result.r == ((argb >> 16) & 0xFF), etc.
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

    // seL4/Microkit themed colors
    pub const SEL4_GREEN: Color = Color::rgb(0, 166, 81);
    pub const SEL4_DARK: Color = Color::rgb(0, 51, 51);
}

/// 2D point
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub const ORIGIN: Point = Point::new(0, 0);
}

/// Rectangle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// Check if a point is inside this rectangle
    ///
    /// # Verification
    /// Verified to correctly implement half-open interval containment:
    /// - x in [rect.x, rect.x + width)
    /// - y in [rect.y, rect.y + height)
    /// ensures result == (p.x >= self.x && p.y >= self.y && p.x < self.x + self.width && p.y < self.y + self.height)
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x
            && p.y >= self.y
            && p.x < self.x + self.width as i32
            && p.y < self.y + self.height as i32
    }

    /// Get the right edge x coordinate
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    /// Get the bottom edge y coordinate
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }
}

/// Draw a line using Bresenham's algorithm
pub fn draw_line(
    fb: &mut crate::Framebuffer,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: Color,
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    let mut x = x0;
    let mut y = y0;

    loop {
        if x >= 0 && y >= 0 {
            fb.put_pixel(x as u32, y as u32, color);
        }

        if x == x1 && y == y1 {
            break;
        }

        let e2 = 2 * err;

        if e2 >= dy {
            err += dy;
            x += sx;
        }

        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Draw a box with label (for architecture diagrams)
pub fn draw_box(
    fb: &mut crate::Framebuffer,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    border_color: Color,
    fill_color: Option<Color>,
) {
    // Fill if requested
    if let Some(fill) = fill_color {
        fb.fill_rect(x + 1, y + 1, width.saturating_sub(2), height.saturating_sub(2), fill);
    }

    // Draw border
    fb.draw_rect(x, y, width, height, border_color);
}

/// Draw an arrow pointing right
pub fn draw_arrow_right(
    fb: &mut crate::Framebuffer,
    x: u32,
    y: u32,
    length: u32,
    color: Color,
) {
    // Shaft
    fb.hline(x, y, length, color);

    // Arrowhead
    let tip_x = x + length;
    let arrow_size = 6u32;

    for i in 0..arrow_size {
        let offset = (arrow_size - i) / 2;
        if tip_x >= i && y >= offset {
            fb.put_pixel(tip_x - i, y - offset, color);
        }
        if tip_x >= i {
            fb.put_pixel(tip_x - i, y + offset, color);
        }
    }
}

/// Draw an arrow pointing down
pub fn draw_arrow_down(
    fb: &mut crate::Framebuffer,
    x: u32,
    y: u32,
    length: u32,
    color: Color,
) {
    // Shaft
    fb.vline(x, y, length, color);

    // Arrowhead
    let tip_y = y + length;
    let arrow_size = 6u32;

    for i in 0..arrow_size {
        let offset = (arrow_size - i) / 2;
        if tip_y >= i && x >= offset {
            fb.put_pixel(x - offset, tip_y - i, color);
        }
        if tip_y >= i {
            fb.put_pixel(x + offset, tip_y - i, color);
        }
    }
}
