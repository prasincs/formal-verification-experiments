//! TV Demo for HDMI on Raspberry Pi 4
//!
//! Displays a snake animation using the EXACT same framebuffer approach
//! as the working graphics demo.

#![no_std]
#![no_main]

use sel4_microkit::{debug_println, protection_domain, Handler, ChannelSet};
use core::fmt;

/// Framebuffer virtual address (same as graphics demo)
const FB_VADDR: usize = 0x5_0001_0000;

/// Screen dimensions - MUST match graphics demo (1280x720)
const WIDTH: usize = 1280;
const HEIGHT: usize = 720;

/// GPIO virtual address
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
    direction: u8,
    frame: u32,
}

impl Snake {
    fn new() -> Self {
        let mut segments = [Segment { x: 0, y: 0 }; 30];
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

        if self.frame % 45 == 0 {
            self.direction = (self.direction + 1) % 4;
        }
        if self.frame % 120 == 0 {
            self.direction = (self.direction + 3) % 4;
        }

        let head = self.segments[0];
        let speed = 10i32;
        let mut new_x = head.x;
        let mut new_y = head.y;

        match self.direction {
            0 => new_x += speed,
            1 => new_y += speed,
            2 => new_x -= speed,
            _ => new_y -= speed,
        }

        // Wrap around
        if new_x < 0 { new_x = WIDTH as i32 - 1; }
        if new_x >= WIDTH as i32 { new_x = 0; }
        if new_y < 0 { new_y = HEIGHT as i32 - 1; }
        if new_y >= HEIGHT as i32 { new_y = 0; }

        for i in (1..self.length).rev() {
            self.segments[i] = self.segments[i - 1];
        }
        self.segments[0] = Segment { x: new_x, y: new_y };
    }
}

struct TvDemoHandler;

impl TvDemoHandler {
    const fn new() -> Self { Self }
}

/// Draw a filled block - EXACT same pattern as graphics demo main.rs line 546
#[inline]
unsafe fn draw_block(fb: *mut u32, pitch: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for dy in 0..h {
        for dx in 0..w {
            fb.add((y + dy) * pitch + (x + dx)).write_volatile(color);
        }
    }
}

/// HSV to RGB
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

/// Clear screen - uses pitch-based calculation like graphics demo
unsafe fn clear_screen(fb: *mut u32, pitch: usize, height: usize, color: u32) {
    for y in 0..height {
        for x in 0..pitch {
            fb.add(y * pitch + x).write_volatile(color);
        }
    }
}

/// Draw snake
unsafe fn draw_snake(fb: *mut u32, pitch: usize, snake: &Snake, frame: u32) {
    let segment_size = 20usize;

    for i in 0..snake.length {
        let seg = snake.segments[i];
        if seg.x >= 0 && seg.y >= 0 {
            let x = (seg.x as usize).saturating_sub(segment_size / 2);
            let y = (seg.y as usize).saturating_sub(segment_size / 2);

            if x + segment_size < pitch && y + segment_size < HEIGHT {
                let hue = ((i as u32 * 18 + frame * 4) % 360) as u16;
                let color = hsv_to_rgb(hue, 255, 255);
                draw_block(fb, pitch, x, y, segment_size, segment_size, color);
            }
        }
    }

    // Eyes
    let head = snake.segments[0];
    if head.x >= 10 && head.y >= 10 && (head.x as usize) < pitch - 10 && (head.y as usize) < HEIGHT - 10 {
        let hx = head.x as usize;
        let hy = head.y as usize;
        let white = 0xFFFFFFFF;
        let black = 0xFF000000;

        let (e1x, e1y, e2x, e2y) = match snake.direction {
            0 => (hx + 5, hy.saturating_sub(5), hx + 5, hy + 5),
            1 => (hx.saturating_sub(5), hy + 5, hx + 5, hy + 5),
            2 => (hx.saturating_sub(5), hy.saturating_sub(5), hx.saturating_sub(5), hy + 5),
            _ => (hx.saturating_sub(5), hy.saturating_sub(5), hx + 5, hy.saturating_sub(5)),
        };

        draw_block(fb, pitch, e1x, e1y, 6, 6, white);
        draw_block(fb, pitch, e2x, e2y, 6, 6, white);
        draw_block(fb, pitch, e1x + 2, e1y + 2, 2, 2, black);
        draw_block(fb, pitch, e2x + 2, e2y + 2, 2, 2, black);
    }
}

/// Draw "SNAKE" title - same block letter style as graphics demo "SEL4"
unsafe fn draw_title(fb: *mut u32, pitch: usize) {
    let white = 0xFFFFFFFF;
    let block = 15usize;
    let start_x = 350usize;
    let start_y = 50usize;

    // S
    draw_block(fb, pitch, start_x, start_y, block * 3, block, white);
    draw_block(fb, pitch, start_x, start_y + block, block, block, white);
    draw_block(fb, pitch, start_x, start_y + block * 2, block * 3, block, white);
    draw_block(fb, pitch, start_x + block * 2, start_y + block * 3, block, block, white);
    draw_block(fb, pitch, start_x, start_y + block * 4, block * 3, block, white);

    // N
    let n_x = start_x + block * 5;
    draw_block(fb, pitch, n_x, start_y, block, block * 5, white);
    draw_block(fb, pitch, n_x + block, start_y + block, block, block, white);
    draw_block(fb, pitch, n_x + block * 2, start_y, block, block * 5, white);

    // A
    let a_x = start_x + block * 9;
    draw_block(fb, pitch, a_x, start_y, block * 3, block, white);
    draw_block(fb, pitch, a_x, start_y + block, block, block * 4, white);
    draw_block(fb, pitch, a_x + block * 2, start_y + block, block, block * 4, white);
    draw_block(fb, pitch, a_x, start_y + block * 2, block * 3, block, white);

    // K
    let k_x = start_x + block * 14;
    draw_block(fb, pitch, k_x, start_y, block, block * 5, white);
    draw_block(fb, pitch, k_x + block, start_y + block * 2, block, block, white);
    draw_block(fb, pitch, k_x + block * 2, start_y, block, block * 2, white);
    draw_block(fb, pitch, k_x + block * 2, start_y + block * 3, block, block * 2, white);

    // E
    let e_x = start_x + block * 18;
    draw_block(fb, pitch, e_x, start_y, block * 3, block, white);
    draw_block(fb, pitch, e_x, start_y + block, block, block, white);
    draw_block(fb, pitch, e_x, start_y + block * 2, block * 2, block, white);
    draw_block(fb, pitch, e_x, start_y + block * 3, block, block, white);
    draw_block(fb, pitch, e_x, start_y + block * 4, block * 3, block, white);
}

/// Draw frame counter bar
unsafe fn draw_frame_counter(fb: *mut u32, pitch: usize, frame: u32) {
    let green = 0xFF00FF00;
    let x_base = 50usize;
    let y_base = 650usize;
    let bar_width = ((frame % 200) as usize) + 10;
    draw_block(fb, pitch, x_base, y_base, bar_width.min(300), 20, green);
}

/// Draw border - same approach as graphics demo
unsafe fn draw_border(fb: *mut u32, pitch: usize, height: usize) {
    let gray = 0xFF808080;
    // Top and bottom
    for x in 0..pitch {
        fb.add(x).write_volatile(gray);
        fb.add((height - 1) * pitch + x).write_volatile(gray);
    }
    // Left and right
    for y in 0..height {
        fb.add(y * pitch).write_volatile(gray);
        fb.add(y * pitch + pitch - 1).write_volatile(gray);
    }
}

/// Blink LED
fn blink_activity_led() {
    debug_println!("Blinking LED...");
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

#[inline]
fn delay(count: u32) {
    for _ in 0..count { core::hint::spin_loop(); }
}

/// Run animation
fn run_animation() {
    debug_println!("Starting animation: {}x{}", WIDTH, HEIGHT);

    let fb = FB_VADDR as *mut u32;
    let mut snake = Snake::new();
    let mut frame: u32 = 0;
    const FRAME_DELAY: u32 = 800_000;

    loop {
        unsafe {
            core::arch::asm!("dsb sy");

            // Clear - use pitch-based calculation
            clear_screen(fb, WIDTH, HEIGHT, 0xFF101030);

            // Draw elements
            draw_title(fb, WIDTH);
            snake.update();
            draw_snake(fb, WIDTH, &snake, frame);
            draw_frame_counter(fb, WIDTH, frame);
            draw_border(fb, WIDTH, HEIGHT);

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
    debug_println!("  Snake Demo - {}x{}", WIDTH, HEIGHT);
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
        write!(f, "Handler error")
    }
}

impl Handler for TvDemoHandler {
    type Error = HandlerError;
    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> { Ok(()) }
}
