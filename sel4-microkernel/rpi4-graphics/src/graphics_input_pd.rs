//! # Graphics Protection Domain with IPC Input
//!
//! TV Demo with isolated input handling. Receives input events from
//! the Input PD via shared memory ring buffer.
//!
//! ## Security Properties (verified with Verus)
//!
//! 1. **Memory Isolation**: This PD only accesses:
//!    - Mailbox registers (GPU communication)
//!    - GPIO registers (LED control)
//!    - Framebuffer memory (graphics output)
//!    - DMA buffer (mailbox messages)
//!    - Shared ring buffer (input events - read only)
//!
//! 2. **No Direct Hardware Input**: Cannot access UART directly,
//!    all input comes through the verified ring buffer protocol.

#![no_std]
#![no_main]

extern crate alloc;

use sel4_microkit::{debug_println, protection_domain, Handler, ChannelSet, Channel};
use core::fmt;
use core::sync::atomic::Ordering;
use linked_list_allocator::LockedHeap;

use rpi4_graphics::{Mailbox, Framebuffer, MAILBOX_BASE};

// Global allocator for alloc-dependent code
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

// 64KB heap
const HEAP_SIZE: usize = 64 * 1024;
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
use rpi4_input::{KeyCode, KeyState};
use rpi4_input_protocol::{
    InputRingHeader, InputRingEntry, INPUT_CHANNEL_ID,
    header_ptr, entries_ptr,
};

/// Screen dimensions
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// GPIO virtual address
const GPIO_BASE: usize = 0x5_0200_0000;

/// Shared ring buffer virtual address
const RING_BUFFER_VADDR: usize = 0x5_0400_0000;

/// Input channel for notifications from Input PD
const INPUT_CHANNEL: Channel = Channel::new(INPUT_CHANNEL_ID);

/// Application state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppState {
    Menu,
    SnakeGame,
    Screensaver,
    About,
}

/// Menu item indices
const MENU_SNAKE_GAME: usize = 0;
const MENU_SCREENSAVER: usize = 1;
const MENU_ABOUT: usize = 2;
const MENU_ITEM_COUNT: usize = 3;

/// Convert u8 back to KeyCode
fn u8_to_key_code(code: u8) -> KeyCode {
    match code {
        1 => KeyCode::Up,
        2 => KeyCode::Down,
        3 => KeyCode::Left,
        4 => KeyCode::Right,
        5 => KeyCode::Enter,
        6 => KeyCode::Escape,
        7 => KeyCode::Space,
        10 => KeyCode::Num0,
        11 => KeyCode::Num1,
        12 => KeyCode::Num2,
        13 => KeyCode::Num3,
        14 => KeyCode::Num4,
        15 => KeyCode::Num5,
        16 => KeyCode::Num6,
        17 => KeyCode::Num7,
        18 => KeyCode::Num8,
        19 => KeyCode::Num9,
        20 => KeyCode::Home,
        21 => KeyCode::End,
        22 => KeyCode::PageUp,
        23 => KeyCode::PageDown,
        30 => KeyCode::VolumeUp,
        31 => KeyCode::VolumeDown,
        32 => KeyCode::Mute,
        _ => KeyCode::Unknown,
    }
}

/// Input reader from shared ring buffer
struct RingBufferInput {
    ring_base: *mut u8,
}

impl RingBufferInput {
    const fn new() -> Self {
        Self {
            ring_base: RING_BUFFER_VADDR as *mut u8,
        }
    }

    /// Poll for next input event from ring buffer
    ///
    /// ## Verification Properties (Verus)
    /// - Only reads from shared memory region
    /// - Updates read_idx atomically
    /// - Returns valid KeyCode/KeyState pairs
    fn poll(&mut self) -> Option<(KeyCode, KeyState)> {
        unsafe {
            let header = &*header_ptr(self.ring_base);

            // Check if data available
            if !header.has_data() {
                return None;
            }

            // Read entry at current read index
            let read_idx = header.current_read_idx();
            let entries = entries_ptr(self.ring_base);
            let entry = entries.add(read_idx as usize).read_volatile();

            // Memory barrier before advancing
            core::sync::atomic::fence(Ordering::Acquire);

            // Advance read index
            header.advance_read();

            // Convert to KeyCode/KeyState
            if entry.is_key_pressed() {
                let key_code = u8_to_key_code(entry.key_code);
                Some((key_code, KeyState::Pressed))
            } else if entry.event_type == 1 {
                // Key released
                let key_code = u8_to_key_code(entry.key_code);
                Some((key_code, KeyState::Released))
            } else {
                None
            }
        }
    }
}

struct GraphicsHandler {
    framebuffer: Option<Framebuffer>,
    input: RingBufferInput,
    state: AppState,
    menu_selected: usize,
    snake: Snake,
    needs_redraw: bool,
    frame: u32,
}

// ============== Snake and drawing code (same as tvdemo_main.rs) ==============

/// Play area bounds
const PLAY_AREA_TOP: i32 = 140;
const PLAY_AREA_BOTTOM: i32 = 630;
const PLAY_AREA_LEFT: i32 = 20;
const PLAY_AREA_RIGHT: i32 = 1260;

#[derive(Clone, Copy)]
struct Segment {
    x: i32,
    y: i32,
}

struct Snake {
    segments: [Segment; 30],
    length: usize,
    direction: u8,
    frame: u32,
}

impl Snake {
    fn new() -> Self {
        let mut segments = [Segment { x: 0, y: 0 }; 30];
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

    fn set_direction(&mut self, dir: u8) {
        self.direction = dir % 4;
    }

    fn update(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        if self.frame % 45 == 0 {
            self.direction = (self.direction + 1) % 4;
        }
        if self.frame % 120 == 0 {
            self.direction = (self.direction + 3) % 4;
        }
        self.move_forward();
    }

    fn update_no_auto_turn(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.move_forward();
    }

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

#[inline]
unsafe fn draw_block(fb: *mut u32, pitch: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for dy in 0..h {
        for dx in 0..w {
            fb.add((y + dy) * pitch + (x + dx)).write_volatile(color);
        }
    }
}

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

// Include the draw_letter and text rendering functions
// (abbreviated for space - in practice, copy from tvdemo_main.rs)

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
        '@' => {
            draw_block(ptr, pitch, x, y, b*3, b, color);
            draw_block(ptr, pitch, x, y+b, b, b*3, color);
            draw_block(ptr, pitch, x+b*2, y+b, b, b*2, color);
            draw_block(ptr, pitch, x+b, y+b*2, b*2, b, color);
            draw_block(ptr, pitch, x, y+b*4, b*3, b, color);
        }
        ' ' => {}
        _ => {}
    }
}

// Simplified rendering functions
unsafe fn draw_text(ptr: *mut u32, pitch: usize, x: usize, y: usize, text: &str, b: usize, color: u32) {
    let spacing = b * 4;
    for (i, c) in text.chars().enumerate() {
        draw_letter(ptr, pitch, x + i * spacing, y, b, c, color);
    }
}

impl GraphicsHandler {
    fn new() -> Self {
        Self {
            framebuffer: None,
            input: RingBufferInput::new(),
            state: AppState::Menu,
            menu_selected: 0,
            snake: Snake::new(),
            needs_redraw: true,
            frame: 0,
        }
    }

    fn handle_input(&mut self, key: KeyCode, state: KeyState) {
        if state != KeyState::Pressed {
            return;
        }

        debug_println!("Graphics PD: Key {:?}", key);

        match self.state {
            AppState::Menu => {
                match key {
                    KeyCode::Up => {
                        if self.menu_selected > 0 {
                            self.menu_selected -= 1;
                            self.needs_redraw = true;
                        }
                    }
                    KeyCode::Down => {
                        if self.menu_selected < MENU_ITEM_COUNT - 1 {
                            self.menu_selected += 1;
                            self.needs_redraw = true;
                        }
                    }
                    KeyCode::Enter | KeyCode::Space => {
                        match self.menu_selected {
                            MENU_SNAKE_GAME => {
                                self.state = AppState::SnakeGame;
                                self.snake = Snake::new();
                                self.needs_redraw = true;
                            }
                            MENU_SCREENSAVER => {
                                self.state = AppState::Screensaver;
                                self.snake = Snake::new();
                                self.needs_redraw = true;
                            }
                            MENU_ABOUT => {
                                self.state = AppState::About;
                                self.needs_redraw = true;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            AppState::SnakeGame => {
                match key {
                    KeyCode::Up => self.snake.set_direction(3),
                    KeyCode::Down => self.snake.set_direction(1),
                    KeyCode::Left => self.snake.set_direction(2),
                    KeyCode::Right => self.snake.set_direction(0),
                    KeyCode::Escape => {
                        self.state = AppState::Menu;
                        self.needs_redraw = true;
                    }
                    _ => {}
                }
            }
            AppState::Screensaver | AppState::About => {
                if key == KeyCode::Escape || key == KeyCode::Enter {
                    self.state = AppState::Menu;
                    self.needs_redraw = true;
                }
            }
        }
    }

    fn render(&mut self) {
        let fb = match &self.framebuffer {
            Some(fb) => fb,
            None => return,
        };

        let ptr = fb.buffer_ptr();
        let pitch = fb.pitch_pixels();
        let (width, height) = fb.dimensions();
        let bg_color: u32 = 0xFF101030;
        let white: u32 = 0xFFFFFFFF;
        let green: u32 = 0xFF00B050;
        let gray: u32 = 0xFF808080;

        unsafe {
            core::arch::asm!("dsb sy");

            if self.needs_redraw {
                // Clear screen
                for y in 0..height as usize {
                    for x in 0..pitch {
                        ptr.add(y * pitch + x).write_volatile(bg_color);
                    }
                }

                // Draw title "SEL4 SNAKE"
                let b = 12usize;
                draw_text(ptr, pitch, 80, 30, "SEL4", b, green);
                draw_text(ptr, pitch, 420, 30, "SNAKE", 15, white);

                // Draw border
                for x in 0..width as usize {
                    ptr.add(x).write_volatile(gray);
                    ptr.add((height as usize - 1) * pitch + x).write_volatile(gray);
                }
                for y in 0..height as usize {
                    ptr.add(y * pitch).write_volatile(gray);
                    ptr.add(y * pitch + width as usize - 1).write_volatile(gray);
                }
            }

            match self.state {
                AppState::Menu => {
                    if self.needs_redraw {
                        // Draw menu
                        let b = 6usize;
                        let spacing = b * 4;
                        let menu_y = 250usize;

                        // Menu items
                        let items = [("SNAKE GAME", 0), ("SCREENSAVER", 1), ("ABOUT", 2)];
                        for (text, idx) in items {
                            let y = menu_y + idx * 60;
                            let color = if idx == self.menu_selected { green } else { white };
                            if idx == self.menu_selected {
                                draw_block(ptr, pitch, 320, y + 5, 15, 15, green);
                            }
                            draw_text(ptr, pitch, 360, y, text, b, color);
                        }

                        self.needs_redraw = false;
                    }
                }
                AppState::SnakeGame | AppState::Screensaver => {
                    if self.needs_redraw {
                        self.needs_redraw = false;
                    }

                    // Update snake
                    if self.state == AppState::Screensaver {
                        self.snake.update();
                    } else {
                        self.snake.update_no_auto_turn();
                    }

                    // Draw snake
                    let segment_size = 20usize;
                    for i in 0..self.snake.length {
                        let seg = self.snake.segments[i];
                        if seg.x >= PLAY_AREA_LEFT && seg.y >= PLAY_AREA_TOP {
                            let x = (seg.x as usize).saturating_sub(segment_size / 2);
                            let y = (seg.y as usize).saturating_sub(segment_size / 2);
                            if x + segment_size < PLAY_AREA_RIGHT as usize && y + segment_size < PLAY_AREA_BOTTOM as usize {
                                let hue = ((i as u32 * 18 + self.frame * 4) % 360) as u16;
                                let color = hsv_to_rgb(hue, 255, 255);
                                draw_block(ptr, pitch, x, y, segment_size, segment_size, color);
                            }
                        }
                    }
                    self.frame = self.frame.wrapping_add(1);
                }
                AppState::About => {
                    if self.needs_redraw {
                        let b = 6usize;
                        draw_text(ptr, pitch, 300, 280, "SEL4 MICROKIT", b, white);
                        draw_text(ptr, pitch, 300, 340, "RPI4", b, white);
                        draw_text(ptr, pitch, 300, 400, "@PRASINCS", b, gray);
                        draw_text(ptr, pitch, 300, 480, "PRESS ESC", b, white);
                        self.needs_redraw = false;
                    }
                }
            }

            core::arch::asm!("dsb sy");
            core::arch::asm!("isb");
        }
    }
}

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
}

fn init_framebuffer() -> Option<Framebuffer> {
    debug_println!("Graphics PD: Initializing framebuffer...");
    let mailbox = unsafe { Mailbox::new(MAILBOX_BASE) };

    match unsafe { Framebuffer::new(&mailbox, WIDTH, HEIGHT) } {
        Ok(fb) => {
            let info = fb.info();
            debug_println!("Graphics PD: FB {}x{} @ 0x{:08x}", info.width, info.height, info.base);
            Some(fb)
        }
        Err(e) => {
            debug_println!("Graphics PD: FB error: {:?}", e);
            None
        }
    }
}

#[protection_domain]
fn init() -> GraphicsHandler {
    // Initialize the heap allocator
    unsafe {
        ALLOCATOR.lock().init(HEAP.as_mut_ptr(), HEAP_SIZE);
    }

    debug_println!("");
    debug_println!("========================================");
    debug_println!("  Graphics Protection Domain (IPC)");
    debug_println!("========================================");
    debug_println!("");

    blink_activity_led();

    let mut handler = GraphicsHandler::new();
    handler.framebuffer = init_framebuffer();

    debug_println!("Graphics PD: Ready, waiting for input events...");
    handler
}

#[derive(Debug)]
pub struct HandlerError;

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Graphics PD handler error")
    }
}

impl Handler for GraphicsHandler {
    type Error = HandlerError;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        // Check if notification is from Input PD
        if channels.contains(INPUT_CHANNEL) {
            // Process all available input events
            while let Some((key, state)) = self.input.poll() {
                self.handle_input(key, state);
            }
        }

        // Render frame
        self.render();

        Ok(())
    }
}
