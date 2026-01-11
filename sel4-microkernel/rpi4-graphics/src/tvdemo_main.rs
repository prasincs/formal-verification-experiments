//! TV Demo for HDMI on Raspberry Pi 4
//!
//! Displays a snake animation using proper mailbox-based framebuffer allocation.
//! The GPU dynamically allocates the framebuffer and returns the address.

#![no_std]
#![no_main]

use sel4_microkit::{debug_println, protection_domain, Handler, ChannelSet};
use core::fmt;

use rpi4_graphics::{Mailbox, Framebuffer, MAILBOX_BASE};

/// Screen dimensions
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// GPIO virtual address
const GPIO_BASE: usize = 0x5_0200_0000;

struct TvDemoHandler;

impl TvDemoHandler {
    const fn new() -> Self { Self }
}

/// Draw a filled block using direct writes for animation performance
#[inline]
unsafe fn draw_block(fb: *mut u32, pitch: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for dy in 0..h {
        for dx in 0..w {
            fb.add((y + dy) * pitch + (x + dx)).write_volatile(color);
        }
    }
}

/// Blink LED to prove seL4 is running
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

/// Initialize framebuffer via VideoCore mailbox
fn init_framebuffer() -> Option<Framebuffer> {
    debug_println!("Initializing framebuffer via mailbox...");

    let mailbox = unsafe { Mailbox::new(MAILBOX_BASE) };

    // Query board info
    let mut buf = [0u32; 36];
    match mailbox.get_firmware_revision(&mut buf) {
        Ok(rev) => debug_println!("Firmware revision: 0x{:08x}", rev),
        Err(_) => debug_println!("Failed to get firmware revision"),
    }

    match mailbox.get_board_model(&mut buf) {
        Ok(model) => debug_println!("Board model: 0x{:08x}", model),
        Err(_) => debug_println!("Failed to get board model"),
    }

    // Allocate framebuffer
    match unsafe { Framebuffer::new(&mailbox, WIDTH, HEIGHT) } {
        Ok(fb) => {
            let info = fb.info();
            debug_println!(
                "Framebuffer allocated: {}x{} @ phys 0x{:08x}, pitch={}",
                info.width, info.height, info.base, info.pitch
            );
            Some(fb)
        }
        Err(e) => {
            debug_println!("Failed to allocate framebuffer: {:?}", e);
            None
        }
    }
}

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
    fn new(width: usize, height: usize) -> Self {
        let mut segments = [Segment { x: 0, y: 0 }; 30];
        let start_x = (width / 2) as i32;
        let start_y = (height / 2) as i32;
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

    fn update(&mut self, width: usize, height: usize) {
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
        if new_x < 0 { new_x = width as i32 - 1; }
        if new_x >= width as i32 { new_x = 0; }
        if new_y < 0 { new_y = height as i32 - 1; }
        if new_y >= height as i32 { new_y = 0; }

        for i in (1..self.length).rev() {
            self.segments[i] = self.segments[i - 1];
        }
        self.segments[0] = Segment { x: new_x, y: new_y };
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

/// Run snake animation using the properly allocated framebuffer
fn run_animation(fb: &Framebuffer) {
    let ptr = fb.buffer_ptr();
    let pitch = fb.pitch_pixels();
    let (width, height) = fb.dimensions();
    let width = width as usize;
    let height = height as usize;

    debug_println!("Starting snake animation: {}x{}, pitch={}", width, height, pitch);

    let mut snake = Snake::new(width, height);
    let mut frame: u32 = 0;
    const FRAME_DELAY: u32 = 800_000;

    loop {
        unsafe {
            core::arch::asm!("dsb sy");

            // Clear with dark blue
            let bg_color: u32 = 0xFF101030;
            for y in 0..height {
                for x in 0..pitch {
                    ptr.add(y * pitch + x).write_volatile(bg_color);
                }
            }

            // Draw "SNAKE" title
            let white: u32 = 0xFFFFFFFF;
            let block = 15usize;
            let start_x = 350usize;
            let start_y = 50usize;

            // S
            draw_block(ptr, pitch, start_x, start_y, block * 3, block, white);
            draw_block(ptr, pitch, start_x, start_y + block, block, block, white);
            draw_block(ptr, pitch, start_x, start_y + block * 2, block * 3, block, white);
            draw_block(ptr, pitch, start_x + block * 2, start_y + block * 3, block, block, white);
            draw_block(ptr, pitch, start_x, start_y + block * 4, block * 3, block, white);

            // N
            let n_x = start_x + block * 5;
            draw_block(ptr, pitch, n_x, start_y, block, block * 5, white);
            draw_block(ptr, pitch, n_x + block, start_y + block, block, block, white);
            draw_block(ptr, pitch, n_x + block * 2, start_y, block, block * 5, white);

            // A
            let a_x = start_x + block * 9;
            draw_block(ptr, pitch, a_x, start_y, block * 3, block, white);
            draw_block(ptr, pitch, a_x, start_y + block, block, block * 4, white);
            draw_block(ptr, pitch, a_x + block * 2, start_y + block, block, block * 4, white);
            draw_block(ptr, pitch, a_x, start_y + block * 2, block * 3, block, white);

            // K
            let k_x = start_x + block * 14;
            draw_block(ptr, pitch, k_x, start_y, block, block * 5, white);
            draw_block(ptr, pitch, k_x + block, start_y + block * 2, block, block, white);
            draw_block(ptr, pitch, k_x + block * 2, start_y, block, block * 2, white);
            draw_block(ptr, pitch, k_x + block * 2, start_y + block * 3, block, block * 2, white);

            // E
            let e_x = start_x + block * 18;
            draw_block(ptr, pitch, e_x, start_y, block * 3, block, white);
            draw_block(ptr, pitch, e_x, start_y + block, block, block, white);
            draw_block(ptr, pitch, e_x, start_y + block * 2, block * 2, block, white);
            draw_block(ptr, pitch, e_x, start_y + block * 3, block, block, white);
            draw_block(ptr, pitch, e_x, start_y + block * 4, block * 3, block, white);

            // Update and draw snake
            snake.update(width, height);
            let segment_size = 20usize;
            for i in 0..snake.length {
                let seg = snake.segments[i];
                if seg.x >= 0 && seg.y >= 0 {
                    let x = (seg.x as usize).saturating_sub(segment_size / 2);
                    let y = (seg.y as usize).saturating_sub(segment_size / 2);
                    if x + segment_size < width && y + segment_size < height {
                        let hue = ((i as u32 * 18 + frame * 4) % 360) as u16;
                        let color = hsv_to_rgb(hue, 255, 255);
                        draw_block(ptr, pitch, x, y, segment_size, segment_size, color);
                    }
                }
            }

            // Draw frame counter bar
            let green: u32 = 0xFF00FF00;
            let bar_width = ((frame % 200) as usize) + 10;
            draw_block(ptr, pitch, 50, 650, bar_width.min(300), 20, green);

            // Draw border
            let gray: u32 = 0xFF808080;
            for x in 0..width {
                ptr.add(x).write_volatile(gray);
                ptr.add((height - 1) * pitch + x).write_volatile(gray);
            }
            for y in 0..height {
                ptr.add(y * pitch).write_volatile(gray);
                ptr.add(y * pitch + width - 1).write_volatile(gray);
            }

            core::arch::asm!("dsb sy");
            core::arch::asm!("isb");
        }

        frame = frame.wrapping_add(1);
        if frame % 60 == 0 {
            debug_println!("Frame {}", frame);
        }

        for _ in 0..FRAME_DELAY { core::hint::spin_loop(); }
    }
}

#[protection_domain]
fn init() -> TvDemoHandler {
    debug_println!("");
    debug_println!("========================================");
    debug_println!("  TV Demo - Mailbox Framebuffer Init");
    debug_println!("========================================");
    debug_println!("");

    // Step 1: Blink LED (proves seL4 is running)
    blink_activity_led();

    // Step 2: Initialize framebuffer via VideoCore mailbox
    match init_framebuffer() {
        Some(fb) => {
            debug_println!("Framebuffer ready, starting animation...");
            run_animation(&fb);
        }
        None => {
            debug_println!("ERROR: Could not allocate framebuffer!");
            debug_println!("Check mailbox communication and memory mappings.");
            // Blink LED rapidly to indicate error
            loop {
                blink_activity_led();
            }
        }
    }

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
