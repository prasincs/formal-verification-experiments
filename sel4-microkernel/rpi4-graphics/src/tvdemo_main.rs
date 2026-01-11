//! TV Demo for HDMI on Raspberry Pi 4
//!
//! Displays a simple snake animation on HDMI using direct framebuffer access.
//! No menu system - just pure animation to verify display works.

#![no_std]
#![no_main]

use sel4_microkit::{debug_println, protection_domain, Handler, ChannelSet};
use core::fmt;

/// Framebuffer virtual address (mapped in tvdemo.system)
const FB_VADDR: usize = 0x5_0001_0000;

/// Screen dimensions from config.txt (hdmi_mode=82 = 1920x1080)
const SCREEN_WIDTH: u32 = 1920;
const SCREEN_HEIGHT: u32 = 1080;

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
    segments: [Segment; 50],
    length: usize,
    direction: u8, // 0=right, 1=down, 2=left, 3=up
    frame: u32,
}

impl Snake {
    fn new() -> Self {
        let mut segments = [Segment { x: 0, y: 0 }; 50];
        // Start in center, horizontal line
        let start_x = (SCREEN_WIDTH / 2) as i32;
        let start_y = (SCREEN_HEIGHT / 2) as i32;
        for i in 0..20 {
            segments[i] = Segment {
                x: start_x - (i as i32 * 20),
                y: start_y,
            };
        }
        Self {
            segments,
            length: 20,
            direction: 0, // Start moving right
            frame: 0,
        }
    }

    fn update(&mut self) {
        self.frame = self.frame.wrapping_add(1);

        // Change direction periodically to create interesting patterns
        if self.frame % 60 == 0 {
            // Turn right
            self.direction = (self.direction + 1) % 4;
        }
        if self.frame % 150 == 0 {
            // Sometimes turn left instead
            self.direction = (self.direction + 3) % 4;
        }

        // Calculate new head position
        let head = self.segments[0];
        let speed = 15i32;
        let new_head = match self.direction {
            0 => Segment { x: head.x + speed, y: head.y }, // right
            1 => Segment { x: head.x, y: head.y + speed }, // down
            2 => Segment { x: head.x - speed, y: head.y }, // left
            _ => Segment { x: head.x, y: head.y - speed }, // up
        };

        // Wrap around screen edges
        let new_head = Segment {
            x: if new_head.x < 0 {
                SCREEN_WIDTH as i32 - 1
            } else if new_head.x >= SCREEN_WIDTH as i32 {
                0
            } else {
                new_head.x
            },
            y: if new_head.y < 0 {
                SCREEN_HEIGHT as i32 - 1
            } else if new_head.y >= SCREEN_HEIGHT as i32 {
                0
            } else {
                new_head.y
            },
        };

        // Move all segments (tail follows head)
        for i in (1..self.length).rev() {
            self.segments[i] = self.segments[i - 1];
        }
        self.segments[0] = new_head;
    }
}

struct TvDemoHandler {
    frame_count: u32,
}

impl TvDemoHandler {
    const fn new() -> Self {
        Self { frame_count: 0 }
    }
}

/// Convert RGB to ARGB u32
#[inline]
fn rgb(r: u8, g: u8, b: u8) -> u32 {
    0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// HSV to RGB conversion for rainbow colors
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

    rgb((r + m) as u8, (g + m) as u8, (b + m) as u8)
}

/// Fill a rectangle
#[inline]
unsafe fn fill_rect(x: i32, y: i32, w: u32, h: u32, color: u32) {
    if x < 0 || y < 0 || x >= SCREEN_WIDTH as i32 || y >= SCREEN_HEIGHT as i32 {
        return;
    }
    let fb = FB_VADDR as *mut u32;
    let x = x as u32;
    let y = y as u32;
    let x_end = (x + w).min(SCREEN_WIDTH);
    let y_end = (y + h).min(SCREEN_HEIGHT);

    for py in y..y_end {
        for px in x..x_end {
            fb.add((py * SCREEN_WIDTH + px) as usize).write_volatile(color);
        }
    }
}

/// Clear screen
#[inline]
unsafe fn clear_screen(color: u32) {
    let fb = FB_VADDR as *mut u32;
    for i in 0..(SCREEN_WIDTH * SCREEN_HEIGHT) as usize {
        fb.add(i).write_volatile(color);
    }
}

/// Draw the snake
unsafe fn draw_snake(snake: &Snake, frame: u32) {
    // Draw each segment with rainbow gradient
    let segment_size = 18u32;

    for i in 0..snake.length {
        let seg = snake.segments[i];
        // Rainbow color based on segment position and frame for animation
        let hue = ((i as u32 * 15 + frame * 3) % 360) as u16;
        let color = hsv_to_rgb(hue, 255, 255);

        fill_rect(
            seg.x - (segment_size as i32 / 2),
            seg.y - (segment_size as i32 / 2),
            segment_size,
            segment_size,
            color,
        );
    }

    // Draw eyes on the head
    let head = snake.segments[0];
    let eye_color = rgb(255, 255, 255);
    let pupil_color = rgb(0, 0, 0);

    // Position eyes based on direction
    let (eye1_x, eye1_y, eye2_x, eye2_y) = match snake.direction {
        0 => (head.x + 4, head.y - 4, head.x + 4, head.y + 4),  // right
        1 => (head.x - 4, head.y + 4, head.x + 4, head.y + 4),  // down
        2 => (head.x - 4, head.y - 4, head.x - 4, head.y + 4),  // left
        _ => (head.x - 4, head.y - 4, head.x + 4, head.y - 4),  // up
    };

    fill_rect(eye1_x - 3, eye1_y - 3, 6, 6, eye_color);
    fill_rect(eye2_x - 3, eye2_y - 3, 6, 6, eye_color);
    fill_rect(eye1_x - 1, eye1_y - 1, 3, 3, pupil_color);
    fill_rect(eye2_x - 1, eye2_y - 1, 3, 3, pupil_color);
}

/// Draw frame counter in corner
unsafe fn draw_frame_counter(frame: u32) {
    // Simple digit display using rectangles
    let x_base = 50;
    let y_base = 50;
    let digit_w = 20;
    let digit_h = 30;
    let spacing = 25;

    // Show last 4 digits of frame count
    let mut n = frame % 10000;
    for i in 0..4 {
        let digit = (n % 10) as usize;
        n /= 10;
        let x = x_base + (3 - i as i32) * spacing;

        // Simple 7-segment style digit
        let color = rgb(100, 255, 100);
        let seg_w = digit_w;
        let seg_h = 4;

        // Segments: top, middle, bottom (horizontal)
        let top = digit == 0 || digit == 2 || digit == 3 || digit == 5 || digit == 6 || digit == 7 || digit == 8 || digit == 9;
        let mid = digit == 2 || digit == 3 || digit == 4 || digit == 5 || digit == 6 || digit == 8 || digit == 9;
        let bot = digit == 0 || digit == 2 || digit == 3 || digit == 5 || digit == 6 || digit == 8 || digit == 9;

        // Segments: top-left, top-right, bot-left, bot-right (vertical)
        let tl = digit == 0 || digit == 4 || digit == 5 || digit == 6 || digit == 8 || digit == 9;
        let tr = digit == 0 || digit == 1 || digit == 2 || digit == 3 || digit == 4 || digit == 7 || digit == 8 || digit == 9;
        let bl = digit == 0 || digit == 2 || digit == 6 || digit == 8;
        let br = digit == 0 || digit == 1 || digit == 3 || digit == 4 || digit == 5 || digit == 6 || digit == 7 || digit == 8 || digit == 9;

        if top { fill_rect(x, y_base, seg_w as u32, seg_h as u32, color); }
        if mid { fill_rect(x, y_base + digit_h / 2, seg_w as u32, seg_h as u32, color); }
        if bot { fill_rect(x, y_base + digit_h, seg_w as u32, seg_h as u32, color); }
        if tl { fill_rect(x, y_base, seg_h as u32, (digit_h / 2) as u32, color); }
        if tr { fill_rect(x + digit_w - seg_h, y_base, seg_h as u32, (digit_h / 2) as u32, color); }
        if bl { fill_rect(x, y_base + digit_h / 2, seg_h as u32, (digit_h / 2 + seg_h) as u32, color); }
        if br { fill_rect(x + digit_w - seg_h, y_base + digit_h / 2, seg_h as u32, (digit_h / 2 + seg_h) as u32, color); }
    }
}

/// Blink the activity LED to prove seL4 is running
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
    debug_println!("Screen: {}x{}", SCREEN_WIDTH, SCREEN_HEIGHT);

    let mut snake = Snake::new();
    let mut frame: u32 = 0;

    // Animation timing (~30fps)
    const FRAME_DELAY: u32 = 800_000;

    loop {
        unsafe {
            core::arch::asm!("dsb sy");

            // Clear to dark blue
            clear_screen(rgb(10, 10, 40));

            // Update and draw snake
            snake.update();
            draw_snake(&snake, frame);

            // Draw frame counter
            draw_frame_counter(frame);

            // Draw border
            let border_color = rgb(100, 100, 100);
            for x in 0..SCREEN_WIDTH {
                let fb = FB_VADDR as *mut u32;
                fb.add(x as usize).write_volatile(border_color);
                fb.add(((SCREEN_HEIGHT - 1) * SCREEN_WIDTH + x) as usize).write_volatile(border_color);
            }
            for y in 0..SCREEN_HEIGHT {
                let fb = FB_VADDR as *mut u32;
                fb.add((y * SCREEN_WIDTH) as usize).write_volatile(border_color);
                fb.add((y * SCREEN_WIDTH + SCREEN_WIDTH - 1) as usize).write_volatile(border_color);
            }

            core::arch::asm!("dsb sy");
            core::arch::asm!("isb");
        }

        frame = frame.wrapping_add(1);

        // Print progress every 100 frames
        if frame % 100 == 0 {
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
        self.frame_count += 1;
        Ok(())
    }
}
