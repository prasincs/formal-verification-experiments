//! Input device drivers for remote control options
//!
//! Provides verified drivers for various input devices including:
//! - Keyboard (USB HID or PS/2)
//! - TV/IR Remote (NEC protocol)
//! - Touch screen (via touch module)

pub mod keyboard;
pub mod ir_remote;

pub use keyboard::{Keyboard, KeyCode, KeyEvent, KeyState};
pub use ir_remote::{IrRemote, IrButton, IrEvent, IrProtocol};

/// Unified input event that can come from any input source
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// Keyboard key event
    Key(KeyEvent),
    /// IR remote button event
    Remote(IrEvent),
    /// Touch event (from touch module)
    Touch(crate::touch::TouchEvent),
}

/// Input source identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSource {
    /// USB or PS/2 keyboard
    Keyboard,
    /// IR remote receiver
    IrRemote,
    /// Touchscreen
    Touch,
}

/// Remote control options configuration
#[derive(Clone, Copy, Debug)]
pub struct RemoteOptions {
    /// Enable keyboard as remote input
    pub keyboard_enabled: bool,
    /// Enable IR remote input
    pub ir_remote_enabled: bool,
    /// Enable touch input
    pub touch_enabled: bool,
    /// IR protocol to use
    pub ir_protocol: IrProtocol,
}

impl Default for RemoteOptions {
    fn default() -> Self {
        Self {
            keyboard_enabled: true,
            ir_remote_enabled: true,
            touch_enabled: true,
            ir_protocol: IrProtocol::Nec,
        }
    }
}

impl RemoteOptions {
    /// Create options with only keyboard enabled
    pub const fn keyboard_only() -> Self {
        Self {
            keyboard_enabled: true,
            ir_remote_enabled: false,
            touch_enabled: false,
            ir_protocol: IrProtocol::Nec,
        }
    }

    /// Create options with only IR remote enabled
    pub const fn ir_remote_only() -> Self {
        Self {
            keyboard_enabled: false,
            ir_remote_enabled: true,
            touch_enabled: false,
            ir_protocol: IrProtocol::Nec,
        }
    }

    /// Create options with only touch enabled
    pub const fn touch_only() -> Self {
        Self {
            keyboard_enabled: false,
            ir_remote_enabled: false,
            touch_enabled: true,
            ir_protocol: IrProtocol::Nec,
        }
    }

    /// Create options with all inputs enabled
    pub const fn all() -> Self {
        Self {
            keyboard_enabled: true,
            ir_remote_enabled: true,
            touch_enabled: true,
            ir_protocol: IrProtocol::Nec,
        }
    }
}

/// Unified input controller trait
pub trait InputController {
    /// Poll for the next input event
    fn poll(&mut self) -> Option<InputEvent>;

    /// Get the input source type
    fn source(&self) -> InputSource;

    /// Check if input is available
    fn has_input(&self) -> bool;
}

/// Combined input manager that polls all enabled input sources
pub struct InputManager {
    options: RemoteOptions,
    keyboard: Option<Keyboard>,
    ir_remote: Option<IrRemote>,
}

impl InputManager {
    /// Create a new input manager with the given options
    pub fn new(options: RemoteOptions) -> Self {
        Self {
            options,
            keyboard: if options.keyboard_enabled {
                Some(Keyboard::new())
            } else {
                None
            },
            ir_remote: if options.ir_remote_enabled {
                Some(IrRemote::new(options.ir_protocol))
            } else {
                None
            },
        }
    }

    /// Poll all enabled input sources for events
    pub fn poll(&mut self) -> Option<InputEvent> {
        // Check keyboard first
        if let Some(ref mut kb) = self.keyboard {
            if let Some(event) = kb.poll() {
                return Some(InputEvent::Key(event));
            }
        }

        // Check IR remote
        if let Some(ref mut ir) = self.ir_remote {
            if let Some(event) = ir.poll() {
                return Some(InputEvent::Remote(event));
            }
        }

        None
    }

    /// Get the current options
    pub fn options(&self) -> &RemoteOptions {
        &self.options
    }

    /// Update options and reconfigure inputs
    pub fn set_options(&mut self, options: RemoteOptions) {
        self.options = options;

        self.keyboard = if options.keyboard_enabled {
            Some(Keyboard::new())
        } else {
            None
        };

        self.ir_remote = if options.ir_remote_enabled {
            Some(IrRemote::new(options.ir_protocol))
        } else {
            None
        };
    }
}
