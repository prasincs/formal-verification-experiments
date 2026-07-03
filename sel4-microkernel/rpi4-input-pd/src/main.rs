//! # Input Protection Domain
//!
//! Isolated protection domain for input handling on Raspberry Pi 4.
//! Communicates with Graphics PD via shared memory ring buffer.
//!
//! ## Security Properties (to be verified with Verus)
//!
//! 1. **Memory Isolation**: This PD only accesses:
//!    - UART registers at mapped virtual address
//!    - Shared ring buffer at mapped virtual address
//!    - No other memory regions
//!
//! 2. **IPC Safety**: Ring buffer operations maintain:
//!    - Single-producer (this PD) single-consumer (Graphics PD)
//!    - No buffer overflow (respects capacity)
//!    - Atomic index updates (release/acquire ordering)
//!
//! 3. **Input Validation**: Only valid KeyCode values are written

#![no_std]
#![no_main]

use sel4_microkit::{debug_println, protection_domain, Handler, ChannelSet, Channel};
use core::fmt;

use rpi4_input::{KeyCode, KeyState};
#[cfg(feature = "uart")]
use rpi4_input::Uart;
#[cfg(feature = "usb")]
use rpi4_input::{usb::DmaRegion, UsbKeyboard};
use rpi4_input_protocol::{
    InputRingHeader, InputRingEntry, KeyState as ProtoKeyState,
    INPUT_CHANNEL_ID, header_ptr, entries_ptr,
};

/// UART virtual address (mapped by Microkit)
/// Physical: 0xFE215000 (page), mini-UART at +0x40
#[cfg(feature = "uart")]
const UART_VADDR: usize = 0x5_0300_0000 + 0x40;

/// Shared ring buffer virtual address (mapped by Microkit)
const RING_BUFFER_VADDR: usize = 0x5_0400_0000;

/// DWC2 USB controller MMIO virtual address (mapped by Microkit).
/// Physical: 0xFE98_0000 (BCM2711 USB OTG core).
#[cfg(feature = "usb")]
const USB_REGS_VADDR: usize = 0x5_0500_0000;

/// USB DMA buffer virtual address (mapped uncached by Microkit).
#[cfg(feature = "usb")]
const USB_DMA_VADDR: usize = 0x5_0600_0000;
/// USB DMA buffer physical address (device-visible, declared in the .system).
#[cfg(feature = "usb")]
const USB_DMA_PADDR: usize = 0x3e86_0000;
/// USB DMA buffer size (one 4KB page is plenty for boot-keyboard transfers).
#[cfg(feature = "usb")]
const USB_DMA_SIZE: usize = 0x1000;

/// Graphics PD channel for notifications
const GRAPHICS_CHANNEL: Channel = Channel::new(INPUT_CHANNEL_ID);

/// Input PD handler
struct InputPdHandler {
    #[cfg(feature = "uart")]
    uart: Uart,
    #[cfg(feature = "usb")]
    usb: Option<UsbKeyboard>,
    ring_base: *mut u8,
}

impl InputPdHandler {
    /// Create new handler with mapped addresses
    ///
    /// # Safety
    /// The virtual addresses must be properly mapped by Microkit.
    unsafe fn new() -> Self {
        // Bring up the USB host controller best-effort: if the core does not
        // reach host mode (e.g. running under QEMU without a USB model), keep
        // the keyboard disabled and fall back to UART input.
        #[cfg(feature = "usb")]
        let usb = {
            let dma = DmaRegion {
                vaddr: USB_DMA_VADDR,
                paddr: USB_DMA_PADDR,
                size: USB_DMA_SIZE,
            };
            let mut usb = UsbKeyboard::new(USB_REGS_VADDR, dma);
            match usb.init() {
                Ok(()) => {
                    debug_println!("Input PD: USB host controller initialized");
                    Some(usb)
                }
                Err(e) => {
                    debug_println!("Input PD: USB init failed ({:?}), USB input disabled", e);
                    None
                }
            }
        };

        Self {
            #[cfg(feature = "uart")]
            uart: Uart::with_base(UART_VADDR),
            #[cfg(feature = "usb")]
            usb,
            ring_base: RING_BUFFER_VADDR as *mut u8,
        }
    }

    /// Initialize the ring buffer (called once at startup)
    ///
    /// # Safety
    /// Must only be called once, before any other ring buffer operations.
    unsafe fn init_ring_buffer(&self) {
        let header = header_ptr(self.ring_base);
        InputRingHeader::init(header);
        debug_println!("Input PD: Ring buffer initialized");
    }

    /// Write an input event to the ring buffer
    ///
    /// Returns true if event was written, false if buffer is full.
    ///
    /// ## Verification Properties (Verus)
    /// - Precondition: ring_base points to valid shared memory
    /// - Postcondition: if returns true, entry was written at correct index
    /// - Invariant: write_idx is always < capacity
    unsafe fn write_event(&self, key_code: KeyCode, key_state: KeyState) -> bool {
        let header = &*header_ptr(self.ring_base);

        // Check if buffer is full
        if header.is_full() {
            debug_println!("Input PD: Ring buffer full, dropping event");
            return false;
        }

        // Get current write index
        let write_idx = header.write_idx.load(core::sync::atomic::Ordering::Acquire);

        // Convert key code to u8 (validated mapping)
        let code_u8 = key_code_to_u8(key_code);
        let state = match key_state {
            KeyState::Pressed => ProtoKeyState::Pressed,
            KeyState::Released => ProtoKeyState::Released,
        };

        // Write entry at current index
        let entries = entries_ptr(self.ring_base);
        let entry = InputRingEntry::key(code_u8, state, 0);
        entries.add(write_idx as usize).write_volatile(entry);

        // Memory barrier before updating index
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

        // Advance write index
        header.advance_write();

        true
    }

    /// Poll all input sources and forward events to the ring buffer.
    fn poll_and_forward(&mut self) {
        // USB HID keyboard (real hardware input path). Poll first so the
        // mutable borrow of `self.usb` is released before `write_event`.
        #[cfg(feature = "usb")]
        {
            let usb_event = self.usb.as_mut().and_then(|usb| usb.poll());
            if let Some(event) = usb_event {
                unsafe {
                    if self.write_event(event.key, event.state) {
                        GRAPHICS_CHANNEL.notify();
                    }
                }
            }
        }

        // UART serial input (development / fallback path).
        #[cfg(feature = "uart")]
        if let Some(event) = self.uart.poll() {
            unsafe {
                if self.write_event(event.key, event.state) {
                    // Notify Graphics PD that new input is available
                    GRAPHICS_CHANNEL.notify();
                }
            }
        }
    }
}

/// Convert KeyCode enum to u8 for IPC
///
/// ## Verification (Verus)
/// - All valid KeyCode variants map to distinct u8 values
/// - Result is always a valid key code identifier
fn key_code_to_u8(key: KeyCode) -> u8 {
    match key {
        KeyCode::Up => 1,
        KeyCode::Down => 2,
        KeyCode::Left => 3,
        KeyCode::Right => 4,
        KeyCode::Enter => 5,
        KeyCode::Escape => 6,
        KeyCode::Space => 7,
        KeyCode::Num0 => 10,
        KeyCode::Num1 => 11,
        KeyCode::Num2 => 12,
        KeyCode::Num3 => 13,
        KeyCode::Num4 => 14,
        KeyCode::Num5 => 15,
        KeyCode::Num6 => 16,
        KeyCode::Num7 => 17,
        KeyCode::Num8 => 18,
        KeyCode::Num9 => 19,
        KeyCode::Home => 20,
        KeyCode::End => 21,
        KeyCode::PageUp => 22,
        KeyCode::PageDown => 23,
        KeyCode::VolumeUp => 30,
        KeyCode::VolumeDown => 31,
        KeyCode::Mute => 32,
        KeyCode::Unknown => 0,
        _ => 0,
    }
}

#[protection_domain]
fn init() -> InputPdHandler {
    debug_println!("");
    debug_println!("========================================");
    debug_println!("  Input Protection Domain Starting");
    debug_println!("========================================");
    debug_println!("");
    #[cfg(feature = "uart")]
    debug_println!("Input PD: UART at 0x{:x}", UART_VADDR);
    #[cfg(feature = "usb")]
    debug_println!("Input PD: USB controller at 0x{:x}", USB_REGS_VADDR);
    debug_println!("Input PD: Ring buffer at 0x{:x}", RING_BUFFER_VADDR);

    let handler = unsafe { InputPdHandler::new() };

    // Initialize ring buffer
    unsafe { handler.init_ring_buffer(); }

    debug_println!("Input PD: Ready, polling for input...");
    handler
}

#[derive(Debug)]
pub struct HandlerError;

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Input PD handler error")
    }
}

impl Handler for InputPdHandler {
    type Error = HandlerError;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        // Poll UART on each notification (or timer tick)
        self.poll_and_forward();
        Ok(())
    }
}
