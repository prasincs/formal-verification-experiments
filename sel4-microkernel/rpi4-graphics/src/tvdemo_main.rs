//! TV Demo for HDMI on Raspberry Pi 4
//!
//! Displays a simple snake animation on HDMI using direct framebuffer access.
//! Uses the same drawing approach as the working graphics demo.

#![no_std]
#![no_main]

use sel4_microkit::{debug_println, protection_domain, Handler, ChannelSet};
use core::fmt;

/// Framebuffer virtual address (mapped in tvdemo.system)
const FB_VADDR: usize = 0x5_0001_0000;

/// Screen dimensions - must match config.txt hdmi_mode=82 (1920x1080)
const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;

/// GPIO virtual address (mapped in tvdemo.system)
const GPIO_BASE: usize = 0x5_0200_0000;

/// Snake segment
#[derive(Clone, Copy)]
struct Segment {
    x: i32,
    y: i32,
}

/// Snake state
struct Snake {
    segments: [Segment; 30],
    length: usize,
    direction: u8, // 0=right, 1=down, 2=left, 3=up
    frame: u32,
}

impl Snake {
    fn new() -> Self {
        let mut segments = [Segment { x: 0, y: 0 }; 30];
        // Start in center, horizontal line
        let start_x = (WIDTH / 2) as i32;
        let start_y = (HEIGHT / 2) as i32;
        for i in 0..20 {
            segments[i] = Segment {
                x: start_x - (i as i32 * 25),
                y: start_y,
            };
        }
        Self {
            segments,
            length: 20,
            direction: 0,
            frame: 0,
        }
    }

    fn update(&mut self) {
        self.frame = self.frame.wrapping_add(1);

        // Change direction periodically
        if self.frame % 45 == 0 {
            self.direction = (self.direction + 1) % 4;
        }
        if self.frame % 120 == 0 {
            self.direction = (self.direction + 3) % 4;
        }

        // Calculate new head position
        let head = self.segments[0];
        let speed = 12i32;
        let mut new_x = head.x;
        let mut new_y = head.y;

        match self.direction {
            0 => new_x += speed,
            1 => new_y += speed,
            2 => new_x -= speed,
            _ => new_y -= speed,
        }

        // Wrap around screen
        if new_x < 0 { new_x = WIDTH as i32 - 1; }
        if new_x >= WIDTH as i32 { new_x = 0; }
        if new_y < 0 { new_y = HEIGHT as i32 - 1; }
        if new_y >= HEIGHT as i32 { new_y = 0; }

        // Move segments
        for i in (1..self.length).rev() {
            self.segments[i] = self.segments[i - 1];
        }
        self.segments[0] = Segment { x: new_x, y: new_y };
    }
}

struct TvDemoHandler;

impl TvDemoHandler {
    const fn new() -> Self {
        Self
    }
}

/// Draw a filled block (same pattern as working graphics demo)
#[inline]
unsafe fn draw_block(fb: *mut u32, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for dy in 0..h {
        for dx in 0..w {
            let px = x + dx;
            let py = y + dy;
            if px < WIDTH && py < HEIGHT {
                fb.add(py * WIDTH + px).write_volatile(color);
            }
        }
    }
}

/// HSV to RGB - returns ARGB u32
fn hsv_to_rgb(h: u16, s: u8, v: u8) -> u32 {
    let h = h % 360;
    let s = s as u32;
    let v = v as u32;

    let c = (v * s) / 255;
    let x = (c * (60 - ((h % 120) as i32 - 60).unsigned_abs() as u32)) / 60;
    let m = v - c;

    let (r, g, b) = match h / 60 {
        0 => (c, x, 0),
        1 => (x, c, 0),
        2 => (0, c, x),
        3 => (0, x, c),
        4 => (x, 0, c),
        _ => (c, 0, x),
    };

    0xFF000000 | (((r + m) as u32) << 16) | (((g + m) as u32) << 8) | ((b + m) as u32)
}

/// Clear screen to a color
unsafe fn clear_screen(fb: *mut u32, color: u32) {
    for i in 0..(WIDTH * HEIGHT) {
        fb.add(i).write_volatile(color);
    }
}

/// Draw the snake
unsafe fn draw_snake(fb: *mut u32, snake: &Snake, frame: u32) {
    let segment_size = 30usize;  // Larger for 1080p

    for i in 0..snake.length {
        let seg = snake.segments[i];
        if seg.x >= 0 && seg.y >= 0 {
            let x = seg.x as usize;
            let y = seg.y as usize;

            // Rainbow color
            let hue = ((i as u32 * 18 + frame * 4) % 360) as u16;
            let color = hsv_to_rgb(hue, 255, 255);

            // Draw segment centered on position
            let sx = x.saturating_sub(segment_size / 2);
            let sy = y.saturating_sub(segment_size / 2);
            draw_block(fb, sx, sy, segment_size, segment_size, color);
        }
    }

    // Draw eyes on head
    let head = snake.segments[0];
    if head.x >= 0 && head.y >= 0 {
        let hx = head.x as usize;
        let hy = head.y as usize;

        let white = 0xFFFFFFFF;
        let black = 0xFF000000;

        // Eye positions based on direction
        let (e1x, e1y, e2x, e2y) = match snake.direction {
            0 => (hx + 5, hy.saturating_sub(5), hx + 5, hy + 5),
            1 => (hx.saturating_sub(5), hy + 5, hx + 5, hy + 5),
            2 => (hx.saturating_sub(5), hy.saturating_sub(5), hx.saturating_sub(5), hy + 5),
            _ => (hx.saturating_sub(5), hy.saturating_sub(5), hx + 5, hy.saturating_sub(5)),
        };

        draw_block(fb, e1x, e1y, 6, 6, white);
        draw_block(fb, e2x, e2y, 6, 6, white);
        draw_block(fb, e1x + 2, e1y + 2, 3, 3, black);
        draw_block(fb, e2x + 2, e2y + 2, 3, 3, black);
    }
}

/// Draw "SNAKE" text using block letters (same style as graphics demo "SEL4")
unsafe fn draw_title(fb: *mut u32) {
    let white = 0xFFFFFFFF;
    let block = 20usize;  // Larger blocks for 1080p
    let start_x = 600usize;  // Centered for 1920 width
    let start_y = 80usize;

    // S
    draw_block(fb, start_x, start_y, block * 3, block, white);
    draw_block(fb, start_x, start_y + block, block, block, white);
    draw_block(fb, start_x, start_y + block * 2, block * 3, block, white);
    draw_block(fb, start_x + block * 2, start_y + block * 3, block, block, white);
    draw_block(fb, start_x, start_y + block * 4, block * 3, block, white);

    // N
    let n_x = start_x + block * 5;
    draw_block(fb, n_x, start_y, block, block * 5, white);
    draw_block(fb, n_x + block, start_y + block, block, block, white);
    draw_block(fb, n_x + block * 2, start_y, block, block * 5, white);

    // A
    let a_x = start_x + block * 9;
    draw_block(fb, a_x, start_y, block * 3, block, white);
    draw_block(fb, a_x, start_y + block, block, block * 4, white);
    draw_block(fb, a_x + block * 2, start_y + block, block, block * 4, white);
    draw_block(fb, a_x, start_y + block * 2, block * 3, block, white);

    // K
    let k_x = start_x + block * 14;
    draw_block(fb, k_x, start_y, block, block * 5, white);
    draw_block(fb, k_x + block, start_y + block * 2, block, block, white);
    draw_block(fb, k_x + block * 2, start_y, block, block * 2, white);
    draw_block(fb, k_x + block * 2, start_y + block * 3, block, block * 2, white);

    // E
    let e_x = start_x + block * 18;
    draw_block(fb, e_x, start_y, block * 3, block, white);
    draw_block(fb, e_x, start_y + block, block, block, white);
    draw_block(fb, e_x, start_y + block * 2, block * 2, block, white);
    draw_block(fb, e_x, start_y + block * 3, block, block, white);
    draw_block(fb, e_x, start_y + block * 4, block * 3, block, white);
}

/// Draw frame counter
unsafe fn draw_frame_counter(fb: *mut u32, frame: u32) {
    let green = 0xFF00FF00;
    let x_base = 50usize;
    let y_base = 1000usize;  // Near bottom for 1080p

    // Simple bar that grows with frame count (visual proof of animation)
    let bar_width = ((frame % 200) as usize) + 10;
    draw_block(fb, x_base, y_base, bar_width, 20, green);

    // Draw frame number as simple blocks
    let digit_w = 12usize;
    let digit_h = 20usize;
    let mut n = frame % 10000;

    for i in 0..4 {
        let digit = (n % 10) as usize;
        n /= 10;
        let dx = x_base + 300 - (i * 18);

        // Draw digit as filled block with number indicator
        let brightness = 100 + (digit * 15) as u8;
        let color = 0xFF000000 | ((brightness as u32) << 16) | ((brightness as u32) << 8) | (brightness as u32);
        draw_block(fb, dx, y_base, digit_w, digit_h, color);
    }
}

/// Draw border
unsafe fn draw_border(fb: *mut u32) {
    let gray = 0xFF808080;

    // Top and bottom
    for x in 0..WIDTH {
        fb.add(x).write_volatile(gray);
        fb.add((HEIGHT - 1) * WIDTH + x).write_volatile(gray);
    }
    // Left and right
    for y in 0..HEIGHT {
        fb.add(y * WIDTH).write_volatile(gray);
        fb.add(y * WIDTH + WIDTH - 1).write_volatile(gray);
    }
}

/// Blink the activity LED
fn blink_activity_led() {
    debug_println!("Blinking activity LED...");

    const GPFSEL4: usize = GPIO_BASE + 0x10;
    const GPSET1: usize = GPIO_BASE + 0x20;
    const GPCLR1: usize = GPIO_BASE + 0x2C;
    const BLINK_DELAY: u32 = 2_000_000;

    unsafe {
        core::arch::asm!("dsb sy");

        let gpfsel4 = GPFSEL4 as *mut u32;
        let mut val = gpfsel4.read_volatile();
        val &= !(7 << 6);
        val |= 1 << 6;
        gpfsel4.write_volatile(val);

        core::arch::asm!("dsb sy");

        for _ in 0..3 {
            (GPSET1 as *mut u32).write_volatile(1 << 10);
            for _ in 0..BLINK_DELAY { core::hint::spin_loop(); }
            (GPCLR1 as *mut u32).write_volatile(1 << 10);
            for _ in 0..BLINK_DELAY { core::hint::spin_loop(); }
        }

        core::arch::asm!("dsb sy");
    }
    debug_println!("LED done!");
}

/// Delay for frame timing
#[inline]
fn delay(count: u32) {
    for _ in 0..count {
        core::hint::spin_loop();
    }
}

/// Run the snake animation
fn run_animation() {
    debug_println!("Starting snake animation...");
    debug_println!("Screen: {}x{}", WIDTH, HEIGHT);

    let fb = FB_VADDR as *mut u32;
    let mut snake = Snake::new();
    let mut frame: u32 = 0;

    // Frame timing (~30fps)
    const FRAME_DELAY: u32 = 600_000;

    loop {
        unsafe {
            core::arch::asm!("dsb sy");

            // Dark blue background
            clear_screen(fb, 0xFF101030);

            // Draw title
            draw_title(fb);

            // Update and draw snake
            snake.update();
            draw_snake(fb, &snake, frame);

            // Draw frame counter (proof of animation)
            draw_frame_counter(fb, frame);

            // Draw border
            draw_border(fb);

            core::arch::asm!("dsb sy");
            core::arch::asm!("isb");
        }

        frame = frame.wrapping_add(1);

        if frame % 60 == 0 {
            debug_println!("Frame {}", frame);
        }

        delay(FRAME_DELAY);
    }
}

#[protection_domain]
fn init() -> TvDemoHandler {
    debug_println!("");
    debug_println!("========================================");
    debug_println!("  Snake Animation Demo                 ");
    debug_println!("  Screen: {}x{}                        ", WIDTH, HEIGHT);
    debug_println!("========================================");
    debug_println!("");

    blink_activity_led();
    run_animation();

    TvDemoHandler::new()
}

#[derive(Debug)]
pub struct HandlerError;

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TvDemo handler error")
    }
}

impl Handler for TvDemoHandler {
    type Error = HandlerError;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }
}
