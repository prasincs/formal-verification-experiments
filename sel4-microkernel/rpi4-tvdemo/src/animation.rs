//! Animation system for TV demo
//!
//! Provides various animations that can be played on any display backend.

use crate::backend::{DisplayBackend, Color};

/// Animation trait for playable content
pub trait Animation {
    /// Update animation state (called each frame)
    fn update(&mut self);

    /// Render current frame to display
    fn render<D: DisplayBackend>(&self, display: &mut D);

    /// Check if animation is complete (for non-looping animations)
    fn is_complete(&self) -> bool;

    /// Reset animation to start
    fn reset(&mut self);
}

/// Bouncing ball animation
pub struct BouncingBall {
    /// Ball position
    x: i32,
    y: i32,
    /// Ball velocity
    vx: i32,
    vy: i32,
    /// Ball radius
    radius: u32,
    /// Ball color
    color: Color,
    /// Background color
    bg_color: Color,
    /// Trail effect (previous positions)
    trail: [(i32, i32); 8],
    trail_idx: usize,
    /// Screen dimensions
    width: u32,
    height: u32,
}

impl BouncingBall {
    /// Create a new bouncing ball animation
    pub fn new(width: u32, height: u32) -> Self {
        let cx = (width / 2) as i32;
        let cy = (height / 2) as i32;
        Self {
            x: cx,
            y: cy,
            vx: 4,
            vy: 3,
            radius: 15,
            color: Color::RED,
            bg_color: Color::BLACK,
            trail: [(cx, cy); 8],
            trail_idx: 0,
            width,
            height,
        }
    }

    /// Set ball color
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }

    /// Set ball speed
    pub fn set_speed(&mut self, vx: i32, vy: i32) {
        self.vx = vx;
        self.vy = vy;
    }

    /// Set ball radius
    pub fn set_radius(&mut self, radius: u32) {
        self.radius = radius;
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
        let r = self.radius as i32;
        let w = self.width as i32;
        let h = self.height as i32;

        if self.x - r <= 0 || self.x + r >= w {
            self.vx = -self.vx;
            self.x = self.x.clamp(r, w - r);
        }
        if self.y - r <= 0 || self.y + r >= h {
            self.vy = -self.vy;
            self.y = self.y.clamp(r, h - r);
        }
    }

    fn render<D: DisplayBackend>(&self, display: &mut D) {
        // Clear background
        display.clear(self.bg_color);

        // Draw trail (fading)
        for (i, &(tx, ty)) in self.trail.iter().enumerate() {
            let age = (self.trail_idx + 8 - i) % 8;
            let brightness = 30 + age * 15;
            let trail_color = Color::rgb(brightness as u8, 0, 0);
            let trail_r = (self.radius as usize).saturating_sub(age * 2).max(3) as u32;
            display.fill_circle(tx, ty, trail_r, trail_color);
        }

        // Draw main ball
        display.fill_circle(self.x, self.y, self.radius, self.color);
    }

    fn is_complete(&self) -> bool {
        false // Loops forever
    }

    fn reset(&mut self) {
        let cx = (self.width / 2) as i32;
        let cy = (self.height / 2) as i32;
        self.x = cx;
        self.y = cy;
        self.trail = [(cx, cy); 8];
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
    /// Screen dimensions
    width: u32,
    height: u32,
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
    pub fn new(width: u32, height: u32, pattern: ColorPattern) -> Self {
        Self {
            hue: 0,
            speed: 2,
            pattern,
            width,
            height,
        }
    }

    /// Set animation speed
    pub fn set_speed(&mut self, speed: u16) {
        self.speed = speed;
    }

    /// Convert HSV to RGB
    fn hsv_to_color(h: u16, s: u8, v: u8) -> Color {
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

        Color::rgb((r + m) as u8, (g + m) as u8, (b + m) as u8)
    }
}

impl Animation for ColorCycle {
    fn update(&mut self) {
        self.hue = (self.hue + self.speed) % 360;
    }

    fn render<D: DisplayBackend>(&self, display: &mut D) {
        match self.pattern {
            ColorPattern::Solid => {
                let color = Self::hsv_to_color(self.hue, 255, 255);
                display.clear(color);
            }

            ColorPattern::HorizontalGradient => {
                for x in 0..self.width {
                    let h = (self.hue + (x as u16) / 2) % 360;
                    let color = Self::hsv_to_color(h, 255, 255);
                    for y in 0..self.height {
                        display.set_pixel(x, y, color);
                    }
                }
            }

            ColorPattern::VerticalGradient => {
                for y in 0..self.height {
                    let h = (self.hue + y as u16) % 360;
                    let color = Self::hsv_to_color(h, 255, 255);
                    for x in 0..self.width {
                        display.set_pixel(x, y, color);
                    }
                }
            }

            ColorPattern::Radial => {
                let cx = (self.width / 2) as i32;
                let cy = (self.height / 2) as i32;
                for y in 0..self.height {
                    for x in 0..self.width {
                        let dx = (x as i32 - cx) as i32;
                        let dy = (y as i32 - cy) as i32;
                        let dist = isqrt((dx * dx + dy * dy) as u32) as u16;
                        let h = (self.hue + dist) % 360;
                        let color = Self::hsv_to_color(h, 255, 255);
                        display.set_pixel(x, y, color);
                    }
                }
            }

            ColorPattern::Plasma => {
                for y in 0..self.height {
                    for x in 0..self.width {
                        let v1 = ((x as i32 + self.hue as i32) % 64) as u16;
                        let v2 = ((y as i32 + self.hue as i32 / 2) % 64) as u16;
                        let v3 = ((x as i32 + y as i32 + self.hue as i32) % 128) as u16;
                        let h = (v1 * 3 + v2 * 2 + v3) % 360;
                        let color = Self::hsv_to_color(h, 200, 255);
                        display.set_pixel(x, y, color);
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
    cx: u32,
    cy: u32,
    /// Outer radius
    radius: u32,
    /// Spinner color
    color: Color,
    /// Background color
    bg_color: Color,
    /// Number of dots
    num_dots: u8,
}

impl Spinner {
    /// Create a new spinner animation
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            angle: 0,
            speed: 15,
            cx: width / 2,
            cy: height / 2,
            radius: 40,
            color: Color::rgb(100, 180, 255),
            bg_color: Color::BLACK,
            num_dots: 8,
        }
    }

    /// Set spinner position
    pub fn set_position(&mut self, cx: u32, cy: u32) {
        self.cx = cx;
        self.cy = cy;
    }

    /// Set spinner colors
    pub fn set_colors(&mut self, color: Color, bg: Color) {
        self.color = color;
        self.bg_color = bg;
    }

    /// Simple sine approximation (input: 0-359, output: -256 to 256)
    fn sin_approx(angle: u16) -> i32 {
        let angle = angle % 360;
        let quadrant = angle / 90;
        let offset = (angle % 90) as i32;

        match quadrant {
            0 => (offset * 256) / 90,
            1 => 256 - ((offset * 256) / 90),
            2 => -((offset * 256) / 90),
            _ => -256 + ((offset * 256) / 90),
        }
    }

    fn cos_approx(angle: u16) -> i32 {
        Self::sin_approx((angle + 90) % 360)
    }
}

impl Animation for Spinner {
    fn update(&mut self) {
        self.angle = (self.angle + self.speed) % 360;
    }

    fn render<D: DisplayBackend>(&self, display: &mut D) {
        display.clear(self.bg_color);

        let cx = self.cx as i32;
        let cy = self.cy as i32;
        let r = self.radius as i32;

        for i in 0..self.num_dots {
            let dot_angle = self.angle + (i as u16) * (360 / self.num_dots as u16);

            let sin = Self::sin_approx(dot_angle);
            let cos = Self::cos_approx(dot_angle);
            let dx = (cos * r) / 256;
            let dy = (sin * r) / 256;

            let px = cx + dx;
            let py = cy + dy;

            let base_size = 6u32;
            let size_mod = if i == 0 { 4 } else { (8 - i as u32).min(base_size) };
            let dot_r = (base_size - size_mod / 2).max(2);

            let brightness = 255 - (i as u16 * 25).min(200) as u8;
            let dot_color = Color::rgb(
                (brightness as u16 * 100 / 255) as u8,
                (brightness as u16 * 180 / 255) as u8,
                brightness,
            );

            display.fill_circle(px, py, dot_r, dot_color);
        }
    }

    fn is_complete(&self) -> bool {
        false
    }

    fn reset(&mut self) {
        self.angle = 0;
    }
}

/// Available animation types
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AnimationType {
    BouncingBall,
    ColorCycle,
    Spinner,
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

impl AnimationPlayer {
    /// Create a new animation player
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            current: AnimationType::BouncingBall,
            frame: 0,
            playing: false,
            ball: BouncingBall::new(width, height),
            colors: ColorCycle::new(width, height, ColorPattern::Plasma),
            spinner: Spinner::new(width, height),
        }
    }

    /// Start playing an animation
    pub fn play(&mut self, anim_type: AnimationType) {
        self.current = anim_type;
        self.playing = true;
        self.frame = 0;

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
    pub fn render<D: DisplayBackend>(&self, display: &mut D) {
        match self.current {
            AnimationType::BouncingBall => self.ball.render(display),
            AnimationType::ColorCycle => self.colors.render(display),
            AnimationType::Spinner => self.spinner.render(display),
        }
    }

    /// Get frame count
    pub fn frame_count(&self) -> u32 {
        self.frame
    }
}

/// Integer square root
fn isqrt(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}
