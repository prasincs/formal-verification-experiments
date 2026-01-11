//! TV Demo for HDMI on Raspberry Pi 4
//!
//! Interactive demo with menu system using UART input.
//! Features:
//! - Snake Game (interactive)
//! - Snake Screensaver (automatic)
//! - Settings and About screens
//!
//! The GPU dynamically allocates the framebuffer via VideoCore mailbox.

#![no_std]
#![no_main]

use sel4_microkit::{debug_println, protection_domain, Handler, ChannelSet};
use core::fmt;

use rpi4_graphics::{Mailbox, Framebuffer, MAILBOX_BASE};
use rpi4_input::{InputManager, RemoteOptions, InputEvent, KeyCode, KeyState};

/// Screen dimensions
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// GPIO virtual address
const GPIO_BASE: usize = 0x5_0200_0000;

/// UART virtual address (mapped by Microkit at 0x5_0400_0000, mini-UART at +0x40)
const UART_VADDR: usize = 0x5_0400_0000 + 0x40;

/// Application state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppState {
    /// Main menu
    Menu,
    /// Interactive snake game
    SnakeGame,
    /// Automatic snake screensaver
    Screensaver,
    /// About screen
    About,
}

/// Menu item indices
const MENU_SNAKE_GAME: usize = 0;
const MENU_SCREENSAVER: usize = 1;
const MENU_ABOUT: usize = 2;
const MENU_ITEM_COUNT: usize = 3;

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

/// Play area bounds (snake stays within this region, below title)
const PLAY_AREA_TOP: i32 = 140;  // Below title (title ends around y=105)
const PLAY_AREA_BOTTOM: i32 = 630;  // Above frame counter (at y=650)
const PLAY_AREA_LEFT: i32 = 20;
const PLAY_AREA_RIGHT: i32 = 1260;

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
        // Start in center of play area
        let start_x = (PLAY_AREA_LEFT + PLAY_AREA_RIGHT) / 2;
        let start_y = (PLAY_AREA_TOP + PLAY_AREA_BOTTOM) / 2;
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

    /// Set direction manually (for interactive game mode)
    fn set_direction(&mut self, dir: u8) {
        self.direction = dir % 4;
    }

    /// Update snake with automatic direction changes (screensaver mode)
    fn update(&mut self) {
        self.frame = self.frame.wrapping_add(1);

        // Auto-turn for screensaver effect
        if self.frame % 45 == 0 {
            self.direction = (self.direction + 1) % 4;
        }
        if self.frame % 120 == 0 {
            self.direction = (self.direction + 3) % 4;
        }

        self.move_forward();
    }

    /// Update snake without automatic direction changes (game mode)
    fn update_no_auto_turn(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.move_forward();
    }

    /// Move the snake forward in current direction
    fn move_forward(&mut self) {
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

        // Wrap around within play area
        if new_x < PLAY_AREA_LEFT { new_x = PLAY_AREA_RIGHT - 1; }
        if new_x >= PLAY_AREA_RIGHT { new_x = PLAY_AREA_LEFT; }
        if new_y < PLAY_AREA_TOP { new_y = PLAY_AREA_BOTTOM - 1; }
        if new_y >= PLAY_AREA_BOTTOM { new_y = PLAY_AREA_TOP; }

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

/// Draw static elements (title, border) - called once
unsafe fn draw_static_elements(ptr: *mut u32, pitch: usize, width: usize, height: usize) {
    let white: u32 = 0xFFFFFFFF;
    let green: u32 = 0xFF00B050;  // seL4 green for "SeL4"
    let block = 12usize;  // Smaller blocks for more text
    let start_y = 30usize;

    // ===== "SeL4" in seL4 green (left side) =====
    let sel4_x = 80usize;

    // S
    draw_block(ptr, pitch, sel4_x, start_y, block * 3, block, green);
    draw_block(ptr, pitch, sel4_x, start_y + block, block, block, green);
    draw_block(ptr, pitch, sel4_x, start_y + block * 2, block * 3, block, green);
    draw_block(ptr, pitch, sel4_x + block * 2, start_y + block * 3, block, block, green);
    draw_block(ptr, pitch, sel4_x, start_y + block * 4, block * 3, block, green);

    // e (lowercase - smaller)
    let e_x = sel4_x + block * 4;
    draw_block(ptr, pitch, e_x, start_y + block, block * 2, block, green);      // top bar
    draw_block(ptr, pitch, e_x, start_y + block * 2, block, block, green);      // left vertical
    draw_block(ptr, pitch, e_x + block, start_y + block * 2, block, block, green); // right vertical (upper)
    draw_block(ptr, pitch, e_x, start_y + block * 3, block * 2, block, green);  // middle bar
    draw_block(ptr, pitch, e_x, start_y + block * 4, block * 2, block, green);  // bottom bar

    // L
    let l_x = sel4_x + block * 7;
    draw_block(ptr, pitch, l_x, start_y, block, block * 4, green);
    draw_block(ptr, pitch, l_x, start_y + block * 4, block * 3, block, green);

    // 4
    let four_x = sel4_x + block * 11;
    draw_block(ptr, pitch, four_x, start_y, block, block * 3, green);
    draw_block(ptr, pitch, four_x, start_y + block * 2, block * 3, block, green);
    draw_block(ptr, pitch, four_x + block * 2, start_y, block, block * 5, green);

    // ===== "SNAKE" in white (right side) =====
    let snake_x = 420usize;
    let block = 15usize;  // Larger blocks for SNAKE

    // S
    draw_block(ptr, pitch, snake_x, start_y, block * 3, block, white);
    draw_block(ptr, pitch, snake_x, start_y + block, block, block, white);
    draw_block(ptr, pitch, snake_x, start_y + block * 2, block * 3, block, white);
    draw_block(ptr, pitch, snake_x + block * 2, start_y + block * 3, block, block, white);
    draw_block(ptr, pitch, snake_x, start_y + block * 4, block * 3, block, white);

    // N - 4 blocks wide for proper diagonal
    let n_x = snake_x + block * 4;
    draw_block(ptr, pitch, n_x, start_y, block, block * 5, white);  // Left vertical
    draw_block(ptr, pitch, n_x + block, start_y + block, block, block, white);  // Diagonal step 1
    draw_block(ptr, pitch, n_x + block * 2, start_y + block * 2, block, block * 2, white);  // Diagonal step 2
    draw_block(ptr, pitch, n_x + block * 3, start_y, block, block * 5, white);  // Right vertical

    // A (shifted right by 1 block since N is now wider)
    let a_x = snake_x + block * 9;
    draw_block(ptr, pitch, a_x, start_y, block * 3, block, white);
    draw_block(ptr, pitch, a_x, start_y + block, block, block * 4, white);
    draw_block(ptr, pitch, a_x + block * 2, start_y + block, block, block * 4, white);
    draw_block(ptr, pitch, a_x, start_y + block * 2, block * 3, block, white);

    // K
    let k_x = snake_x + block * 13;
    draw_block(ptr, pitch, k_x, start_y, block, block * 5, white);
    draw_block(ptr, pitch, k_x + block, start_y + block * 2, block, block, white);
    draw_block(ptr, pitch, k_x + block * 2, start_y, block, block * 2, white);
    draw_block(ptr, pitch, k_x + block * 2, start_y + block * 3, block, block * 2, white);

    // E
    let e2_x = snake_x + block * 17;
    draw_block(ptr, pitch, e2_x, start_y, block * 3, block, white);
    draw_block(ptr, pitch, e2_x, start_y + block, block, block, white);
    draw_block(ptr, pitch, e2_x, start_y + block * 2, block * 2, block, white);
    draw_block(ptr, pitch, e2_x, start_y + block * 3, block, block, white);
    draw_block(ptr, pitch, e2_x, start_y + block * 4, block * 3, block, white);

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
}

/// Draw the main menu
unsafe fn draw_menu(ptr: *mut u32, pitch: usize, width: usize, height: usize, selected: usize) {
    let bg_color: u32 = 0xFF101030;
    let white: u32 = 0xFFFFFFFF;
    let green: u32 = 0xFF00B050;
    let gray: u32 = 0xFF808080;

    // Clear menu area
    let menu_top = 200usize;
    let menu_height = 300usize;
    for y in menu_top..(menu_top + menu_height) {
        for x in 200..1080 {
            ptr.add(y * pitch + x).write_volatile(bg_color);
        }
    }

    // Draw menu box
    let box_left = 300usize;
    let box_right = 980usize;
    let box_top = 220usize;
    let box_bottom = 480usize;

    // Box border
    for x in box_left..box_right {
        ptr.add(box_top * pitch + x).write_volatile(gray);
        ptr.add(box_bottom * pitch + x).write_volatile(gray);
    }
    for y in box_top..box_bottom {
        ptr.add(y * pitch + box_left).write_volatile(gray);
        ptr.add(y * pitch + box_right).write_volatile(gray);
    }

    // Menu title "SELECT MODE"
    let title_y = 240usize;
    let title_x = 500usize;
    draw_text_select(ptr, pitch, title_x, title_y, white);

    // Menu items
    let item_height = 50usize;
    let item_start_y = 300usize;

    let items = ["Snake Game", "Screensaver", "About"];
    for (i, _item) in items.iter().enumerate() {
        let y = item_start_y + i * item_height;
        let color = if i == selected { green } else { white };

        // Draw selection indicator
        if i == selected {
            draw_block(ptr, pitch, box_left + 30, y + 10, 20, 20, green);
        }

        // Draw item text (simplified block text)
        match i {
            0 => draw_text_snake_game(ptr, pitch, box_left + 80, y, color),
            1 => draw_text_screensaver(ptr, pitch, box_left + 80, y, color),
            2 => draw_text_about(ptr, pitch, box_left + 80, y, color),
            _ => {}
        }
    }

    // Navigation hint at bottom
    draw_text_hint(ptr, pitch, 400, 520, gray);
}

/// Draw "SELECT" text using blocks
unsafe fn draw_text_select(ptr: *mut u32, pitch: usize, x: usize, y: usize, color: u32) {
    let b = 8usize;  // block size
    // S
    draw_block(ptr, pitch, x, y, b*2, b, color);
    draw_block(ptr, pitch, x, y+b, b, b, color);
    draw_block(ptr, pitch, x, y+b*2, b*2, b, color);
    draw_block(ptr, pitch, x+b, y+b*3, b, b, color);
    draw_block(ptr, pitch, x, y+b*4, b*2, b, color);
    // E
    let ex = x + b*3;
    draw_block(ptr, pitch, ex, y, b*2, b, color);
    draw_block(ptr, pitch, ex, y+b, b, b*3, color);
    draw_block(ptr, pitch, ex, y+b*2, b*2, b, color);
    draw_block(ptr, pitch, ex, y+b*4, b*2, b, color);
    // L
    let lx = x + b*6;
    draw_block(ptr, pitch, lx, y, b, b*4, color);
    draw_block(ptr, pitch, lx, y+b*4, b*2, b, color);
    // E
    let e2x = x + b*9;
    draw_block(ptr, pitch, e2x, y, b*2, b, color);
    draw_block(ptr, pitch, e2x, y+b, b, b*3, color);
    draw_block(ptr, pitch, e2x, y+b*2, b*2, b, color);
    draw_block(ptr, pitch, e2x, y+b*4, b*2, b, color);
    // C
    let cx = x + b*12;
    draw_block(ptr, pitch, cx, y, b*2, b, color);
    draw_block(ptr, pitch, cx, y+b, b, b*3, color);
    draw_block(ptr, pitch, cx, y+b*4, b*2, b, color);
    // T
    let tx = x + b*15;
    draw_block(ptr, pitch, tx, y, b*3, b, color);
    draw_block(ptr, pitch, tx+b, y+b, b, b*4, color);
}

/// Draw a single block letter
unsafe fn draw_letter(ptr: *mut u32, pitch: usize, x: usize, y: usize, b: usize, letter: char, color: u32) {
    match letter {
        'S' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x, y+b, b, b, color);
            draw_block(ptr, pitch, x, y+b*2, b*3, b, color);
            draw_block(ptr, pitch, x+b*2, y+b*3, b, b, color);
            draw_block(ptr, pitch, x, y+b*4, b*3, b, color);
        }
        'N' => {
            draw_block(ptr, pitch, x, y, b, b*5, color);
            draw_block(ptr, pitch, x+b, y+b, b, b*2, color);
            draw_block(ptr, pitch, x+b*2, y, b, b*5, color);
        }
        'A' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x, y+b, b, b*4, color);
            draw_block(ptr, pitch, x+b*2, y+b, b, b*4, color);
            draw_block(ptr, pitch, x, y+b*2, b*3, b, color);
        }
        'K' => {
            draw_block(ptr, pitch, x, y, b, b*5, color);
            draw_block(ptr, pitch, x+b, y+b*2, b, b, color);
            draw_block(ptr, pitch, x+b*2, y, b, b*2, color);
            draw_block(ptr, pitch, x+b*2, y+b*3, b, b*2, color);
        }
        'E' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x, y+b, b, b*3, color);
            draw_block(ptr, pitch, x, y+b*2, b*2, b, color);
            draw_block(ptr, pitch, x, y+b*4, b*3, b, color);
        }
        'G' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x, y+b, b, b*3, color);
            draw_block(ptr, pitch, x, y+b*4, b*3, b, color);
            draw_block(ptr, pitch, x+b*2, y+b*2, b, b*2, color);
            draw_block(ptr, pitch, x+b, y+b*2, b, b, color);
        }
        'M' => {
            draw_block(ptr, pitch, x, y, b, b*5, color);
            draw_block(ptr, pitch, x+b, y+b, b, b, color);
            draw_block(ptr, pitch, x+b*2, y, b, b*5, color);
        }
        'C' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x, y+b, b, b*3, color);
            draw_block(ptr, pitch, x, y+b*4, b*3, b, color);
        }
        'R' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x, y+b, b, b*4, color);
            draw_block(ptr, pitch, x+b*2, y+b, b, b, color);
            draw_block(ptr, pitch, x, y+b*2, b*3, b, color);
            draw_block(ptr, pitch, x+b*2, y+b*3, b, b*2, color);
        }
        'V' => {
            draw_block(ptr, pitch, x, y, b, b*3, color);
            draw_block(ptr, pitch, x+b*2, y, b, b*3, color);
            draw_block(ptr, pitch, x+b, y+b*3, b, b*2, color);
        }
        'B' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x, y+b, b, b*4, color);
            draw_block(ptr, pitch, x+b*2, y+b, b, b, color);
            draw_block(ptr, pitch, x, y+b*2, b*3, b, color);
            draw_block(ptr, pitch, x+b*2, y+b*3, b, b, color);
            draw_block(ptr, pitch, x, y+b*4, b*3, b, color);
        }
        'O' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x, y+b, b, b*3, color);
            draw_block(ptr, pitch, x+b*2, y+b, b, b*3, color);
            draw_block(ptr, pitch, x, y+b*4, b*3, b, color);
        }
        'U' => {
            draw_block(ptr, pitch, x, y, b, b*4, color);
            draw_block(ptr, pitch, x+b*2, y, b, b*4, color);
            draw_block(ptr, pitch, x, y+b*4, b*3, b, color);
        }
        'T' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x+b, y+b, b, b*4, color);
        }
        'I' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x+b, y+b, b, b*3, color);
            draw_block(ptr, pitch, x, y+b*4, b*3, b, color);
        }
        'L' => {
            draw_block(ptr, pitch, x, y, b, b*4, color);
            draw_block(ptr, pitch, x, y+b*4, b*3, b, color);
        }
        'P' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x, y+b, b, b*4, color);
            draw_block(ptr, pitch, x+b*2, y+b, b, b, color);
            draw_block(ptr, pitch, x, y+b*2, b*3, b, color);
        }
        '4' => {
            draw_block(ptr, pitch, x, y, b, b*3, color);
            draw_block(ptr, pitch, x, y+b*2, b*3, b, color);
            draw_block(ptr, pitch, x+b*2, y, b, b*5, color);
        }
        ' ' => {} // space - do nothing
        _ => {} // unknown letter
    }
}

/// Draw "SNAKE GAME" text
unsafe fn draw_text_snake_game(ptr: *mut u32, pitch: usize, x: usize, y: usize, color: u32) {
    let b = 6usize;
    let spacing = b * 4;
    // S N A K E   G A M E
    draw_letter(ptr, pitch, x, y, b, 'S', color);
    draw_letter(ptr, pitch, x + spacing, y, b, 'N', color);
    draw_letter(ptr, pitch, x + spacing*2, y, b, 'A', color);
    draw_letter(ptr, pitch, x + spacing*3, y, b, 'K', color);
    draw_letter(ptr, pitch, x + spacing*4, y, b, 'E', color);
    // gap
    draw_letter(ptr, pitch, x + spacing*6, y, b, 'G', color);
    draw_letter(ptr, pitch, x + spacing*7, y, b, 'A', color);
    draw_letter(ptr, pitch, x + spacing*8, y, b, 'M', color);
    draw_letter(ptr, pitch, x + spacing*9, y, b, 'E', color);
}

/// Draw "SCREENSAVER" text
unsafe fn draw_text_screensaver(ptr: *mut u32, pitch: usize, x: usize, y: usize, color: u32) {
    let b = 5usize;
    let spacing = b * 4;
    // S C R E E N S A V E R
    draw_letter(ptr, pitch, x, y, b, 'S', color);
    draw_letter(ptr, pitch, x + spacing, y, b, 'C', color);
    draw_letter(ptr, pitch, x + spacing*2, y, b, 'R', color);
    draw_letter(ptr, pitch, x + spacing*3, y, b, 'E', color);
    draw_letter(ptr, pitch, x + spacing*4, y, b, 'E', color);
    draw_letter(ptr, pitch, x + spacing*5, y, b, 'N', color);
    draw_letter(ptr, pitch, x + spacing*6, y, b, 'S', color);
    draw_letter(ptr, pitch, x + spacing*7, y, b, 'A', color);
    draw_letter(ptr, pitch, x + spacing*8, y, b, 'V', color);
    draw_letter(ptr, pitch, x + spacing*9, y, b, 'E', color);
    draw_letter(ptr, pitch, x + spacing*10, y, b, 'R', color);
}

/// Draw "ABOUT" text
unsafe fn draw_text_about(ptr: *mut u32, pitch: usize, x: usize, y: usize, color: u32) {
    let b = 6usize;
    let spacing = b * 4;
    // A B O U T
    draw_letter(ptr, pitch, x, y, b, 'A', color);
    draw_letter(ptr, pitch, x + spacing, y, b, 'B', color);
    draw_letter(ptr, pitch, x + spacing*2, y, b, 'O', color);
    draw_letter(ptr, pitch, x + spacing*3, y, b, 'U', color);
    draw_letter(ptr, pitch, x + spacing*4, y, b, 'T', color);
}

/// Draw navigation hint - simplified arrow indicators
unsafe fn draw_text_hint(ptr: *mut u32, pitch: usize, x: usize, y: usize, color: u32) {
    let b = 4usize;
    // Up/Down arrows indicator
    draw_block(ptr, pitch, x, y, b, b, color);
    draw_block(ptr, pitch, x+b, y+b, b, b, color);
    draw_block(ptr, pitch, x, y+b*2, b, b, color);
    // "NAVIGATE" text
    let spacing = b * 4;
    let tx = x + 40;
    draw_letter(ptr, pitch, tx, y, b, 'N', color);
    draw_letter(ptr, pitch, tx + spacing, y, b, 'A', color);
    draw_letter(ptr, pitch, tx + spacing*2, y, b, 'V', color);
}

/// Draw About screen
unsafe fn draw_about_screen(ptr: *mut u32, pitch: usize, _width: usize, _height: usize) {
    let bg_color: u32 = 0xFF101030;
    let white: u32 = 0xFFFFFFFF;
    let green: u32 = 0xFF00B050;

    // Clear screen
    for y in 150..600 {
        for x in 100..1180 {
            ptr.add(y * pitch + x).write_volatile(bg_color);
        }
    }

    // Title "ABOUT"
    let b = 10usize;
    let spacing = b * 4;
    let title_x = 520usize;
    let title_y = 180usize;
    draw_letter(ptr, pitch, title_x, title_y, b, 'A', green);
    draw_letter(ptr, pitch, title_x + spacing, title_y, b, 'B', green);
    draw_letter(ptr, pitch, title_x + spacing*2, title_y, b, 'O', green);
    draw_letter(ptr, pitch, title_x + spacing*3, title_y, b, 'U', green);
    draw_letter(ptr, pitch, title_x + spacing*4, title_y, b, 'T', green);

    // "SEL4" line
    let b = 6usize;
    let spacing = b * 4;
    let line_x = 300usize;
    draw_letter(ptr, pitch, line_x, 280, b, 'S', white);
    draw_letter(ptr, pitch, line_x + spacing, 280, b, 'E', white);
    draw_letter(ptr, pitch, line_x + spacing*2, 280, b, 'L', white);
    draw_letter(ptr, pitch, line_x + spacing*3, 280, b, '4', white);
    // "MICROKIT"
    let mk_x = line_x + spacing*5;
    draw_letter(ptr, pitch, mk_x, 280, b, 'M', white);
    draw_letter(ptr, pitch, mk_x + spacing, 280, b, 'I', white);
    draw_letter(ptr, pitch, mk_x + spacing*2, 280, b, 'C', white);
    draw_letter(ptr, pitch, mk_x + spacing*3, 280, b, 'R', white);
    draw_letter(ptr, pitch, mk_x + spacing*4, 280, b, 'O', white);
    draw_letter(ptr, pitch, mk_x + spacing*5, 280, b, 'K', white);
    draw_letter(ptr, pitch, mk_x + spacing*6, 280, b, 'I', white);
    draw_letter(ptr, pitch, mk_x + spacing*7, 280, b, 'T', white);

    // "RPI4" line
    draw_letter(ptr, pitch, line_x, 340, b, 'R', white);
    draw_letter(ptr, pitch, line_x + spacing, 340, b, 'P', white);
    draw_letter(ptr, pitch, line_x + spacing*2, 340, b, 'I', white);
    draw_letter(ptr, pitch, line_x + spacing*3, 340, b, '4', white);

    // "PRESS ESC" line
    let esc_x = line_x;
    draw_letter(ptr, pitch, esc_x, 420, b, 'P', white);
    draw_letter(ptr, pitch, esc_x + spacing, 420, b, 'R', white);
    draw_letter(ptr, pitch, esc_x + spacing*2, 420, b, 'E', white);
    draw_letter(ptr, pitch, esc_x + spacing*3, 420, b, 'S', white);
    draw_letter(ptr, pitch, esc_x + spacing*4, 420, b, 'S', white);
    // ESC
    let esc2_x = esc_x + spacing*6;
    draw_letter(ptr, pitch, esc2_x, 420, b, 'E', white);
    draw_letter(ptr, pitch, esc2_x + spacing, 420, b, 'S', white);
    draw_letter(ptr, pitch, esc2_x + spacing*2, 420, b, 'C', white);
}

/// Run the main application loop with menu and state machine
fn run_app(fb: &Framebuffer) {
    let ptr = fb.buffer_ptr();
    let pitch = fb.pitch_pixels();
    let (width, height) = fb.dimensions();
    let width = width as usize;
    let height = height as usize;

    debug_println!("Starting app with UART input: {}x{}, pitch={}", width, height, pitch);

    // Initialize input manager with UART at mapped virtual address
    let mut input = InputManager::new(RemoteOptions::uart_at(UART_VADDR));

    let bg_color: u32 = 0xFF101030;

    // Application state
    let mut state = AppState::Menu;
    let mut menu_selected: usize = 0;
    let mut needs_redraw = true;

    // Snake state (for game and screensaver)
    let mut snake = Snake::new();
    let mut prev_segments: [Segment; 30] = [Segment { x: -100, y: -100 }; 30];
    let mut frame: u32 = 0;
    let segment_size = 20usize;

    // Clear screen once
    unsafe {
        core::arch::asm!("dsb sy");
        for y in 0..height {
            for x in 0..pitch {
                ptr.add(y * pitch + x).write_volatile(bg_color);
            }
        }
        core::arch::asm!("dsb sy");
    }

    debug_println!("Entering main loop. Use WASD/arrows to navigate, Enter to select, Q to quit.");

    loop {
        // Poll for input
        if let Some(event) = input.poll() {
            if let InputEvent::Key(key_event) = event {
                if key_event.state == KeyState::Pressed {
                    debug_println!("Key pressed: {:?}", key_event.key);

                    match state {
                        AppState::Menu => {
                            match key_event.key {
                                KeyCode::Up => {
                                    if menu_selected > 0 {
                                        menu_selected -= 1;
                                        needs_redraw = true;
                                    }
                                }
                                KeyCode::Down => {
                                    if menu_selected < MENU_ITEM_COUNT - 1 {
                                        menu_selected += 1;
                                        needs_redraw = true;
                                    }
                                }
                                KeyCode::Enter | KeyCode::Space => {
                                    match menu_selected {
                                        MENU_SNAKE_GAME => {
                                            state = AppState::SnakeGame;
                                            snake = Snake::new();
                                            needs_redraw = true;
                                            debug_println!("Starting Snake Game");
                                        }
                                        MENU_SCREENSAVER => {
                                            state = AppState::Screensaver;
                                            snake = Snake::new();
                                            needs_redraw = true;
                                            debug_println!("Starting Screensaver");
                                        }
                                        MENU_ABOUT => {
                                            state = AppState::About;
                                            needs_redraw = true;
                                            debug_println!("Showing About");
                                        }
                                        _ => {}
                                    }
                                }
                                _ => {}
                            }
                        }
                        AppState::SnakeGame => {
                            match key_event.key {
                                KeyCode::Up => snake.set_direction(3),
                                KeyCode::Down => snake.set_direction(1),
                                KeyCode::Left => snake.set_direction(2),
                                KeyCode::Right => snake.set_direction(0),
                                KeyCode::Escape => {
                                    state = AppState::Menu;
                                    needs_redraw = true;
                                    debug_println!("Returning to menu");
                                }
                                _ => {}
                            }
                        }
                        AppState::Screensaver | AppState::About => {
                            if key_event.key == KeyCode::Escape || key_event.key == KeyCode::Enter {
                                state = AppState::Menu;
                                needs_redraw = true;
                                debug_println!("Returning to menu");
                            }
                        }
                    }
                }
            }
        }

        // Render based on state
        unsafe {
            core::arch::asm!("dsb sy");

            match state {
                AppState::Menu => {
                    if needs_redraw {
                        // Clear and draw menu
                        for y in 0..height {
                            for x in 0..pitch {
                                ptr.add(y * pitch + x).write_volatile(bg_color);
                            }
                        }
                        draw_static_elements(ptr, pitch, width, height);
                        draw_menu(ptr, pitch, width, height, menu_selected);
                        needs_redraw = false;
                    }
                }
                AppState::SnakeGame | AppState::Screensaver => {
                    if needs_redraw {
                        // Clear and draw title
                        for y in 0..height {
                            for x in 0..pitch {
                                ptr.add(y * pitch + x).write_volatile(bg_color);
                            }
                        }
                        draw_static_elements(ptr, pitch, width, height);
                        needs_redraw = false;
                    }

                    // Erase previous snake
                    for i in 0..snake.length {
                        let seg = prev_segments[i];
                        if seg.x >= PLAY_AREA_LEFT && seg.y >= PLAY_AREA_TOP {
                            let x = (seg.x as usize).saturating_sub(segment_size / 2);
                            let y = (seg.y as usize).saturating_sub(segment_size / 2);
                            if x + segment_size < PLAY_AREA_RIGHT as usize && y + segment_size < PLAY_AREA_BOTTOM as usize {
                                draw_block(ptr, pitch, x, y, segment_size, segment_size, bg_color);
                            }
                        }
                    }

                    // Save positions
                    for i in 0..snake.length {
                        prev_segments[i] = snake.segments[i];
                    }

                    // Update snake (auto-turn only in screensaver mode)
                    if state == AppState::Screensaver {
                        snake.update();
                    } else {
                        snake.update_no_auto_turn();
                    }

                    // Draw snake
                    for i in 0..snake.length {
                        let seg = snake.segments[i];
                        if seg.x >= PLAY_AREA_LEFT && seg.y >= PLAY_AREA_TOP {
                            let x = (seg.x as usize).saturating_sub(segment_size / 2);
                            let y = (seg.y as usize).saturating_sub(segment_size / 2);
                            if x + segment_size < PLAY_AREA_RIGHT as usize && y + segment_size < PLAY_AREA_BOTTOM as usize {
                                let hue = ((i as u32 * 18 + frame * 4) % 360) as u16;
                                let color = hsv_to_rgb(hue, 255, 255);
                                draw_block(ptr, pitch, x, y, segment_size, segment_size, color);
                            }
                        }
                    }

                    frame = frame.wrapping_add(1);
                }
                AppState::About => {
                    if needs_redraw {
                        for y in 0..height {
                            for x in 0..pitch {
                                ptr.add(y * pitch + x).write_volatile(bg_color);
                            }
                        }
                        draw_static_elements(ptr, pitch, width, height);
                        draw_about_screen(ptr, pitch, width, height);
                        needs_redraw = false;
                    }
                }
            }

            core::arch::asm!("dsb sy");
            core::arch::asm!("isb");
        }

        // Frame delay (shorter for responsive input)
        for _ in 0..100_000 { core::hint::spin_loop(); }
    }
}

#[protection_domain]
fn init() -> TvDemoHandler {
    debug_println!("");
    debug_println!("========================================");
    debug_println!("  seL4 TV Demo - Interactive Menu");
    debug_println!("========================================");
    debug_println!("");

    // Step 1: Blink LED (proves seL4 is running)
    blink_activity_led();

    // Step 2: Initialize framebuffer via VideoCore mailbox
    match init_framebuffer() {
        Some(fb) => {
            debug_println!("Framebuffer ready, starting app...");
            run_app(&fb);
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
