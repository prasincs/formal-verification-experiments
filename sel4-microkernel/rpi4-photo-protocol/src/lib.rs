//! # Verified Photo Frame IPC Protocol
//!
//! Formally verified IPC protocol for secure photo frame with Decoder/Display isolation.
//! Uses Verus to prove memory safety, bounds correctness, and isolation properties.
//!
//! ## Security Model
//!
//! The Decoder PD is considered **untrusted** - it parses potentially malicious image files.
//! Even if compromised, the Decoder cannot:
//! - Access the framebuffer directly (enforced by seL4 capabilities)
//! - Read arbitrary storage (no Storage PD capability)
//! - Send malformed pixel data (verified bounds checking in Display PD)
//!
//! ## Memory Layout
//!
//! ### Command Ring (4KB) - Display ↔ Input/Timer
//! ```text
//! +-------------------+ 0x000
//! | CommandRingHeader | (16 bytes)
//! +-------------------+ 0x010
//! | PhotoCommand[0]   | (8 bytes each)
//! | PhotoCommand[1]   |
//! | ...               |
//! +-------------------+ 0x1000
//! ```
//!
//! ### Pixel Buffer (4MB) - Decoder → Display
//! ```text
//! +-------------------+ 0x000
//! | PixelBufferHeader | (32 bytes)
//! +-------------------+ 0x020
//! | Pixel data        | (up to ~4MB)
//! | RGBA32 format     |
//! +-------------------+
//! ```

#![no_std]
#![allow(unused)]
#![allow(clippy::assign_op_pattern)]
#![allow(clippy::new_without_default)]

use verus_builtin_macros::verus;

verus! {

// ============================================================================
// DISPLAY CONFIGURATION
// ============================================================================

/// Maximum supported photo width
pub const MAX_PHOTO_WIDTH: u32 = 1920;

/// Maximum supported photo height
pub const MAX_PHOTO_HEIGHT: u32 = 1080;

/// Maximum pixels (for buffer sizing)
pub const MAX_PIXELS: u32 = MAX_PHOTO_WIDTH * MAX_PHOTO_HEIGHT;

/// Bytes per pixel (RGBA32)
pub const BYTES_PER_PIXEL: u32 = 4;

/// Maximum pixel data size (approximately 8MB for 1920x1080 RGBA)
pub const MAX_PIXEL_DATA_SIZE: u32 = MAX_PIXELS * BYTES_PER_PIXEL;

// ============================================================================
// CHANNEL IDS
// ============================================================================

/// Channel ID for input → display notifications
pub const INPUT_CHANNEL_ID: usize = 1;

/// Channel ID for decoder → display notifications
pub const DECODER_CHANNEL_ID: usize = 2;

/// Channel ID for timer → display notifications
pub const TIMER_CHANNEL_ID: usize = 3;

/// Channel ID for display → decoder requests
pub const DISPLAY_TO_DECODER_CHANNEL_ID: usize = 4;

// ============================================================================
// PHOTO COMMANDS
// ============================================================================

/// Command types for photo navigation
pub const CMD_NONE: u8 = 0;
pub const CMD_NEXT: u8 = 1;
pub const CMD_PREV: u8 = 2;
pub const CMD_PAUSE: u8 = 3;
pub const CMD_RESUME: u8 = 4;
pub const CMD_GOTO: u8 = 5;
pub const CMD_LOAD_COMPLETE: u8 = 6;
pub const CMD_LOAD_ERROR: u8 = 7;

/// Specification: is a command type valid?
pub open spec fn valid_command_type(cmd: u8) -> bool {
    cmd == CMD_NONE ||
    cmd == CMD_NEXT ||
    cmd == CMD_PREV ||
    cmd == CMD_PAUSE ||
    cmd == CMD_RESUME ||
    cmd == CMD_GOTO ||
    cmd == CMD_LOAD_COMPLETE ||
    cmd == CMD_LOAD_ERROR
}

/// A photo navigation command.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct PhotoCommand {
    /// Command type
    pub command: u8,
    /// Flags (reserved)
    pub flags: u8,
    /// Target photo index (for CMD_GOTO)
    pub photo_index: u16,
    /// Reserved for future use
    pub _reserved: u32,
}

impl PhotoCommand {
    /// Specification: is this command valid?
    pub open spec fn valid(&self) -> bool {
        valid_command_type(self.command)
    }

    /// Create a next photo command
    pub fn next() -> (cmd: Self)
        ensures cmd.valid(), cmd.command == CMD_NEXT
    {
        PhotoCommand {
            command: CMD_NEXT,
            flags: 0,
            photo_index: 0,
            _reserved: 0,
        }
    }

    /// Create a previous photo command
    pub fn prev() -> (cmd: Self)
        ensures cmd.valid(), cmd.command == CMD_PREV
    {
        PhotoCommand {
            command: CMD_PREV,
            flags: 0,
            photo_index: 0,
            _reserved: 0,
        }
    }

    /// Create a goto command
    pub fn goto(index: u16) -> (cmd: Self)
        ensures cmd.valid(), cmd.command == CMD_GOTO, cmd.photo_index == index
    {
        PhotoCommand {
            command: CMD_GOTO,
            flags: 0,
            photo_index: index,
            _reserved: 0,
        }
    }

    /// Create a pause command
    pub fn pause() -> (cmd: Self)
        ensures cmd.valid(), cmd.command == CMD_PAUSE
    {
        PhotoCommand {
            command: CMD_PAUSE,
            flags: 0,
            photo_index: 0,
            _reserved: 0,
        }
    }

    /// Create a resume command
    pub fn resume() -> (cmd: Self)
        ensures cmd.valid(), cmd.command == CMD_RESUME
    {
        PhotoCommand {
            command: CMD_RESUME,
            flags: 0,
            photo_index: 0,
            _reserved: 0,
        }
    }

    /// Create a load complete notification
    pub fn load_complete() -> (cmd: Self)
        ensures cmd.valid(), cmd.command == CMD_LOAD_COMPLETE
    {
        PhotoCommand {
            command: CMD_LOAD_COMPLETE,
            flags: 0,
            photo_index: 0,
            _reserved: 0,
        }
    }

    /// Create an error notification
    pub fn load_error() -> (cmd: Self)
        ensures cmd.valid(), cmd.command == CMD_LOAD_ERROR
    {
        PhotoCommand {
            command: CMD_LOAD_ERROR,
            flags: 0,
            photo_index: 0,
            _reserved: 0,
        }
    }

    /// Create an empty command
    pub fn empty() -> (cmd: Self)
        ensures cmd.valid(), cmd.command == CMD_NONE
    {
        PhotoCommand {
            command: CMD_NONE,
            flags: 0,
            photo_index: 0,
            _reserved: 0,
        }
    }
}

// ============================================================================
// PIXEL BUFFER
// ============================================================================

/// Pixel format types
pub const PIXEL_FORMAT_RGB24: u8 = 0;
pub const PIXEL_FORMAT_RGBA32: u8 = 1;
pub const PIXEL_FORMAT_RGB565: u8 = 2;

/// Buffer status codes
pub const BUFFER_STATUS_EMPTY: u8 = 0;
pub const BUFFER_STATUS_LOADING: u8 = 1;
pub const BUFFER_STATUS_READY: u8 = 2;
pub const BUFFER_STATUS_ERROR: u8 = 3;

/// Specification: is pixel format valid?
pub open spec fn valid_pixel_format(fmt: u8) -> bool {
    fmt == PIXEL_FORMAT_RGB24 ||
    fmt == PIXEL_FORMAT_RGBA32 ||
    fmt == PIXEL_FORMAT_RGB565
}

/// Specification: is buffer status valid?
pub open spec fn valid_buffer_status(status: u8) -> bool {
    status == BUFFER_STATUS_EMPTY ||
    status == BUFFER_STATUS_LOADING ||
    status == BUFFER_STATUS_READY ||
    status == BUFFER_STATUS_ERROR
}

/// Pixel buffer header for Decoder → Display transfer.
///
/// The Decoder writes pixels and sets status to READY.
/// The Display reads pixels and sets status to EMPTY.
#[derive(Clone, Copy, Debug)]
#[repr(C, align(32))]
pub struct PixelBufferHeader {
    /// Image width in pixels (verified: ≤ MAX_PHOTO_WIDTH)
    pub width: u32,
    /// Image height in pixels (verified: ≤ MAX_PHOTO_HEIGHT)
    pub height: u32,
    /// Pixel format (RGBA32, RGB24, etc.)
    pub format: u8,
    /// Buffer status (Empty, Loading, Ready, Error)
    pub status: u8,
    /// Photo index in slideshow
    pub photo_index: u16,
    /// Actual data length in bytes
    pub data_len: u32,
    /// Checksum of pixel data (for integrity verification)
    pub checksum: u32,
    /// Reserved padding to 32 bytes
    pub _reserved: [u8; 8],
}

impl PixelBufferHeader {
    /// Size of header in bytes
    pub const SIZE: usize = 32;

    /// Specification: are dimensions within bounds?
    pub open spec fn valid_dimensions(&self) -> bool {
        self.width > 0 &&
        self.height > 0 &&
        self.width <= MAX_PHOTO_WIDTH &&
        self.height <= MAX_PHOTO_HEIGHT
    }

    /// Specification: is the data length valid for the dimensions?
    pub open spec fn valid_data_len(&self) -> bool {
        if self.format == PIXEL_FORMAT_RGBA32 {
            self.data_len == self.width * self.height * 4
        } else if self.format == PIXEL_FORMAT_RGB24 {
            self.data_len == self.width * self.height * 3
        } else if self.format == PIXEL_FORMAT_RGB565 {
            self.data_len == self.width * self.height * 2
        } else {
            false
        }
    }

    /// Specification: is this header fully valid?
    pub open spec fn valid(&self) -> bool {
        self.valid_dimensions() &&
        valid_pixel_format(self.format) &&
        valid_buffer_status(self.status) &&
        self.valid_data_len()
    }

    /// Create an empty buffer header
    pub fn empty() -> (header: Self)
        ensures
            header.status == BUFFER_STATUS_EMPTY,
            header.width == 0,
            header.height == 0,
    {
        PixelBufferHeader {
            width: 0,
            height: 0,
            format: PIXEL_FORMAT_RGBA32,
            status: BUFFER_STATUS_EMPTY,
            photo_index: 0,
            data_len: 0,
            checksum: 0,
            _reserved: [0; 8],
        }
    }

    /// Create a header for an image
    pub fn new(width: u32, height: u32, format: u8, photo_index: u16) -> (header: Self)
        requires
            width > 0,
            height > 0,
            width <= MAX_PHOTO_WIDTH,
            height <= MAX_PHOTO_HEIGHT,
            valid_pixel_format(format),
        ensures
            header.valid_dimensions(),
            header.width == width,
            header.height == height,
            header.format == format,
            header.status == BUFFER_STATUS_LOADING,
    {
        let bpp: u32 = if format == PIXEL_FORMAT_RGBA32 { 4 }
                      else if format == PIXEL_FORMAT_RGB24 { 3 }
                      else { 2 };

        PixelBufferHeader {
            width,
            height,
            format,
            status: BUFFER_STATUS_LOADING,
            photo_index,
            data_len: width * height * bpp,
            checksum: 0,
            _reserved: [0; 8],
        }
    }
}

// ============================================================================
// PIXEL INDEX CALCULATIONS (Verified)
// ============================================================================

/// Specification: is a pixel coordinate valid for given dimensions?
pub open spec fn valid_pixel_coord(x: u32, y: u32, width: u32, height: u32) -> bool {
    x < width && y < height
}

/// Specification: can the pixel index be safely computed?
pub open spec fn pixel_index_safe(x: u32, y: u32, width: u32, height: u32) -> bool {
    valid_pixel_coord(x, y, width, height) &&
    (y as u64) * (width as u64) + (x as u64) < 0xFFFF_FFFF  // Fits in u32
}

/// Compute pixel index with verified bounds.
/// Returns (y * width + x) * bytes_per_pixel
pub fn pixel_offset_rgba(x: u32, y: u32, width: u32, height: u32) -> (offset: u32)
    requires
        pixel_index_safe(x, y, width, height),
        width <= MAX_PHOTO_WIDTH,
        height <= MAX_PHOTO_HEIGHT,
    ensures
        offset == (y * width + x) * 4,
        offset < MAX_PIXEL_DATA_SIZE,
{
    (y * width + x) * 4
}

/// Verified pixel copy: copies pixels from source to destination with bounds checking.
/// This is the core operation used by Display PD to blit decoded images.
pub open spec fn valid_blit_params(
    src_w: u32, src_h: u32,
    dst_x: u32, dst_y: u32,
    dst_w: u32, dst_h: u32
) -> bool {
    src_w > 0 && src_h > 0 &&
    dst_w > 0 && dst_h > 0 &&
    dst_x < dst_w && dst_y < dst_h &&
    // Source fits at destination position
    dst_x + src_w <= dst_w &&
    dst_y + src_h <= dst_h
}

// ============================================================================
// COMMAND RING BUFFER
// ============================================================================

/// Command ring capacity
pub const CMD_RING_CAPACITY: u32 = 500;

/// Command entry size
pub const CMD_ENTRY_SIZE: usize = 8;

/// Command ring header size
pub const CMD_HEADER_SIZE: usize = 16;

/// Command ring buffer shared memory size (4KB)
pub const CMD_RING_SIZE: usize = 0x1000;

/// Command ring header
#[derive(Clone, Copy, Debug)]
#[repr(C, align(16))]
pub struct CommandRingHeader {
    pub write_idx: u32,
    pub read_idx: u32,
    pub capacity: u32,
    pub _pad: u32,
}

impl CommandRingHeader {
    /// Specification: are indices valid?
    pub open spec fn valid(&self) -> bool {
        self.capacity > 0 &&
        self.capacity <= CMD_RING_CAPACITY &&
        self.write_idx < self.capacity &&
        self.read_idx < self.capacity
    }

    /// Specification: is buffer empty?
    pub open spec fn is_empty_spec(&self) -> bool {
        self.write_idx == self.read_idx
    }

    /// Specification: is buffer full?
    pub open spec fn is_full_spec(&self) -> bool {
        (self.write_idx + 1) % self.capacity == self.read_idx
    }

    /// Check if buffer has data
    pub fn has_data(&self) -> (has: bool)
        requires self.valid(),
        ensures has == !self.is_empty_spec(),
    {
        self.write_idx != self.read_idx
    }

    /// Check if buffer is full
    pub fn is_full(&self) -> (full: bool)
        requires self.valid(),
        ensures full == self.is_full_spec(),
    {
        (self.write_idx + 1) % self.capacity == self.read_idx
    }
}

// ============================================================================
// MEMORY REGION DEFINITIONS
// ============================================================================

/// Virtual address for command ring buffer (shared: Input, Timer, Display)
pub const CMD_RING_VADDR: usize = 0x5_0500_0000;

/// Virtual address for pixel buffer (shared: Decoder, Display)
pub const PIXEL_BUFFER_VADDR: usize = 0x5_0600_0000;

/// Pixel buffer size (8MB for 1920x1080 RGBA + header)
pub const PIXEL_BUFFER_SIZE: usize = 0x80_0000;

/// Specification: is address in command ring region?
pub open spec fn in_cmd_ring_region(addr: usize) -> bool {
    addr >= CMD_RING_VADDR && addr < CMD_RING_VADDR + CMD_RING_SIZE
}

/// Specification: is address in pixel buffer region?
pub open spec fn in_pixel_buffer_region(addr: usize) -> bool {
    addr >= PIXEL_BUFFER_VADDR && addr < PIXEL_BUFFER_VADDR + PIXEL_BUFFER_SIZE
}

// ============================================================================
// PROTECTION DOMAIN ISOLATION SPECIFICATIONS
// ============================================================================

/// Decoder PD memory regions (untrusted - image parsing)
/// Has: Pixel buffer (write), Photo data buffer (read)
/// Missing: Framebuffer, Storage, UART, Network
pub const DECODER_PD_PHOTO_DATA_BASE: usize = 0x5_0700_0000;
pub const DECODER_PD_PHOTO_DATA_SIZE: usize = 0x10_0000; // 1MB for photo file data

/// Specification: can Decoder PD access this address?
pub open spec fn decoder_pd_can_access(addr: usize) -> bool {
    // Pixel buffer (write decoded pixels)
    in_pixel_buffer_region(addr) ||
    // Photo data (read raw file bytes)
    (addr >= DECODER_PD_PHOTO_DATA_BASE &&
     addr < DECODER_PD_PHOTO_DATA_BASE + DECODER_PD_PHOTO_DATA_SIZE)
}

/// Display PD memory regions
pub const DISPLAY_PD_FB_BASE: usize = 0x5_0001_0000;
pub const DISPLAY_PD_FB_SIZE: usize = 0x100_0000;
pub const DISPLAY_PD_MAILBOX_BASE: usize = 0x5_0000_0000;
pub const DISPLAY_PD_MAILBOX_SIZE: usize = 0x1000;

/// Specification: can Display PD access this address?
pub open spec fn display_pd_can_access(addr: usize) -> bool {
    // Framebuffer (render)
    (addr >= DISPLAY_PD_FB_BASE && addr < DISPLAY_PD_FB_BASE + DISPLAY_PD_FB_SIZE) ||
    // GPU mailbox
    (addr >= DISPLAY_PD_MAILBOX_BASE && addr < DISPLAY_PD_MAILBOX_BASE + DISPLAY_PD_MAILBOX_SIZE) ||
    // Pixel buffer (read decoded images)
    in_pixel_buffer_region(addr) ||
    // Command ring (receive commands)
    in_cmd_ring_region(addr)
}

// ============================================================================
// ISOLATION PROOFS
// ============================================================================

/// Prove: Decoder PD cannot access framebuffer
/// This is the key security property - a compromised decoder cannot draw directly
proof fn decoder_cannot_access_framebuffer()
    ensures
        forall|addr: usize|
            (addr >= DISPLAY_PD_FB_BASE && addr < DISPLAY_PD_FB_BASE + DISPLAY_PD_FB_SIZE)
            ==> !decoder_pd_can_access(addr)
{
    // Decoder only has access to pixel buffer and photo data regions
    // Framebuffer is in a completely separate address range
}

/// Prove: Decoder PD cannot access storage
/// Prevents exfiltration if decoder is compromised
proof fn decoder_cannot_access_storage()
    ensures
        forall|addr: usize|
            // Storage region (hypothetical)
            (addr >= 0x5_0800_0000 && addr < 0x5_0900_0000)
            ==> !decoder_pd_can_access(addr)
{
    // Decoder regions don't overlap with storage
}

/// Prove: Only pixel buffer is shared between Decoder and Display
proof fn decoder_display_only_share_pixel_buffer()
    ensures
        forall|addr: usize|
            (decoder_pd_can_access(addr) && display_pd_can_access(addr))
            ==> in_pixel_buffer_region(addr)
{
    // The only overlapping region is the pixel buffer
}

} // verus!

// ============================================================================
// NON-VERIFIED RUNTIME HELPERS
// ============================================================================

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

/// Runtime command ring header with atomics
#[repr(C, align(16))]
pub struct AtomicCommandRingHeader {
    pub write_idx: AtomicU32,
    pub read_idx: AtomicU32,
    pub capacity: u32,
    pub _pad: u32,
}

impl AtomicCommandRingHeader {
    /// Initialize at memory location
    ///
    /// # Safety
    /// Pointer must be valid and properly aligned
    pub unsafe fn init(ptr: *mut Self) {
        (*ptr).write_idx = AtomicU32::new(0);
        (*ptr).read_idx = AtomicU32::new(0);
        (*ptr).capacity = CMD_RING_CAPACITY;
        (*ptr)._pad = 0;
    }

    pub fn has_data(&self) -> bool {
        let write = self.write_idx.load(Ordering::Acquire);
        let read = self.read_idx.load(Ordering::Acquire);
        write != read
    }

    pub fn is_full(&self) -> bool {
        let write = self.write_idx.load(Ordering::Acquire);
        let read = self.read_idx.load(Ordering::Acquire);
        ((write + 1) % self.capacity) == read
    }

    pub fn advance_write(&self) {
        let next = (self.write_idx.load(Ordering::Acquire) + 1) % self.capacity;
        self.write_idx.store(next, Ordering::Release);
    }

    pub fn advance_read(&self) {
        let next = (self.read_idx.load(Ordering::Acquire) + 1) % self.capacity;
        self.read_idx.store(next, Ordering::Release);
    }

    pub fn current_write_idx(&self) -> u32 {
        self.write_idx.load(Ordering::Acquire)
    }

    pub fn current_read_idx(&self) -> u32 {
        self.read_idx.load(Ordering::Acquire)
    }
}

/// Runtime pixel buffer header with atomics
#[repr(C, align(32))]
pub struct AtomicPixelBufferHeader {
    pub width: AtomicU32,
    pub height: AtomicU32,
    pub format: AtomicU8,
    pub status: AtomicU8,
    pub photo_index: u16,
    pub data_len: AtomicU32,
    pub checksum: AtomicU32,
    pub _reserved: [u8; 8],
}

impl AtomicPixelBufferHeader {
    /// Initialize at memory location
    ///
    /// # Safety
    /// Pointer must be valid and properly aligned
    pub unsafe fn init(ptr: *mut Self) {
        (*ptr).width = AtomicU32::new(0);
        (*ptr).height = AtomicU32::new(0);
        (*ptr).format = AtomicU8::new(PIXEL_FORMAT_RGBA32);
        (*ptr).status = AtomicU8::new(BUFFER_STATUS_EMPTY);
        (*ptr).photo_index = 0;
        (*ptr).data_len = AtomicU32::new(0);
        (*ptr).checksum = AtomicU32::new(0);
        (*ptr)._reserved = [0; 8];
    }

    pub fn is_ready(&self) -> bool {
        self.status.load(Ordering::Acquire) == BUFFER_STATUS_READY
    }

    pub fn is_empty(&self) -> bool {
        self.status.load(Ordering::Acquire) == BUFFER_STATUS_EMPTY
    }

    pub fn set_ready(&self) {
        self.status.store(BUFFER_STATUS_READY, Ordering::Release);
    }

    pub fn set_empty(&self) {
        self.status.store(BUFFER_STATUS_EMPTY, Ordering::Release);
    }

    pub fn set_loading(&self) {
        self.status.store(BUFFER_STATUS_LOADING, Ordering::Release);
    }

    pub fn set_error(&self) {
        self.status.store(BUFFER_STATUS_ERROR, Ordering::Release);
    }

    pub fn get_dimensions(&self) -> (u32, u32) {
        (
            self.width.load(Ordering::Acquire),
            self.height.load(Ordering::Acquire),
        )
    }

    pub fn set_dimensions(&self, width: u32, height: u32, format: u8) {
        self.width.store(width, Ordering::Release);
        self.height.store(height, Ordering::Release);
        self.format.store(format, Ordering::Release);

        let bpp = match format {
            PIXEL_FORMAT_RGBA32 => 4,
            PIXEL_FORMAT_RGB24 => 3,
            _ => 2,
        };
        self.data_len.store(width * height * bpp, Ordering::Release);
    }
}

/// Get command ring header pointer
///
/// # Safety
/// Base must be valid command ring memory
pub unsafe fn cmd_ring_header_ptr(base: *mut u8) -> *mut AtomicCommandRingHeader {
    base as *mut AtomicCommandRingHeader
}

/// Get command entries pointer
///
/// # Safety
/// Base must be valid command ring memory
pub unsafe fn cmd_entries_ptr(base: *mut u8) -> *mut PhotoCommand {
    base.add(CMD_HEADER_SIZE) as *mut PhotoCommand
}

/// Get pixel buffer header pointer
///
/// # Safety
/// Base must be valid pixel buffer memory
pub unsafe fn pixel_header_ptr(base: *mut u8) -> *mut AtomicPixelBufferHeader {
    base as *mut AtomicPixelBufferHeader
}

/// Get pixel data pointer (after header)
///
/// # Safety
/// Base must be valid pixel buffer memory
pub unsafe fn pixel_data_ptr(base: *mut u8) -> *mut u8 {
    base.add(PixelBufferHeader::SIZE)
}

// ============================================================================
// SIMPLE CHECKSUM (for data integrity)
// ============================================================================

/// Compute a simple checksum over pixel data
/// This is not cryptographic, just for detecting corruption
pub fn compute_checksum(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    for byte in data {
        sum = sum.wrapping_add(*byte as u32);
        sum = sum.wrapping_mul(31);
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_size() {
        assert_eq!(core::mem::size_of::<PhotoCommand>(), CMD_ENTRY_SIZE);
    }

    #[test]
    fn test_header_size() {
        assert_eq!(core::mem::size_of::<PixelBufferHeader>(), PixelBufferHeader::SIZE);
    }

    #[test]
    fn test_commands() {
        let next = PhotoCommand::next();
        assert_eq!(next.command, CMD_NEXT);

        let goto = PhotoCommand::goto(42);
        assert_eq!(goto.command, CMD_GOTO);
        assert_eq!(goto.photo_index, 42);
    }
}
