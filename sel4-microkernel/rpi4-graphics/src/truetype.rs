//! # TrueType Font Rendering
//!
//! Real font rendering using fontdue for high-quality text display.
//! Supports variable font sizes and anti-aliased rendering.

use crate::framebuffer::Framebuffer;
use crate::graphics::Color;
use fontdue::{Font, FontSettings};

/// Embedded DejaVu Sans Mono font (343KB) - Latin/ASCII
pub static DEJAVU_MONO: &[u8] = include_bytes!("../fonts/DejaVuSansMono.ttf");

/// Embedded Noto Sans Devanagari font (224KB) - Nepali/Hindi/Sanskrit
pub static NOTO_DEVANAGARI: &[u8] = include_bytes!("../fonts/NotoSansDevanagari-Regular.ttf");

// Additional font placeholders (add .ttf files to fonts/ directory to enable)
// pub static LOHIT_DEVANAGARI: &[u8] = include_bytes!("../fonts/Lohit-Devanagari.ttf");
// pub static PREETI: &[u8] = include_bytes!("../fonts/Preeti.ttf");  // Legacy non-Unicode
// pub static KANTIPUR: &[u8] = include_bytes!("../fonts/Kantipur.ttf");  // Legacy non-Unicode

/// Font renderer for TrueType fonts
pub struct FontRenderer {
    font: Font,
    size: f32,
}

/// Glyph metrics returned after rasterization
#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    pub width: usize,
    pub height: usize,
    pub advance_width: f32,
    pub xmin: i32,
    pub ymin: i32,
}

impl FontRenderer {
    /// Create a new font renderer from embedded font data
    pub fn new(font_data: &[u8], size: f32) -> Option<Self> {
        let font = Font::from_bytes(font_data, FontSettings::default()).ok()?;
        Some(Self { font, size })
    }

    /// Create a renderer using the built-in DejaVu Sans Mono
    pub fn default_mono(size: f32) -> Option<Self> {
        Self::new(DEJAVU_MONO, size)
    }

    /// Create a renderer using Noto Sans Devanagari for Nepali/Hindi text
    pub fn devanagari(size: f32) -> Option<Self> {
        Self::new(NOTO_DEVANAGARI, size)
    }

    /// Get the current font size
    pub fn size(&self) -> f32 {
        self.size
    }

    /// Set the font size
    pub fn set_size(&mut self, size: f32) {
        self.size = size;
    }

    /// Get line height for current font size
    pub fn line_height(&self) -> f32 {
        self.font
            .horizontal_line_metrics(self.size)
            .map_or(self.size, |m| m.new_line_size)
    }

    /// Rasterize a single character and draw it to the framebuffer
    /// Returns the advance width (how far to move for next character)
    pub fn draw_char(
        &self,
        fb: &mut Framebuffer,
        x: i32,
        y: i32,
        c: char,
        fg: Color,
        bg: Option<Color>,
    ) -> f32 {
        let (metrics, bitmap) = self.font.rasterize(c, self.size);

        // Calculate actual position with glyph offset
        let glyph_x = x + metrics.xmin;
        let glyph_y = y + (self.size as i32) - metrics.height as i32 - metrics.ymin;

        // Draw background if specified
        if let Some(bg_color) = bg {
            let advance = metrics.advance_width as u32;
            let height = self.size as u32;
            if x >= 0 && y >= 0 {
                fb.fill_rect(x as u32, y as u32, advance, height, bg_color);
            }
        }

        // Draw glyph pixels with alpha blending
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let alpha = bitmap[row * metrics.width + col];
                if alpha > 0 {
                    let px = glyph_x + col as i32;
                    let py = glyph_y + row as i32;

                    if px >= 0 && py >= 0 {
                        let color = if alpha == 255 {
                            fg
                        } else {
                            // Simple alpha blend with background
                            blend_color(fg, bg.unwrap_or(Color::TERM_BLACK), alpha)
                        };
                        fb.put_pixel(px as u32, py as u32, color);
                    }
                }
            }
        }

        metrics.advance_width
    }

    /// Draw a string at the given position
    /// Returns the total width drawn
    pub fn draw_string(
        &self,
        fb: &mut Framebuffer,
        x: i32,
        y: i32,
        s: &str,
        fg: Color,
        bg: Option<Color>,
    ) -> f32 {
        let mut cursor_x = x as f32;

        for c in s.chars() {
            if c == '\n' {
                continue; // Caller handles newlines
            }
            let advance = self.draw_char(fb, cursor_x as i32, y, c, fg, bg);
            cursor_x += advance;
        }

        cursor_x - x as f32
    }

    /// Get metrics for a character without drawing
    pub fn char_metrics(&self, c: char) -> GlyphMetrics {
        let (metrics, _) = self.font.rasterize(c, self.size);
        GlyphMetrics {
            width: metrics.width,
            height: metrics.height,
            advance_width: metrics.advance_width,
            xmin: metrics.xmin,
            ymin: metrics.ymin,
        }
    }

    /// Measure string width without drawing
    pub fn measure_string(&self, s: &str) -> f32 {
        let mut width = 0.0;
        for c in s.chars() {
            let (metrics, _) = self.font.rasterize(c, self.size);
            width += metrics.advance_width;
        }
        width
    }
}

/// Blend foreground and background colors with alpha
fn blend_color(fg: Color, bg: Color, alpha: u8) -> Color {
    let a = alpha as u32;
    let inv_a = 255 - a;

    Color::rgb(
        ((fg.r as u32 * a + bg.r as u32 * inv_a) / 255) as u8,
        ((fg.g as u32 * a + bg.g as u32 * inv_a) / 255) as u8,
        ((fg.b as u32 * a + bg.b as u32 * inv_a) / 255) as u8,
    )
}

/// Common font sizes
pub mod sizes {
    pub const SMALL: f32 = 12.0;
    pub const NORMAL: f32 = 16.0;
    pub const LARGE: f32 = 20.0;
    pub const XLARGE: f32 = 24.0;
    pub const TITLE: f32 = 32.0;
}
