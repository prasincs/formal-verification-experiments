//! # Secure Photo Frame Protection Domain
//!
//! A photo frame demo demonstrating seL4 isolation principles.
//! Photos are embedded at compile time and displayed with slideshow functionality.
//!
//! ## Security Model
//!
//! This demo uses a 2-PD architecture for simplicity:
//! - **Input PD**: UART input handling (isolated from display)
//! - **Photoframe PD**: Photo decoding + display (this PD)
//!
//! The full 3-PD architecture would separate decoder from display,
//! providing defense-in-depth against malicious image files.
//!
//! ## Features
//!
//! - Slideshow with configurable interval
//! - Manual navigation (next/prev)
//! - Pause/resume
//! - Photo info overlay

#![no_std]
#![no_main]

extern crate alloc;

mod decoder;
mod bounded_alloc;
mod validate;
mod secure_decode;

use sel4_microkit::{debug_println, protection_domain, Handler, ChannelSet, Channel};
use core::fmt;
use core::cell::UnsafeCell;
use core::sync::atomic::Ordering;

use bounded_alloc::BoundedBumpAllocator;
use secure_decode::{secure_decode_into, SecureDecodeError};

// ============================================================================
// BOUNDED GLOBAL ALLOCATOR
// ============================================================================
//
// The decoders for allocation-based formats (JPEG/PNG) allocate through the
// global allocator. We make that a fixed-size bump allocator so total
// allocation is hard-capped regardless of input. A malicious image cannot
// exhaust memory: once the pool is full, allocation fails and the secure
// pipeline reports OutOfMemory instead of letting the heap grow unbounded.
//
// Memory cost: this reserves DECODER_HEAP_SIZE of zero-initialized BSS in the
// PD image. 16 MB comfortably covers a full-screen (1280x720) JPEG/PNG decode
// plus the decoder's internal working buffers. A production build would back
// this with a dedicated `memory_region` in the .system file rather than BSS.

/// Fixed heap budget for image decoding.
const DECODER_HEAP_SIZE: usize = 16 * 1024 * 1024;

#[global_allocator]
static DECODER_HEAP: BoundedBumpAllocator<DECODER_HEAP_SIZE> = BoundedBumpAllocator::new();

use rpi4_graphics::{Mailbox, Framebuffer, MAILBOX_BASE};
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

/// Shared ring buffer virtual address (same as tvdemo)
const RING_BUFFER_VADDR: usize = 0x5_0400_0000;

/// Input channel for notifications from Input PD
const INPUT_CHANNEL: Channel = Channel::new(INPUT_CHANNEL_ID);

/// Slideshow interval in frames (at ~60fps, 300 = 5 seconds)
const SLIDESHOW_INTERVAL: u32 = 300;

// ============================================================================
// EMBEDDED PHOTO DATA
// ============================================================================

/// Where a photo's pixels come from.
#[derive(Clone, Copy)]
enum PhotoSource {
    /// Procedurally generated test pattern (no decoding).
    Generated(fn(u32, u32) -> u32),
    /// Encoded image bytes (BMP/QOI/PNG/JPEG) run through the secure pipeline.
    Encoded(&'static [u8]),
}

/// Photo metadata
struct Photo {
    name: &'static str,
    source: PhotoSource,
}

// Real encoded images embedded at compile time. These are decoded at runtime
// through the secure pipeline (validate -> budget -> bounded decode), exercising
// the exact path a dropped-in JPEG/PNG would take.
//
// To add your own: drop a file in `photos/` and `include_bytes!` it below. JPEG
// and PNG decode through zune-jpeg / zune-png under the bounded heap; BMP and QOI
// decode allocation-free.
const SAMPLE_QOI: &[u8] = include_bytes!("../photos/sample_gradient.qoi");
const SAMPLE_BMP: &[u8] = include_bytes!("../photos/sample_gradient.bmp");

/// Decoded-pixel scratch buffer (ARGB32), sized to the display. Encoded photos
/// are decoded here, then blitted (centered) to the framebuffer. Wrapped in an
/// `UnsafeCell` because the PD is single-threaded and the seL4 event loop never
/// re-enters `render` concurrently.
struct PixelScratch(UnsafeCell<[u32; (WIDTH * HEIGHT) as usize]>);
unsafe impl Sync for PixelScratch {}
static PIXEL_SCRATCH: PixelScratch =
    PixelScratch(UnsafeCell::new([0; (WIDTH * HEIGHT) as usize]));

/// Test pattern generators for demo photos
fn pattern_gradient(x: u32, y: u32) -> u32 {
    // Smooth gradient
    let r = (x * 255 / WIDTH) as u8;
    let g = (y * 255 / HEIGHT) as u8;
    let b = ((x + y) * 128 / (WIDTH + HEIGHT)) as u8;
    0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

fn pattern_circles(x: u32, y: u32) -> u32 {
    // Concentric circles
    let cx = WIDTH / 2;
    let cy = HEIGHT / 2;
    let dx = (x as i32 - cx as i32).unsigned_abs();
    let dy = (y as i32 - cy as i32).unsigned_abs();
    let dist = ((dx * dx + dy * dy) as f32).sqrt() as u32;
    let ring = (dist / 40) % 2;
    if ring == 0 {
        0xFF2060A0 // Blue
    } else {
        0xFF40B060 // Green
    }
}

fn pattern_checkerboard(x: u32, y: u32) -> u32 {
    // Checkerboard pattern
    let tile_size = 80;
    let tx = (x / tile_size) % 2;
    let ty = (y / tile_size) % 2;
    if tx == ty {
        0xFFE0E0E0 // Light
    } else {
        0xFF303030 // Dark
    }
}

fn pattern_sunset(x: u32, y: u32) -> u32 {
    // Sunset gradient
    let t = y * 255 / HEIGHT;
    let r = 255u8;
    let g = (180 - (t as i32 * 140 / 255).min(180)).max(0) as u8;
    let b = (100 - (t as i32 * 100 / 255).min(100)).max(0) as u8;

    // Sun
    let sun_x = WIDTH / 2;
    let sun_y = HEIGHT / 3;
    let sun_r = 80u32;
    let dx = (x as i32 - sun_x as i32).unsigned_abs();
    let dy = (y as i32 - sun_y as i32).unsigned_abs();
    if dx * dx + dy * dy < sun_r * sun_r {
        0xFFFFFF00 // Yellow sun
    } else {
        0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }
}

fn pattern_mountains(x: u32, y: u32) -> u32 {
    // Simple mountain silhouette
    let sky_color = 0xFF4080C0;
    let mountain_color = 0xFF203020;
    let snow_color = 0xFFE0E0F0;

    // Mountain peaks using simple triangle functions
    let peak1_x = WIDTH / 4;
    let peak1_h = HEIGHT / 2;
    let peak2_x = WIDTH * 2 / 3;
    let peak2_h = HEIGHT * 2 / 5;

    // Calculate mountain heights at this x
    let m1_height = if x < peak1_x {
        (x * peak1_h / peak1_x) as i32
    } else if x < WIDTH / 2 {
        ((WIDTH / 2 - x) * peak1_h / (WIDTH / 4)) as i32
    } else {
        0
    };

    let m2_height = if x > WIDTH / 3 && x < WIDTH {
        let rel_x = if x < peak2_x {
            (x - WIDTH / 3) * peak2_h / (peak2_x - WIDTH / 3)
        } else {
            (WIDTH - x) * peak2_h / (WIDTH - peak2_x)
        };
        rel_x as i32
    } else {
        0
    };

    let mountain_line = HEIGHT as i32 - m1_height.max(m2_height);

    if (y as i32) > mountain_line {
        mountain_color
    } else if (y as i32) > mountain_line - 30 && m1_height > (HEIGHT / 3) as i32 {
        snow_color
    } else {
        sky_color
    }
}

/// Collection of demo photos: a mix of procedural patterns and real encoded
/// images decoded through the secure pipeline.
const PHOTOS: &[Photo] = &[
    Photo { name: "GRADIENT", source: PhotoSource::Generated(pattern_gradient) },
    Photo { name: "QOI PHOTO", source: PhotoSource::Encoded(SAMPLE_QOI) },
    Photo { name: "BMP PHOTO", source: PhotoSource::Encoded(SAMPLE_BMP) },
    Photo { name: "CIRCLES", source: PhotoSource::Generated(pattern_circles) },
    Photo { name: "CHECKERBOARD", source: PhotoSource::Generated(pattern_checkerboard) },
    Photo { name: "SUNSET", source: PhotoSource::Generated(pattern_sunset) },
    Photo { name: "MOUNTAINS", source: PhotoSource::Generated(pattern_mountains) },
];

// ============================================================================
// INPUT HANDLING
// ============================================================================

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

    fn poll(&mut self) -> Option<(KeyCode, KeyState)> {
        unsafe {
            let header = &*header_ptr(self.ring_base);

            if !header.has_data() {
                return None;
            }

            let read_idx = header.current_read_idx();
            let entries = entries_ptr(self.ring_base);
            let entry = entries.add(read_idx as usize).read_volatile();

            core::sync::atomic::fence(Ordering::Acquire);
            header.advance_read();

            if entry.event_type == 1 && entry.key_state == 1 {
                let key_code = u8_to_key_code(entry.key_code);
                Some((key_code, KeyState::Pressed))
            } else {
                None
            }
        }
    }
}

// ============================================================================
// DRAWING HELPERS
// ============================================================================

/// Draw a filled rectangle
#[inline]
unsafe fn fill_rect(fb: *mut u32, pitch: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for dy in 0..h {
        for dx in 0..w {
            let px = x + dx;
            let py = y + dy;
            if px < WIDTH as usize && py < HEIGHT as usize {
                fb.add(py * pitch + px).write_volatile(color);
            }
        }
    }
}

/// 8x8 bitmap font for status display
const FONT_8X8: [[u8; 8]; 64] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // Space (32)
    [0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x00], // !
    [0x6C, 0x6C, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00], // "
    [0x6C, 0xFE, 0x6C, 0x6C, 0xFE, 0x6C, 0x00, 0x00], // #
    [0x18, 0x7E, 0x58, 0x7C, 0x1A, 0x7E, 0x18, 0x00], // $
    [0x62, 0x64, 0x08, 0x10, 0x26, 0x46, 0x00, 0x00], // %
    [0x38, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00], // &
    [0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00], // '
    [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00], // (
    [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00], // )
    [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00], // *
    [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00], // +
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30], // ,
    [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00], // -
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00], // .
    [0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x00], // /
    [0x7C, 0xC6, 0xCE, 0xD6, 0xE6, 0xC6, 0x7C, 0x00], // 0
    [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00], // 1
    [0x7C, 0xC6, 0x06, 0x1C, 0x70, 0xC0, 0xFE, 0x00], // 2
    [0x7C, 0xC6, 0x06, 0x3C, 0x06, 0xC6, 0x7C, 0x00], // 3
    [0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x1E, 0x00], // 4
    [0xFE, 0xC0, 0xFC, 0x06, 0x06, 0xC6, 0x7C, 0x00], // 5
    [0x38, 0x60, 0xC0, 0xFC, 0xC6, 0xC6, 0x7C, 0x00], // 6
    [0xFE, 0xC6, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00], // 7
    [0x7C, 0xC6, 0xC6, 0x7C, 0xC6, 0xC6, 0x7C, 0x00], // 8
    [0x7C, 0xC6, 0xC6, 0x7E, 0x06, 0x0C, 0x78, 0x00], // 9
    [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00], // :
    [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x30], // ;
    [0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00], // <
    [0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00], // =
    [0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x00], // >
    [0x7C, 0xC6, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x00], // ?
    [0x7C, 0xC6, 0xDE, 0xDE, 0xDE, 0xC0, 0x7C, 0x00], // @
    [0x38, 0x6C, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0x00], // A
    [0xFC, 0xC6, 0xC6, 0xFC, 0xC6, 0xC6, 0xFC, 0x00], // B
    [0x7C, 0xC6, 0xC0, 0xC0, 0xC0, 0xC6, 0x7C, 0x00], // C
    [0xF8, 0xCC, 0xC6, 0xC6, 0xC6, 0xCC, 0xF8, 0x00], // D
    [0xFE, 0xC0, 0xC0, 0xF8, 0xC0, 0xC0, 0xFE, 0x00], // E
    [0xFE, 0xC0, 0xC0, 0xF8, 0xC0, 0xC0, 0xC0, 0x00], // F
    [0x7C, 0xC6, 0xC0, 0xCE, 0xC6, 0xC6, 0x7E, 0x00], // G
    [0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0x00], // H
    [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00], // I
    [0x1E, 0x06, 0x06, 0x06, 0xC6, 0xC6, 0x7C, 0x00], // J
    [0xC6, 0xCC, 0xD8, 0xF0, 0xD8, 0xCC, 0xC6, 0x00], // K
    [0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xFE, 0x00], // L
    [0xC6, 0xEE, 0xFE, 0xD6, 0xC6, 0xC6, 0xC6, 0x00], // M
    [0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0xC6, 0xC6, 0x00], // N
    [0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00], // O
    [0xFC, 0xC6, 0xC6, 0xFC, 0xC0, 0xC0, 0xC0, 0x00], // P
    [0x7C, 0xC6, 0xC6, 0xC6, 0xD6, 0xDE, 0x7C, 0x06], // Q
    [0xFC, 0xC6, 0xC6, 0xFC, 0xD8, 0xCC, 0xC6, 0x00], // R
    [0x7C, 0xC6, 0xC0, 0x7C, 0x06, 0xC6, 0x7C, 0x00], // S
    [0xFF, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00], // T
    [0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00], // U
    [0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00], // V
    [0xC6, 0xC6, 0xC6, 0xD6, 0xFE, 0xEE, 0xC6, 0x00], // W
    [0xC6, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0xC6, 0x00], // X
    [0xC3, 0xC3, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x00], // Y
    [0xFE, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xFE, 0x00], // Z
    [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00], // [
    [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x00], // \
    [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00], // ]
    [0x10, 0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00], // ^
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF], // _
];

/// Draw a character at position (x, y) with scale
unsafe fn draw_char(fb: *mut u32, pitch: usize, x: usize, y: usize, ch: char, scale: usize, color: u32) {
    let idx = if ch >= ' ' && ch <= '_' {
        (ch as usize) - 32
    } else if ch >= 'a' && ch <= 'z' {
        (ch as usize) - 'a' as usize + 33 // Map to uppercase
    } else {
        0 // Space for unknown
    };

    let glyph = &FONT_8X8[idx];
    for row in 0..8 {
        for col in 0..8 {
            if (glyph[row] >> (7 - col)) & 1 != 0 {
                fill_rect(fb, pitch, x + col * scale, y + row * scale, scale, scale, color);
            }
        }
    }
}

/// Draw text string at position
unsafe fn draw_text(fb: *mut u32, pitch: usize, x: usize, y: usize, text: &str, scale: usize, color: u32) {
    let char_width = 8 * scale + scale; // 8 pixels + 1 spacing
    for (i, ch) in text.chars().enumerate() {
        draw_char(fb, pitch, x + i * char_width, y, ch, scale, color);
    }
}

// ============================================================================
// PHOTO FRAME STATE
// ============================================================================

/// Result of drawing the current photo, used to render the on-screen status.
#[derive(Clone, Copy)]
enum PhotoStatus {
    /// A procedural pattern was drawn (no decoding).
    Generated,
    /// An encoded image was decoded through the secure pipeline.
    Decoded {
        format: validate::ImageType,
        heap_peak_kb: u32,
    },
    /// The secure pipeline rejected or failed to decode the image.
    Failed(&'static str),
}

/// Photoframe application state
enum AppMode {
    Slideshow,
    Paused,
}

struct PhotoFrameHandler {
    framebuffer: Option<Framebuffer>,
    input: RingBufferInput,
    current_photo: usize,
    mode: AppMode,
    frame_counter: u32,
    slideshow_timer: u32,
    show_info: bool,
    needs_redraw: bool,
}

impl PhotoFrameHandler {
    fn new() -> Self {
        Self {
            framebuffer: None,
            input: RingBufferInput::new(),
            current_photo: 0,
            mode: AppMode::Slideshow,
            frame_counter: 0,
            slideshow_timer: 0,
            show_info: true,
            needs_redraw: true,
        }
    }

    fn next_photo(&mut self) {
        self.current_photo = (self.current_photo + 1) % PHOTOS.len();
        self.needs_redraw = true;
        self.slideshow_timer = 0;
        debug_println!("Photo {}/{}: {}", self.current_photo + 1, PHOTOS.len(), PHOTOS[self.current_photo].name);
    }

    fn prev_photo(&mut self) {
        if self.current_photo == 0 {
            self.current_photo = PHOTOS.len() - 1;
        } else {
            self.current_photo -= 1;
        }
        self.needs_redraw = true;
        self.slideshow_timer = 0;
        debug_println!("Photo {}/{}: {}", self.current_photo + 1, PHOTOS.len(), PHOTOS[self.current_photo].name);
    }

    fn handle_input(&mut self, key: KeyCode, _state: KeyState) {
        match key {
            KeyCode::Right | KeyCode::Down => {
                self.next_photo();
            }
            KeyCode::Left | KeyCode::Up => {
                self.prev_photo();
            }
            KeyCode::Space => {
                // Toggle pause
                match self.mode {
                    AppMode::Slideshow => {
                        self.mode = AppMode::Paused;
                        debug_println!("Paused");
                    }
                    AppMode::Paused => {
                        self.mode = AppMode::Slideshow;
                        self.slideshow_timer = 0;
                        debug_println!("Slideshow resumed");
                    }
                }
                self.needs_redraw = true;
            }
            KeyCode::Enter => {
                // Toggle info display
                self.show_info = !self.show_info;
                self.needs_redraw = true;
            }
            KeyCode::Escape => {
                // Return to first photo
                self.current_photo = 0;
                self.mode = AppMode::Slideshow;
                self.slideshow_timer = 0;
                self.needs_redraw = true;
            }
            _ => {}
        }
    }

    fn update(&mut self) {
        self.frame_counter = self.frame_counter.wrapping_add(1);

        // Handle slideshow timing
        if matches!(self.mode, AppMode::Slideshow) {
            self.slideshow_timer += 1;
            if self.slideshow_timer >= SLIDESHOW_INTERVAL {
                self.next_photo();
            }
        }
    }

    fn render(&mut self) {
        let fb = match &self.framebuffer {
            Some(fb) => fb,
            None => return,
        };

        if !self.needs_redraw {
            return;
        }

        let ptr = fb.buffer_ptr();
        let pitch = fb.pitch_pixels();

        unsafe {
            core::arch::asm!("dsb sy");

            let photo = &PHOTOS[self.current_photo];

            // Draw the photo. Generated patterns fill the screen directly;
            // encoded images are run through the secure decode pipeline into the
            // scratch buffer and then blitted centered.
            let photo_status = match photo.source {
                PhotoSource::Generated(gen) => {
                    for y in 0..HEIGHT {
                        for x in 0..WIDTH {
                            let color = gen(x, y);
                            ptr.add(y as usize * pitch + x as usize).write_volatile(color);
                        }
                    }
                    PhotoStatus::Generated
                }
                PhotoSource::Encoded(bytes) => {
                    // Fill the background first so margins around a smaller,
                    // centered image are clean rather than stale pixels.
                    for y in 0..HEIGHT as usize {
                        for x in 0..WIDTH as usize {
                            ptr.add(y * pitch + x).write_volatile(0xFF101018);
                        }
                    }

                    // SECURE PIPELINE: validate -> budget -> bounded decode.
                    let scratch = &mut *PIXEL_SCRATCH.0.get();
                    match secure_decode_into(bytes, scratch, &DECODER_HEAP) {
                        Ok(res) => {
                            blit_centered(ptr, pitch, scratch, res.width, res.height);
                            debug_println!(
                                "Photoframe PD: decoded {} {}x{} heap_peak={}KB",
                                image_type_str(res.format),
                                res.width,
                                res.height,
                                res.heap_peak / 1024
                            );
                            PhotoStatus::Decoded {
                                format: res.format,
                                heap_peak_kb: (res.heap_peak / 1024) as u32,
                            }
                        }
                        Err(e) => {
                            let reason = secure_error_str(&e);
                            debug_println!("Photoframe PD: secure decode rejected: {}", reason);
                            PhotoStatus::Failed(reason)
                        }
                    }
                }
            };

            // Draw info overlay if enabled
            if self.show_info {
                // Semi-transparent black bar at top
                let bar_height = 40usize;
                for y in 0..bar_height {
                    for x in 0..WIDTH as usize {
                        let bg = ptr.add(y * pitch + x).read_volatile();
                        // Darken by 50%
                        let r = ((bg >> 16) & 0xFF) / 2;
                        let g = ((bg >> 8) & 0xFF) / 2;
                        let b = (bg & 0xFF) / 2;
                        ptr.add(y * pitch + x).write_volatile(0xFF000000 | (r << 16) | (g << 8) | b);
                    }
                }

                // Draw photo name
                draw_text(ptr, pitch, 20, 12, photo.name, 2, 0xFFFFFFFF);

                // Draw photo counter
                let mut counter_buf = [0u8; 8];
                let counter_str = format_counter(self.current_photo + 1, PHOTOS.len(), &mut counter_buf);
                draw_text(ptr, pitch, WIDTH as usize - 120, 12, counter_str, 2, 0xFFFFFFFF);

                // Draw status indicator
                let status = match self.mode {
                    AppMode::Slideshow => ">",
                    AppMode::Paused => "||",
                };
                draw_text(ptr, pitch, WIDTH as usize / 2 - 20, 12, status, 2, 0xFF00FF00);

                // Draw controls hint at bottom
                let hint_y = HEIGHT as usize - 30;
                // Darken bottom bar
                for y in hint_y..HEIGHT as usize {
                    for x in 0..WIDTH as usize {
                        let bg = ptr.add(y * pitch + x).read_volatile();
                        let r = ((bg >> 16) & 0xFF) / 2;
                        let g = ((bg >> 8) & 0xFF) / 2;
                        let b = (bg & 0xFF) / 2;
                        ptr.add(y * pitch + x).write_volatile(0xFF000000 | (r << 16) | (g << 8) | b);
                    }
                }
                draw_text(ptr, pitch, 20, hint_y + 5, "ARROWS:NAV  SPACE:PAUSE  ENTER:INFO", 1, 0xFFCCCCCC);

                // SEL4 SECURE badge
                draw_text(ptr, pitch, WIDTH as usize - 180, hint_y + 5, "SEL4 SECURE", 1, 0xFF00B050);

                // Secure-decode status line for encoded photos (just above the
                // bottom hint bar). Tells the on-screen story: which decoder ran
                // and how much of the bounded heap it touched.
                let status_y = hint_y - 22;
                match photo_status {
                    PhotoStatus::Decoded { format, heap_peak_kb } => {
                        let mut num = [0u8; 12];
                        let kb = format_u32(heap_peak_kb, &mut num);
                        draw_text(ptr, pitch, 20, status_y, "SECURE DECODE:", 1, 0xFF80FF80);
                        draw_text(ptr, pitch, 180, status_y, image_type_str(format), 1, 0xFFFFFFFF);
                        draw_text(ptr, pitch, 260, status_y, "OK  HEAP PEAK", 1, 0xFFCCCCCC);
                        draw_text(ptr, pitch, 420, status_y, kb, 1, 0xFFFFFFFF);
                        draw_text(ptr, pitch, 480, status_y, "KB", 1, 0xFFCCCCCC);
                    }
                    PhotoStatus::Failed(reason) => {
                        draw_text(ptr, pitch, 20, status_y, "SECURE DECODE: REJECTED", 1, 0xFFFF6060);
                        draw_text(ptr, pitch, 320, status_y, reason, 1, 0xFFFF6060);
                    }
                    PhotoStatus::Generated => {
                        draw_text(ptr, pitch, 20, status_y, "PROCEDURAL PATTERN", 1, 0xFF8080FF);
                    }
                }
            }

            core::arch::asm!("dsb sy");
            core::arch::asm!("isb");
        }

        self.needs_redraw = false;
    }
}

/// Format photo counter as "N/M"
fn format_counter(current: usize, total: usize, buf: &mut [u8; 8]) -> &str {
    let c = (current % 10) as u8 + b'0';
    let t = (total % 10) as u8 + b'0';
    buf[0] = c;
    buf[1] = b'/';
    buf[2] = t;
    // Safety: we know these are valid ASCII
    unsafe { core::str::from_utf8_unchecked(&buf[0..3]) }
}

/// Format a u32 as decimal into `buf`, returning the written slice as &str.
fn format_u32(mut val: u32, buf: &mut [u8; 12]) -> &str {
    if val == 0 {
        buf[0] = b'0';
        return unsafe { core::str::from_utf8_unchecked(&buf[0..1]) };
    }
    // Write digits back-to-front, then shift to the front.
    let mut tmp = [0u8; 12];
    let mut i = 12;
    while val > 0 {
        i -= 1;
        tmp[i] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    let len = 12 - i;
    buf[..len].copy_from_slice(&tmp[i..]);
    unsafe { core::str::from_utf8_unchecked(&buf[..len]) }
}

/// Human-readable name for a validated image format.
fn image_type_str(t: validate::ImageType) -> &'static str {
    match t {
        validate::ImageType::Jpeg => "JPEG",
        validate::ImageType::Png => "PNG",
        validate::ImageType::Bmp => "BMP",
        validate::ImageType::Qoi => "QOI",
        validate::ImageType::Unknown => "UNKNOWN",
    }
}

/// Short, fixed reason string for a secure-decode failure (for logs/overlay).
fn secure_error_str(e: &SecureDecodeError) -> &'static str {
    match e {
        SecureDecodeError::Validation(_) => "BAD HEADER",
        SecureDecodeError::ExceedsBudget { .. } => "OVER BUDGET",
        SecureDecodeError::OutputTooSmall { .. } => "TOO LARGE",
        SecureDecodeError::Decode(_) => "CORRUPT",
        SecureDecodeError::OutOfMemory { .. } => "OUT OF MEMORY",
    }
}

/// Blit a decoded image from the scratch buffer to the framebuffer, centered.
///
/// Bounds are clamped to the display so an image reported larger than the
/// screen (or a dimension mismatch) can never write outside the framebuffer.
/// `src` is laid out as `src_w * src_h` ARGB32 pixels, row-major.
unsafe fn blit_centered(fb: *mut u32, pitch: usize, src: &[u32], src_w: u32, src_h: u32) {
    let copy_w = src_w.min(WIDTH) as usize;
    let copy_h = src_h.min(HEIGHT) as usize;
    let off_x = ((WIDTH.saturating_sub(src_w)) / 2) as usize;
    let off_y = ((HEIGHT.saturating_sub(src_h)) / 2) as usize;
    let stride = src_w as usize;

    for y in 0..copy_h {
        let src_row = y * stride;
        let dst_row = (off_y + y) * pitch + off_x;
        for x in 0..copy_w {
            // Defensive bound: never read past the scratch slice.
            let si = src_row + x;
            if si < src.len() {
                fb.add(dst_row + x).write_volatile(src[si]);
            }
        }
    }
}

// ============================================================================
// LED BLINK (startup indication)
// ============================================================================

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

// ============================================================================
// FRAMEBUFFER INIT
// ============================================================================

fn init_framebuffer() -> Option<Framebuffer> {
    debug_println!("Photoframe PD: Initializing framebuffer...");
    let mailbox = unsafe { Mailbox::new(MAILBOX_BASE) };

    match unsafe { Framebuffer::new(&mailbox, WIDTH, HEIGHT) } {
        Ok(fb) => {
            let info = fb.info();
            debug_println!("Photoframe PD: FB {}x{} @ 0x{:08x}", info.width, info.height, info.base);
            Some(fb)
        }
        Err(e) => {
            debug_println!("Photoframe PD: FB error: {:?}", e);
            None
        }
    }
}

// ============================================================================
// MICROKIT PROTECTION DOMAIN ENTRY
// ============================================================================

#[protection_domain]
fn init() -> PhotoFrameHandler {
    debug_println!("");
    debug_println!("========================================");
    debug_println!("  Secure Photo Frame - seL4 Microkit");
    debug_println!("========================================");
    debug_println!("");
    debug_println!("Security: Input isolated from display");
    debug_println!("Photos: {} ({} procedural, BMP+QOI decoded securely)", PHOTOS.len(), PHOTOS.len() - 2);
    debug_println!("Decoder heap: {} MB bounded (BoundedBumpAllocator)", DECODER_HEAP_SIZE / (1024 * 1024));
    debug_println!("Pipeline: validate -> budget -> bounded decode");
    debug_println!("");

    blink_activity_led();

    let mut handler = PhotoFrameHandler::new();
    handler.framebuffer = init_framebuffer();

    // Initial render
    handler.render();

    debug_println!("Photoframe PD: Ready");
    debug_println!("Controls: Arrows=Navigate, Space=Pause, Enter=Info");
    handler
}

#[derive(Debug)]
pub struct HandlerError;

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Photoframe PD handler error")
    }
}

impl Handler for PhotoFrameHandler {
    type Error = HandlerError;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        // Process input from Input PD
        if channels.contains(INPUT_CHANNEL) {
            while let Some((key, state)) = self.input.poll() {
                if state == KeyState::Pressed {
                    self.handle_input(key, state);
                }
            }
        }

        // Update state
        self.update();

        // Render if needed
        self.render();

        Ok(())
    }
}
