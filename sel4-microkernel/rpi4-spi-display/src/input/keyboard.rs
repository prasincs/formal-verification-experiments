//! Keyboard input driver
//!
//! Supports USB HID keyboard input for remote control functionality.
//! Common keycodes are mapped for media/navigation control.

use verus_builtin::*;
use verus_builtin_macros::*;

/// USB HID Keyboard base address (depends on USB controller setup)
pub const USB_HID_BASE: usize = 0xFE980000;

/// Key state (pressed or released)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    /// Key was pressed down
    Pressed,
    /// Key was released
    Released,
}

/// Common key codes for remote control functionality
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyCode {
    // Navigation keys
    /// Up arrow
    Up = 0x52,
    /// Down arrow
    Down = 0x51,
    /// Left arrow
    Left = 0x50,
    /// Right arrow
    Right = 0x4F,
    /// Enter/Select
    Enter = 0x28,
    /// Escape/Back
    Escape = 0x29,
    /// Space/Play-Pause
    Space = 0x2C,

    // Number keys (for channel selection)
    /// Key 0
    Num0 = 0x27,
    /// Key 1
    Num1 = 0x1E,
    /// Key 2
    Num2 = 0x1F,
    /// Key 3
    Num3 = 0x20,
    /// Key 4
    Num4 = 0x21,
    /// Key 5
    Num5 = 0x22,
    /// Key 6
    Num6 = 0x23,
    /// Key 7
    Num7 = 0x24,
    /// Key 8
    Num8 = 0x25,
    /// Key 9
    Num9 = 0x26,

    // Media control keys
    /// Volume Up (F12 as fallback)
    VolumeUp = 0x80,
    /// Volume Down (F11 as fallback)
    VolumeDown = 0x81,
    /// Mute (F10 as fallback)
    Mute = 0x7F,
    /// Play/Pause
    PlayPause = 0xCD,
    /// Stop
    Stop = 0xB7,
    /// Next Track
    NextTrack = 0xB5,
    /// Previous Track
    PrevTrack = 0xB6,

    // Function keys (for custom actions)
    /// F1 - Help/Info
    F1 = 0x3A,
    /// F2 - Menu
    F2 = 0x3B,
    /// F3 - Guide
    F3 = 0x3C,
    /// F4 - Settings
    F4 = 0x3D,

    // Special keys
    /// Home
    Home = 0x4A,
    /// End
    End = 0x4D,
    /// Page Up (Channel Up)
    PageUp = 0x4B,
    /// Page Down (Channel Down)
    PageDown = 0x4E,

    /// Unknown key
    Unknown = 0x00,
}

impl KeyCode {
    /// Convert from raw USB HID scancode
    pub fn from_scancode(code: u8) -> Self {
        match code {
            0x52 => KeyCode::Up,
            0x51 => KeyCode::Down,
            0x50 => KeyCode::Left,
            0x4F => KeyCode::Right,
            0x28 => KeyCode::Enter,
            0x29 => KeyCode::Escape,
            0x2C => KeyCode::Space,
            0x27 => KeyCode::Num0,
            0x1E => KeyCode::Num1,
            0x1F => KeyCode::Num2,
            0x20 => KeyCode::Num3,
            0x21 => KeyCode::Num4,
            0x22 => KeyCode::Num5,
            0x23 => KeyCode::Num6,
            0x24 => KeyCode::Num7,
            0x25 => KeyCode::Num8,
            0x26 => KeyCode::Num9,
            0x80 => KeyCode::VolumeUp,
            0x81 => KeyCode::VolumeDown,
            0x7F => KeyCode::Mute,
            0xCD => KeyCode::PlayPause,
            0xB7 => KeyCode::Stop,
            0xB5 => KeyCode::NextTrack,
            0xB6 => KeyCode::PrevTrack,
            0x3A => KeyCode::F1,
            0x3B => KeyCode::F2,
            0x3C => KeyCode::F3,
            0x3D => KeyCode::F4,
            0x4A => KeyCode::Home,
            0x4D => KeyCode::End,
            0x4B => KeyCode::PageUp,
            0x4E => KeyCode::PageDown,
            _ => KeyCode::Unknown,
        }
    }

    /// Check if this is a navigation key
    pub fn is_navigation(&self) -> bool {
        matches!(
            self,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Enter
                | KeyCode::Escape
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageUp
                | KeyCode::PageDown
        )
    }

    /// Check if this is a number key
    pub fn is_number(&self) -> bool {
        matches!(
            self,
            KeyCode::Num0
                | KeyCode::Num1
                | KeyCode::Num2
                | KeyCode::Num3
                | KeyCode::Num4
                | KeyCode::Num5
                | KeyCode::Num6
                | KeyCode::Num7
                | KeyCode::Num8
                | KeyCode::Num9
        )
    }

    /// Convert number key to digit (0-9), returns None for non-number keys
    pub fn to_digit(&self) -> Option<u8> {
        match self {
            KeyCode::Num0 => Some(0),
            KeyCode::Num1 => Some(1),
            KeyCode::Num2 => Some(2),
            KeyCode::Num3 => Some(3),
            KeyCode::Num4 => Some(4),
            KeyCode::Num5 => Some(5),
            KeyCode::Num6 => Some(6),
            KeyCode::Num7 => Some(7),
            KeyCode::Num8 => Some(8),
            KeyCode::Num9 => Some(9),
            _ => None,
        }
    }

    /// Check if this is a media control key
    pub fn is_media(&self) -> bool {
        matches!(
            self,
            KeyCode::VolumeUp
                | KeyCode::VolumeDown
                | KeyCode::Mute
                | KeyCode::PlayPause
                | KeyCode::Stop
                | KeyCode::NextTrack
                | KeyCode::PrevTrack
        )
    }
}

/// Keyboard key event
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// The key that was pressed or released
    pub key: KeyCode,
    /// Whether the key was pressed or released
    pub state: KeyState,
    /// Modifier keys held (shift, ctrl, alt)
    pub modifiers: KeyModifiers,
}

/// Modifier key states
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    /// Left or right shift is held
    pub shift: bool,
    /// Left or right control is held
    pub ctrl: bool,
    /// Left or right alt is held
    pub alt: bool,
}

/// Keyboard driver
pub struct Keyboard {
    base: usize,
    modifiers: KeyModifiers,
    last_keys: [u8; 6],
}

impl Keyboard {
    /// Create a new keyboard driver instance
    pub fn new() -> Self {
        Self::with_base(USB_HID_BASE)
    }

    /// Create a new keyboard driver with custom base address
    pub const fn with_base(base: usize) -> Self {
        Self {
            base,
            modifiers: KeyModifiers {
                shift: false,
                ctrl: false,
                alt: false,
            },
            last_keys: [0; 6],
        }
    }

    /// Poll for keyboard events
    #[verus_verify]
    pub fn poll(&mut self) -> Option<KeyEvent> {
        // Read HID report (8 bytes: modifiers + reserved + 6 keycodes)
        // This is a simplified implementation - actual USB HID requires
        // proper USB stack integration

        // TODO: Read from USB HID endpoint
        // For now, return None as we need USB driver integration
        None
    }

    /// Check if any key is currently pressed
    pub fn has_input(&self) -> bool {
        self.last_keys.iter().any(|&k| k != 0)
    }

    /// Get current modifier state
    pub fn modifiers(&self) -> KeyModifiers {
        self.modifiers
    }

    /// Process a raw HID report (8 bytes)
    pub fn process_hid_report(&mut self, report: &[u8; 8]) -> Option<KeyEvent> {
        // Byte 0: Modifier keys
        let mod_byte = report[0];
        self.modifiers = KeyModifiers {
            shift: (mod_byte & 0x22) != 0, // Left or right shift
            ctrl: (mod_byte & 0x11) != 0,  // Left or right ctrl
            alt: (mod_byte & 0x44) != 0,   // Left or right alt
        };

        // Byte 1: Reserved
        // Bytes 2-7: Up to 6 keycodes

        // Find new key presses (in current report but not in last)
        for i in 2..8 {
            let key = report[i];
            if key != 0 && !self.last_keys.contains(&key) {
                // Update last keys
                self.last_keys.copy_from_slice(&report[2..8]);

                return Some(KeyEvent {
                    key: KeyCode::from_scancode(key),
                    state: KeyState::Pressed,
                    modifiers: self.modifiers,
                });
            }
        }

        // Find released keys (in last report but not in current)
        for &key in &self.last_keys {
            if key != 0 && !report[2..8].contains(&key) {
                // Update last keys
                self.last_keys.copy_from_slice(&report[2..8]);

                return Some(KeyEvent {
                    key: KeyCode::from_scancode(key),
                    state: KeyState::Released,
                    modifiers: self.modifiers,
                });
            }
        }

        // Update last keys for next comparison
        self.last_keys.copy_from_slice(&report[2..8]);

        None
    }
}

impl Default for Keyboard {
    fn default() -> Self {
        Self::new()
    }
}
