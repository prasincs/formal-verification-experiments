//! Animation system for TV demo
//!
//! Provides various animations that can be played on the display.

use crate::display::{Framebuffer, Rgb565};

/// Animation trait for playable content
pub trait Animation {
    /// Update animation state (called each frame)
    fn update(&mut self);

    /// Render current frame to framebuffer
    fn render(&self, fb: &mut Framebuffer);

    /// Check if animation is complete (for non-looping animations)
    fn is_complete(&self) -> bool;

    /// Reset animation to start
    fn reset(&mut self);
}

/// Bouncing ball animation
pub struct BouncingBall {
    /// Ball position
    x: i16,
    y: i16,
    /// Ball velocity
    vx: i16,
    vy: i16,
    /// Ball radius
    radius: u16,
    /// Ball color
    color: Rgb565,
    /// Background color
    bg_color: Rgb565,
    /// Trail effect (previous positions)
    trail: [(i16, i16); 8],
    trail_idx: usize,
}

impl BouncingBall {
    /// Create a new bouncing ball animation
    pub fn new() -> Self {
        Self {
            x: 160,
            y: 120,
            vx: 4,
            vy: 3,
            radius: 15,
            color: Rgb565::RED,
            bg_color: Rgb565::BLACK,
            trail: [(160, 120); 8],
            trail_idx: 0,
        }
    }

    /// Set ball color
    pub fn set_color(&mut self, color: Rgb565) {
        self.color = color;
    }

    /// Set ball speed
    pub fn set_speed(&mut self, vx: i16, vy: i16) {
        self.vx = vx;
        self.vy = vy;
    }

    /// Set ball radius
    pub fn set_radius(&mut self, radius: u16) {
        self.radius = radius;
    }

    fn draw_circle(&self, fb: &mut Framebuffer, cx: i16, cy: i16, r: u16, color: Rgb565) {
        // Simple filled circle using midpoint algorithm
        let r = r as i16;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    let px = cx + dx;
                    let py = cy + dy;
                    if px >= 0 && px < 320 && py >= 0 && py < 240 {
                        fb.set_pixel(px as u16, py as u16, color);
                    }
                }
            }
        }
    }
}

impl Default for BouncingBall {
    fn default() -> Self {
        Self::new()
    }
}

impl Animation for BouncingBall {
    fn update(&mut self) {
        // Store trail position
        self.trail[self.trail_idx] = (self.x, self.y);
        self.trail_idx = (self.trail_idx + 1) % 8;

        // Move ball
        self.x += self.vx;
        self.y += self.vy;

        // Bounce off walls
        let r = self.radius as i16;
        if self.x - r <= 0 || self.x + r >= 320 {
            self.vx = -self.vx;
            self.x = self.x.clamp(r, 320 - r);
        }
        if self.y - r <= 0 || self.y + r >= 240 {
            self.vy = -self.vy;
            self.y = self.y.clamp(r, 240 - r);
        }
    }

    fn render(&self, fb: &mut Framebuffer) {
        // Clear background
        fb.clear(self.bg_color);

        // Draw trail (fading)
        for (i, &(tx, ty)) in self.trail.iter().enumerate() {
            let age = (self.trail_idx + 8 - i) % 8;
            let brightness = 30 + age * 15;
            let trail_color = Rgb565::from_rgb(brightness as u8, 0, 0);
            let trail_r = (self.radius as usize).saturating_sub(age * 2).max(3) as u16;
            self.draw_circle(fb, tx, ty, trail_r, trail_color);
        }

        // Draw main ball
        self.draw_circle(fb, self.x, self.y, self.radius, self.color);
    }

    fn is_complete(&self) -> bool {
        false // Loops forever
    }

    fn reset(&mut self) {
        self.x = 160;
        self.y = 120;
        self.trail = [(160, 120); 8];
        self.trail_idx = 0;
    }
}

/// Color cycling animation
pub struct ColorCycle {
    /// Current hue (0-359)
    hue: u16,
    /// Hue increment per frame
    speed: u16,
    /// Pattern type
    pattern: ColorPattern,
}

/// Color cycle pattern types
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ColorPattern {
    /// Solid color fill
    Solid,
    /// Horizontal gradient
    HorizontalGradient,
    /// Vertical gradient
    VerticalGradient,
    /// Radial gradient from center
    Radial,
    /// Plasma effect
    Plasma,
}

impl ColorCycle {
    /// Create a new color cycle animation
    pub fn new(pattern: ColorPattern) -> Self {
        Self {
            hue: 0,
            speed: 2,
            pattern,
        }
    }

    /// Set animation speed
    pub fn set_speed(&mut self, speed: u16) {
        self.speed = speed;
    }

    /// Convert HSV to RGB565
    fn hsv_to_rgb565(h: u16, s: u8, v: u8) -> Rgb565 {
        let h = h % 360;
        let s = s as u16;
        let v = v as u16;

        let c = (v * s) / 255;
        let x = (c * (60 - ((h % 120) as i16 - 60).unsigned_abs() as u16)) / 60;
        let m = v - c;

        let (r, g, b) = match h / 60 {
            0 => (c, x, 0),
            1 => (x, c, 0),
            2 => (0, c, x),
            3 => (0, x, c),
            4 => (x, 0, c),
            _ => (c, 0, x),
        };

        Rgb565::from_rgb((r + m) as u8, (g + m) as u8, (b + m) as u8)
    }
}

impl Default for ColorCycle {
    fn default() -> Self {
        Self::new(ColorPattern::Plasma)
    }
}

impl Animation for ColorCycle {
    fn update(&mut self) {
        self.hue = (self.hue + self.speed) % 360;
    }

    fn render(&self, fb: &mut Framebuffer) {
        match self.pattern {
            ColorPattern::Solid => {
                let color = Self::hsv_to_rgb565(self.hue, 255, 255);
                fb.clear(color);
            }

            ColorPattern::HorizontalGradient => {
                for x in 0..320u16 {
                    let h = (self.hue + x / 2) % 360;
                    let color = Self::hsv_to_rgb565(h, 255, 255);
                    for y in 0..240u16 {
                        fb.set_pixel(x, y, color);
                    }
                }
            }

            ColorPattern::VerticalGradient => {
                for y in 0..240u16 {
                    let h = (self.hue + y) % 360;
                    let color = Self::hsv_to_rgb565(h, 255, 255);
                    for x in 0..320u16 {
                        fb.set_pixel(x, y, color);
                    }
                }
            }

            ColorPattern::Radial => {
                let cx = 160i16;
                let cy = 120i16;
                for y in 0..240u16 {
                    for x in 0..320u16 {
                        let dx = (x as i16 - cx) as i32;
                        let dy = (y as i16 - cy) as i32;
                        let dist = ((dx * dx + dy * dy) as f32).sqrt() as u16;
                        let h = (self.hue + dist) % 360;
                        let color = Self::hsv_to_rgb565(h, 255, 255);
                        fb.set_pixel(x, y, color);
                    }
                }
            }

            ColorPattern::Plasma => {
                // Simplified plasma effect
                for y in 0..240u16 {
                    for x in 0..320u16 {
                        // Create plasma pattern using sin approximation
                        let v1 = ((x as i32 + self.hue as i32) % 64) as u16;
                        let v2 = ((y as i32 + self.hue as i32 / 2) % 64) as u16;
                        let v3 = ((x as i32 + y as i32 + self.hue as i32) % 128) as u16;

                        let h = (v1 * 3 + v2 * 2 + v3) % 360;
                        let color = Self::hsv_to_rgb565(h, 200, 255);
                        fb.set_pixel(x, y, color);
                    }
                }
            }
        }
    }

    fn is_complete(&self) -> bool {
        false
    }

    fn reset(&mut self) {
        self.hue = 0;
    }
}

/// Loading spinner animation
pub struct Spinner {
    /// Current angle (0-359)
    angle: u16,
    /// Rotation speed (degrees per frame)
    speed: u16,
    /// Center position
    cx: u16,
    cy: u16,
    /// Outer radius
    radius: u16,
    /// Spinner color
    color: Rgb565,
    /// Background color
    bg_color: Rgb565,
    /// Number of dots
    num_dots: u8,
}

impl Spinner {
    /// Create a new spinner animation
    pub fn new() -> Self {
        Self {
            angle: 0,
            speed: 15,
            cx: 160,
            cy: 120,
            radius: 40,
            color: Rgb565::from_rgb(100, 180, 255),
            bg_color: Rgb565::BLACK,
            num_dots: 8,
        }
    }

    /// Set spinner position
    pub fn set_position(&mut self, cx: u16, cy: u16) {
        self.cx = cx;
        self.cy = cy;
    }

    /// Set spinner colors
    pub fn set_colors(&mut self, color: Rgb565, bg: Rgb565) {
        self.color = color;
        self.bg_color = bg;
    }

    fn draw_filled_circle(&self, fb: &mut Framebuffer, cx: i16, cy: i16, r: u16, color: Rgb565) {
        let r = r as i16;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    let px = cx + dx;
                    let py = cy + dy;
                    if px >= 0 && px < 320 && py >= 0 && py < 240 {
                        fb.set_pixel(px as u16, py as u16, color);
                    }
                }
            }
        }
    }

    /// Simple sine approximation (input: 0-359, output: -256 to 256)
    fn sin_approx(angle: u16) -> i16 {
        let angle = angle % 360;
        // Piecewise linear approximation
        let quadrant = angle / 90;
        let offset = (angle % 90) as i16;

        match quadrant {
            0 => (offset * 256) / 90,
            1 => 256 - ((offset * 256) / 90),
            2 => -((offset * 256) / 90),
            _ => -256 + ((offset * 256) / 90),
        }
    }

    fn cos_approx(angle: u16) -> i16 {
        Self::sin_approx((angle + 90) % 360)
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Animation for Spinner {
    fn update(&mut self) {
        self.angle = (self.angle + self.speed) % 360;
    }

    fn render(&self, fb: &mut Framebuffer) {
        fb.clear(self.bg_color);

        let cx = self.cx as i16;
        let cy = self.cy as i16;
        let r = self.radius as i16;

        for i in 0..self.num_dots {
            let dot_angle = self.angle + (i as u16) * (360 / self.num_dots as u16);

            // Calculate position
            let sin = Self::sin_approx(dot_angle);
            let cos = Self::cos_approx(dot_angle);
            let dx = (cos * r as i16) / 256;
            let dy = (sin * r as i16) / 256;

            let px = cx + dx;
            let py = cy + dy;

            // Size varies with position (leading dot is larger)
            let base_size = 6u16;
            let size_mod = if i == 0 { 4 } else { (8 - i as u16).min(base_size) };
            let dot_r = (base_size - size_mod / 2).max(2);

            // Brightness varies (leading dot is brighter)
            let brightness = 255 - (i as u16 * 25).min(200) as u8;
            let dot_color = Rgb565::from_rgb(
                (brightness as u16 * 100 / 255) as u8,
                (brightness as u16 * 180 / 255) as u8,
                brightness,
            );

            self.draw_filled_circle(fb, px, py, dot_r, dot_color);
        }
    }

    fn is_complete(&self) -> bool {
        false
    }

    fn reset(&mut self) {
        self.angle = 0;
    }
}

/// Animation player that manages playback
pub struct AnimationPlayer {
    /// Current animation type being played
    current: AnimationType,
    /// Frame counter
    frame: u32,
    /// Is playing
    playing: bool,
    /// Bouncing ball instance
    ball: BouncingBall,
    /// Color cycle instance
    colors: ColorCycle,
    /// Spinner instance
    spinner: Spinner,
}

/// Available animation types
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AnimationType {
    BouncingBall,
    ColorCycle,
    Spinner,
}

impl AnimationPlayer {
    /// Create a new animation player
    pub fn new() -> Self {
        Self {
            current: AnimationType::BouncingBall,
            frame: 0,
            playing: false,
            ball: BouncingBall::new(),
            colors: ColorCycle::new(ColorPattern::Plasma),
            spinner: Spinner::new(),
        }
    }

    /// Start playing an animation
    pub fn play(&mut self, anim_type: AnimationType) {
        self.current = anim_type;
        self.playing = true;
        self.frame = 0;

        // Reset the selected animation
        match anim_type {
            AnimationType::BouncingBall => self.ball.reset(),
            AnimationType::ColorCycle => self.colors.reset(),
            AnimationType::Spinner => self.spinner.reset(),
        }
    }

    /// Stop playback
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Toggle play/pause
    pub fn toggle(&mut self) {
        self.playing = !self.playing;
    }

    /// Check if playing
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Get current animation type
    pub fn current(&self) -> AnimationType {
        self.current
    }

    /// Switch to next animation
    pub fn next(&mut self) {
        self.current = match self.current {
            AnimationType::BouncingBall => AnimationType::ColorCycle,
            AnimationType::ColorCycle => AnimationType::Spinner,
            AnimationType::Spinner => AnimationType::BouncingBall,
        };

        if self.playing {
            self.play(self.current);
        }
    }

    /// Switch to previous animation
    pub fn prev(&mut self) {
        self.current = match self.current {
            AnimationType::BouncingBall => AnimationType::Spinner,
            AnimationType::ColorCycle => AnimationType::BouncingBall,
            AnimationType::Spinner => AnimationType::ColorCycle,
        };

        if self.playing {
            self.play(self.current);
        }
    }

    /// Update animation state
    pub fn update(&mut self) {
        if !self.playing {
            return;
        }

        self.frame = self.frame.wrapping_add(1);

        match self.current {
            AnimationType::BouncingBall => self.ball.update(),
            AnimationType::ColorCycle => self.colors.update(),
            AnimationType::Spinner => self.spinner.update(),
        }
    }

    /// Render current frame
    pub fn render(&self, fb: &mut Framebuffer) {
        match self.current {
            AnimationType::BouncingBall => self.ball.render(fb),
            AnimationType::ColorCycle => self.colors.render(fb),
            AnimationType::Spinner => self.spinner.render(fb),
        }
    }

    /// Get frame count
    pub fn frame_count(&self) -> u32 {
        self.frame
    }
}

impl Default for AnimationPlayer {
    fn default() -> Self {
        Self::new()
    }
}
