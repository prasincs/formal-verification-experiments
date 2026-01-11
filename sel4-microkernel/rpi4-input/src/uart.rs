//! UART serial input driver for Raspberry Pi 4
//!
//! Receives keyboard input via the mini-UART (serial console).
//! Maps ASCII characters and escape sequences to KeyCode/KeyEvent.
//!
//! This allows keyboard input from a terminal emulator connected to the
//! serial port, useful for development and testing before USB keyboard
//! driver is available.

use core::ptr::{read_volatile, write_volatile};
use crate::keyboard::{KeyCode, KeyState, KeyEvent, KeyModifiers};

/// Mini-UART base address (BCM2711)
/// Physical: 0xFE215040
/// Must be mapped by Microkit system file
pub const UART_BASE: usize = 0xFE215040;

/// Mini-UART register offsets
const MU_IO: usize = 0x00;      // I/O Data register
const MU_IER: usize = 0x04;     // Interrupt Enable
const MU_IIR: usize = 0x08;     // Interrupt Identify
const MU_LCR: usize = 0x0C;     // Line Control
const MU_MCR: usize = 0x10;     // Modem Control
const MU_LSR: usize = 0x14;     // Line Status
const MU_MSR: usize = 0x18;     // Modem Status
const MU_SCRATCH: usize = 0x1C; // Scratch
const MU_CNTL: usize = 0x20;    // Extra Control
const MU_STAT: usize = 0x24;    // Extra Status
const MU_BAUD: usize = 0x28;    // Baudrate

/// Line Status Register bits
const MU_LSR_DATA_READY: u32 = 1 << 0;  // Receive FIFO has data
const MU_LSR_TX_IDLE: u32 = 1 << 6;     // Transmit FIFO idle

/// Escape sequence parser state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EscapeState {
    /// Normal character input
    Normal,
    /// Received ESC, waiting for '['
    GotEsc,
    /// Received ESC [, waiting for code
    GotCsi,
}

/// UART serial input driver
pub struct Uart {
    base: usize,
    escape_state: EscapeState,
}

impl Uart {
    /// Create a new UART driver with default base address
    pub const fn new() -> Self {
        Self::with_base(UART_BASE)
    }

    /// Create a new UART driver with specified virtual base address
    pub const fn with_base(base: usize) -> Self {
        Self {
            base,
            escape_state: EscapeState::Normal,
        }
    }

    /// Read a register
    #[inline]
    fn read_reg(&self, offset: usize) -> u32 {
        unsafe { read_volatile((self.base + offset) as *const u32) }
    }

    /// Write a register
    #[inline]
    fn write_reg(&self, offset: usize, value: u32) {
        unsafe { write_volatile((self.base + offset) as *mut u32, value) }
    }

    /// Check if data is available to read
    #[inline]
    pub fn has_data(&self) -> bool {
        (self.read_reg(MU_LSR) & MU_LSR_DATA_READY) != 0
    }

    /// Read a single byte (non-blocking, returns None if no data)
    pub fn try_read_byte(&self) -> Option<u8> {
        if self.has_data() {
            Some((self.read_reg(MU_IO) & 0xFF) as u8)
        } else {
            None
        }
    }

    /// Poll for keyboard input event
    ///
    /// Handles ASCII characters and ANSI escape sequences for arrow keys.
    /// Returns a KeyEvent when a complete key input is recognized.
    pub fn poll(&mut self) -> Option<KeyEvent> {
        let byte = self.try_read_byte()?;

        match self.escape_state {
            EscapeState::Normal => {
                if byte == 0x1B {  // ESC
                    self.escape_state = EscapeState::GotEsc;
                    None
                } else {
                    // Regular ASCII character
                    self.map_ascii_to_event(byte)
                }
            }
            EscapeState::GotEsc => {
                if byte == b'[' {
                    self.escape_state = EscapeState::GotCsi;
                    None
                } else {
                    // Not a CSI sequence, treat ESC as Escape key
                    self.escape_state = EscapeState::Normal;
                    Some(KeyEvent {
                        key: KeyCode::Escape,
                        state: KeyState::Pressed,
                        modifiers: KeyModifiers::default(),
                    })
                }
            }
            EscapeState::GotCsi => {
                self.escape_state = EscapeState::Normal;
                // Arrow keys: ESC [ A/B/C/D
                let key = match byte {
                    b'A' => KeyCode::Up,
                    b'B' => KeyCode::Down,
                    b'C' => KeyCode::Right,
                    b'D' => KeyCode::Left,
                    b'H' => KeyCode::Home,
                    b'F' => KeyCode::End,
                    b'5' => {
                        // Page Up: ESC [ 5 ~
                        // Consume the trailing '~'
                        let _ = self.try_read_byte();
                        KeyCode::PageUp
                    }
                    b'6' => {
                        // Page Down: ESC [ 6 ~
                        let _ = self.try_read_byte();
                        KeyCode::PageDown
                    }
                    _ => KeyCode::Unknown,
                };

                if key != KeyCode::Unknown {
                    Some(KeyEvent {
                        key,
                        state: KeyState::Pressed,
                        modifiers: KeyModifiers::default(),
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Map ASCII byte to KeyEvent
    fn map_ascii_to_event(&self, byte: u8) -> Option<KeyEvent> {
        let key = match byte {
            // Control characters
            0x0D | 0x0A => KeyCode::Enter,   // CR or LF
            0x1B => KeyCode::Escape,          // ESC (standalone)
            0x20 => KeyCode::Space,           // Space
            0x7F | 0x08 => KeyCode::Escape,   // DEL or Backspace -> use as back

            // Number keys
            b'0' => KeyCode::Num0,
            b'1' => KeyCode::Num1,
            b'2' => KeyCode::Num2,
            b'3' => KeyCode::Num3,
            b'4' => KeyCode::Num4,
            b'5' => KeyCode::Num5,
            b'6' => KeyCode::Num6,
            b'7' => KeyCode::Num7,
            b'8' => KeyCode::Num8,
            b'9' => KeyCode::Num9,

            // WASD for navigation (common game controls)
            b'w' | b'W' => KeyCode::Up,
            b's' | b'S' => KeyCode::Down,
            b'a' | b'A' => KeyCode::Left,
            b'd' | b'D' => KeyCode::Right,

            // Alternative: HJKL (vim style)
            b'h' => KeyCode::Left,
            b'j' => KeyCode::Down,
            b'k' => KeyCode::Up,
            b'l' => KeyCode::Right,

            // Function keys (use letters as shortcuts)
            b'q' | b'Q' => KeyCode::Escape,   // Quit
            b'p' | b'P' => KeyCode::Space,    // Pause/Play
            b'm' | b'M' => KeyCode::Mute,     // Mute
            b'+' | b'=' => KeyCode::VolumeUp,
            b'-' | b'_' => KeyCode::VolumeDown,

            _ => KeyCode::Unknown,
        };

        if key != KeyCode::Unknown {
            Some(KeyEvent {
                key,
                state: KeyState::Pressed,
                modifiers: KeyModifiers::default(),
            })
        } else {
            None
        }
    }
}

impl Default for Uart {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_mapping() {
        let uart = Uart::new();

        // Test WASD mapping
        assert_eq!(
            uart.map_ascii_to_event(b'w').map(|e| e.key),
            Some(KeyCode::Up)
        );
        assert_eq!(
            uart.map_ascii_to_event(b's').map(|e| e.key),
            Some(KeyCode::Down)
        );

        // Test number keys
        assert_eq!(
            uart.map_ascii_to_event(b'5').map(|e| e.key),
            Some(KeyCode::Num5)
        );

        // Test enter
        assert_eq!(
            uart.map_ascii_to_event(0x0D).map(|e| e.key),
            Some(KeyCode::Enter)
        );
    }
}
