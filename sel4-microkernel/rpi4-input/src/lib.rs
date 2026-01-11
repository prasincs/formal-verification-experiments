//! # Raspberry Pi 4 Input Device Drivers
//!
//! Provides verified drivers for various input devices:
//! - **Keyboard**: USB HID keyboard for navigation/media control
//! - **IR Remote**: Infrared remote (NEC, RC5, RC6 protocols)
//! - **Touch**: Touch event types (actual driver in display crates)
//!
//! # Usage
//!
//! ```no_run
//! use rpi4_input::{InputManager, RemoteOptions, InputEvent};
//!
//! let mut input = InputManager::new(RemoteOptions::all());
//!
//! loop {
//!     if let Some(event) = input.poll() {
//!         match event {
//!             InputEvent::Key(key) => { /* handle keyboard */ }
//!             InputEvent::Remote(ir) => { /* handle IR remote */ }
//!             InputEvent::Touch(touch) => { /* handle touch */ }
//!         }
//!     }
//! }
//! ```

#![no_std]
#![allow(dead_code)]

pub mod keyboard;
pub mod ir_remote;
pub mod touch;
pub mod uart;

pub use keyboard::{Keyboard, KeyCode, KeyEvent, KeyState, KeyModifiers};
pub use ir_remote::{IrRemote, IrButton, IrEvent, IrProtocol, ButtonMap};
pub use touch::{TouchEvent, TouchPoint};
pub use uart::Uart;

/// Unified input event that can come from any input source
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// Keyboard key event
    Key(KeyEvent),
    /// IR remote button event
    Remote(IrEvent),
    /// Touch event
    Touch(TouchEvent),
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
    /// UART serial input
    Uart,
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
    /// Enable UART serial input
    pub uart_enabled: bool,
    /// UART base address (virtual address mapped by Microkit)
    pub uart_base: usize,
    /// IR protocol to use
    pub ir_protocol: IrProtocol,
}

impl Default for RemoteOptions {
    fn default() -> Self {
        Self {
            keyboard_enabled: true,
            ir_remote_enabled: true,
            touch_enabled: true,
            uart_enabled: false,
            uart_base: uart::UART_BASE,
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
            uart_enabled: false,
            uart_base: uart::UART_BASE,
            ir_protocol: IrProtocol::Nec,
        }
    }

    /// Create options with only IR remote enabled
    pub const fn ir_remote_only() -> Self {
        Self {
            keyboard_enabled: false,
            ir_remote_enabled: true,
            touch_enabled: false,
            uart_enabled: false,
            uart_base: uart::UART_BASE,
            ir_protocol: IrProtocol::Nec,
        }
    }

    /// Create options with only touch enabled
    pub const fn touch_only() -> Self {
        Self {
            keyboard_enabled: false,
            ir_remote_enabled: false,
            touch_enabled: true,
            uart_enabled: false,
            uart_base: uart::UART_BASE,
            ir_protocol: IrProtocol::Nec,
        }
    }

    /// Create options with only UART serial input enabled
    pub const fn uart_only() -> Self {
        Self {
            keyboard_enabled: false,
            ir_remote_enabled: false,
            touch_enabled: false,
            uart_enabled: true,
            uart_base: uart::UART_BASE,
            ir_protocol: IrProtocol::Nec,
        }
    }

    /// Create options with UART at a specific virtual address
    pub const fn uart_at(base: usize) -> Self {
        Self {
            keyboard_enabled: false,
            ir_remote_enabled: false,
            touch_enabled: false,
            uart_enabled: true,
            uart_base: base,
            ir_protocol: IrProtocol::Nec,
        }
    }

    /// Create options with all inputs enabled
    pub const fn all() -> Self {
        Self {
            keyboard_enabled: true,
            ir_remote_enabled: true,
            touch_enabled: true,
            uart_enabled: true,
            uart_base: uart::UART_BASE,
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
    uart: Option<Uart>,
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
            uart: if options.uart_enabled {
                Some(Uart::with_base(options.uart_base))
            } else {
                None
            },
        }
    }

    /// Poll all enabled input sources for events
    pub fn poll(&mut self) -> Option<InputEvent> {
        // Check UART first (most common for serial console development)
        if let Some(ref mut uart) = self.uart {
            if let Some(event) = uart.poll() {
                return Some(InputEvent::Key(event));
            }
        }

        // Check keyboard
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

        self.uart = if options.uart_enabled {
            Some(Uart::with_base(options.uart_base))
        } else {
            None
        };
    }

    /// Get mutable access to keyboard driver (for injecting HID reports)
    pub fn keyboard_mut(&mut self) -> Option<&mut Keyboard> {
        self.keyboard.as_mut()
    }

    /// Get mutable access to IR remote driver (for processing edges)
    pub fn ir_remote_mut(&mut self) -> Option<&mut IrRemote> {
        self.ir_remote.as_mut()
    }

    /// Get mutable access to UART driver
    pub fn uart_mut(&mut self) -> Option<&mut Uart> {
        self.uart.as_mut()
    }
}
