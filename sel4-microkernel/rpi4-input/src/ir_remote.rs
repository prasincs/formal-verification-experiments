//! IR Remote receiver driver
//!
//! Supports infrared remote controls using common protocols:
//! - NEC (most common, used by many TV remotes)
//! - RC5 (Philips)
//! - RC6 (Microsoft MCE remotes)
//!
//! Connects to a GPIO pin via an IR receiver module (e.g., TSOP38238)

/// Default GPIO pin for IR receiver (active low)
pub const IR_RECEIVER_PIN: u8 = 4;

/// IR protocol types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrProtocol {
    /// NEC protocol (32-bit, most common)
    Nec,
    /// NEC extended protocol (for larger address space)
    NecExtended,
    /// Philips RC5 protocol (14-bit)
    Rc5,
    /// Philips RC6 protocol (used by Microsoft MCE remotes)
    Rc6,
    /// Samsung protocol
    Samsung,
    /// Sony SIRC protocol
    Sony,
}

/// Common IR remote button codes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IrButton {
    // Power
    /// Power on/off
    Power = 0x00,

    // Navigation
    /// Up
    Up = 0x01,
    /// Down
    Down = 0x02,
    /// Left
    Left = 0x03,
    /// Right
    Right = 0x04,
    /// OK/Select/Enter
    Ok = 0x05,
    /// Back/Return
    Back = 0x06,
    /// Menu
    Menu = 0x07,
    /// Home
    Home = 0x08,

    // Numbers
    /// 0
    Num0 = 0x10,
    /// 1
    Num1 = 0x11,
    /// 2
    Num2 = 0x12,
    /// 3
    Num3 = 0x13,
    /// 4
    Num4 = 0x14,
    /// 5
    Num5 = 0x15,
    /// 6
    Num6 = 0x16,
    /// 7
    Num7 = 0x17,
    /// 8
    Num8 = 0x18,
    /// 9
    Num9 = 0x19,

    // Volume/Channel
    /// Volume Up
    VolumeUp = 0x20,
    /// Volume Down
    VolumeDown = 0x21,
    /// Mute
    Mute = 0x22,
    /// Channel Up
    ChannelUp = 0x23,
    /// Channel Down
    ChannelDown = 0x24,

    // Media controls
    /// Play
    Play = 0x30,
    /// Pause
    Pause = 0x31,
    /// Stop
    Stop = 0x32,
    /// Fast Forward
    FastForward = 0x33,
    /// Rewind
    Rewind = 0x34,
    /// Skip Next
    SkipNext = 0x35,
    /// Skip Previous
    SkipPrev = 0x36,
    /// Record
    Record = 0x37,

    // Color buttons (common on European remotes)
    /// Red button
    Red = 0x40,
    /// Green button
    Green = 0x41,
    /// Yellow button
    Yellow = 0x42,
    /// Blue button
    Blue = 0x43,

    // Info/Guide
    /// Info/Display
    Info = 0x50,
    /// Guide/EPG
    Guide = 0x51,
    /// Input/Source
    Input = 0x52,
    /// Subtitle/CC
    Subtitle = 0x53,
    /// Audio/Language
    Audio = 0x54,

    /// Unknown button
    Unknown = 0xFF,
}

impl IrButton {
    /// Check if this is a navigation button
    pub fn is_navigation(&self) -> bool {
        matches!(
            self,
            IrButton::Up
                | IrButton::Down
                | IrButton::Left
                | IrButton::Right
                | IrButton::Ok
                | IrButton::Back
                | IrButton::Menu
                | IrButton::Home
        )
    }

    /// Check if this is a number button
    pub fn is_number(&self) -> bool {
        matches!(
            self,
            IrButton::Num0
                | IrButton::Num1
                | IrButton::Num2
                | IrButton::Num3
                | IrButton::Num4
                | IrButton::Num5
                | IrButton::Num6
                | IrButton::Num7
                | IrButton::Num8
                | IrButton::Num9
        )
    }

    /// Convert number button to digit (0-9), returns None for non-number buttons
    pub fn to_digit(&self) -> Option<u8> {
        match self {
            IrButton::Num0 => Some(0),
            IrButton::Num1 => Some(1),
            IrButton::Num2 => Some(2),
            IrButton::Num3 => Some(3),
            IrButton::Num4 => Some(4),
            IrButton::Num5 => Some(5),
            IrButton::Num6 => Some(6),
            IrButton::Num7 => Some(7),
            IrButton::Num8 => Some(8),
            IrButton::Num9 => Some(9),
            _ => None,
        }
    }

    /// Check if this is a media control button
    pub fn is_media(&self) -> bool {
        matches!(
            self,
            IrButton::Play
                | IrButton::Pause
                | IrButton::Stop
                | IrButton::FastForward
                | IrButton::Rewind
                | IrButton::SkipNext
                | IrButton::SkipPrev
                | IrButton::Record
        )
    }

    /// Check if this is a color button
    pub fn is_color(&self) -> bool {
        matches!(
            self,
            IrButton::Red | IrButton::Green | IrButton::Yellow | IrButton::Blue
        )
    }
}

/// IR remote event
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrEvent {
    /// The button that was pressed
    pub button: IrButton,
    /// Remote address (device identifier)
    pub address: u16,
    /// Raw command code
    pub command: u8,
    /// Whether this is a repeat (button held down)
    pub is_repeat: bool,
}

/// NEC protocol timing constants (in microseconds)
mod nec_timing {
    /// Lead pulse duration
    pub const LEAD_PULSE: u32 = 9000;
    /// Lead space duration
    pub const LEAD_SPACE: u32 = 4500;
    /// Repeat space duration
    pub const REPEAT_SPACE: u32 = 2250;
    /// Bit pulse duration
    pub const BIT_PULSE: u32 = 562;
    /// Zero bit space duration
    pub const ZERO_SPACE: u32 = 562;
    /// One bit space duration
    pub const ONE_SPACE: u32 = 1687;
    /// Timing tolerance percentage
    pub const TOLERANCE: u32 = 25;
}

/// Decoder state for NEC protocol
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecoderState {
    /// Waiting for lead pulse
    Idle,
    /// Received lead pulse, waiting for space
    LeadPulse,
    /// Receiving data bits
    Data { bits_received: u8, data: u32 },
    /// Repeat code detected
    Repeat,
}

/// IR Remote receiver driver
pub struct IrRemote {
    protocol: IrProtocol,
    gpio_pin: u8,
    state: DecoderState,
    last_edge_time: u32,
    last_command: Option<IrEvent>,
    /// Custom button mapping (NEC command -> IrButton)
    button_map: ButtonMap,
}

/// Button mapping for NEC protocol
/// Maps raw command codes to IrButton values
#[derive(Clone, Copy)]
pub struct ButtonMap {
    /// Mapping table: index is command code, value is IrButton
    /// Using a fixed array for common remotes
    map: [IrButton; 256],
}

impl Default for ButtonMap {
    fn default() -> Self {
        let mut map = [IrButton::Unknown; 256];

        // Common NEC remote mapping (e.g., generic Chinese remotes)
        // These are typical codes - actual codes vary by remote
        map[0x45] = IrButton::Power;
        map[0x46] = IrButton::Menu;
        map[0x47] = IrButton::Mute;
        map[0x44] = IrButton::Back;
        map[0x40] = IrButton::SkipPrev;
        map[0x43] = IrButton::SkipNext;
        map[0x07] = IrButton::VolumeDown;
        map[0x15] = IrButton::VolumeUp;
        map[0x09] = IrButton::ChannelUp;
        map[0x19] = IrButton::ChannelDown;
        map[0x16] = IrButton::Num0;
        map[0x0C] = IrButton::Num1;
        map[0x18] = IrButton::Num2;
        map[0x5E] = IrButton::Num3;
        map[0x08] = IrButton::Num4;
        map[0x1C] = IrButton::Num5;
        map[0x5A] = IrButton::Num6;
        map[0x42] = IrButton::Num7;
        map[0x52] = IrButton::Num8;
        map[0x4A] = IrButton::Num9;

        // Navigation (common across many remotes)
        map[0x18] = IrButton::Up;
        map[0x52] = IrButton::Down;
        map[0x08] = IrButton::Left;
        map[0x5A] = IrButton::Right;
        map[0x1C] = IrButton::Ok;

        Self { map }
    }
}

impl ButtonMap {
    /// Create a new empty button map
    pub fn new() -> Self {
        Self {
            map: [IrButton::Unknown; 256],
        }
    }

    /// Set a button mapping
    pub fn set(&mut self, command: u8, button: IrButton) {
        self.map[command as usize] = button;
    }

    /// Get the button for a command
    pub fn get(&self, command: u8) -> IrButton {
        self.map[command as usize]
    }

    /// Create a Samsung TV remote mapping
    pub fn samsung_tv() -> Self {
        let mut map = Self::new();
        map.set(0x02, IrButton::Power);
        map.set(0x07, IrButton::VolumeUp);
        map.set(0x0B, IrButton::VolumeDown);
        map.set(0x0F, IrButton::Mute);
        map.set(0x12, IrButton::ChannelUp);
        map.set(0x10, IrButton::ChannelDown);
        map.set(0x60, IrButton::Up);
        map.set(0x61, IrButton::Down);
        map.set(0x65, IrButton::Left);
        map.set(0x62, IrButton::Right);
        map.set(0x68, IrButton::Ok);
        map.set(0x58, IrButton::Back);
        map.set(0x1A, IrButton::Menu);
        map.set(0x79, IrButton::Home);
        map
    }

    /// Create an LG TV remote mapping
    pub fn lg_tv() -> Self {
        let mut map = Self::new();
        map.set(0x08, IrButton::Power);
        map.set(0x02, IrButton::VolumeUp);
        map.set(0x03, IrButton::VolumeDown);
        map.set(0x09, IrButton::Mute);
        map.set(0x00, IrButton::ChannelUp);
        map.set(0x01, IrButton::ChannelDown);
        map.set(0x40, IrButton::Up);
        map.set(0x41, IrButton::Down);
        map.set(0x07, IrButton::Left);
        map.set(0x06, IrButton::Right);
        map.set(0x44, IrButton::Ok);
        map.set(0x28, IrButton::Back);
        map.set(0x43, IrButton::Menu);
        map.set(0x42, IrButton::Home);
        map
    }
}

impl IrRemote {
    /// Create a new IR remote receiver
    pub fn new(protocol: IrProtocol) -> Self {
        Self::with_pin(protocol, IR_RECEIVER_PIN)
    }

    /// Create a new IR remote receiver with custom GPIO pin
    pub const fn with_pin(protocol: IrProtocol, gpio_pin: u8) -> Self {
        Self {
            protocol,
            gpio_pin,
            state: DecoderState::Idle,
            last_edge_time: 0,
            last_command: None,
            button_map: ButtonMap {
                map: [IrButton::Unknown; 256],
            },
        }
    }

    /// Set the button mapping
    pub fn set_button_map(&mut self, map: ButtonMap) {
        self.button_map = map;
    }

    /// Get the GPIO pin used for receiving
    pub fn gpio_pin(&self) -> u8 {
        self.gpio_pin
    }

    /// Get the current protocol
    pub fn protocol(&self) -> IrProtocol {
        self.protocol
    }

    /// Poll for IR remote events
    pub fn poll(&mut self) -> Option<IrEvent> {
        // TODO: Read GPIO pin and decode IR signal
        // This requires integration with GPIO driver and timer
        // for measuring pulse durations
        None
    }

    /// Process a timing edge (for interrupt-driven operation)
    /// `duration` is the time in microseconds since the last edge
    /// `is_mark` is true for IR signal present (typically low from receiver)
    pub fn process_edge(&mut self, duration: u32, is_mark: bool) -> Option<IrEvent> {
        match self.protocol {
            IrProtocol::Nec | IrProtocol::NecExtended => {
                self.decode_nec_edge(duration, is_mark)
            }
            _ => {
                // Other protocols not yet implemented
                None
            }
        }
    }

    /// Decode NEC protocol edge
    fn decode_nec_edge(&mut self, duration: u32, is_mark: bool) -> Option<IrEvent> {
        let tolerance = |expected: u32| -> bool {
            let margin = expected * nec_timing::TOLERANCE / 100;
            duration >= expected.saturating_sub(margin)
                && duration <= expected.saturating_add(margin)
        };

        match self.state {
            DecoderState::Idle => {
                // Looking for 9ms lead pulse (mark)
                if is_mark && tolerance(nec_timing::LEAD_PULSE) {
                    self.state = DecoderState::LeadPulse;
                }
                None
            }

            DecoderState::LeadPulse => {
                if !is_mark {
                    if tolerance(nec_timing::LEAD_SPACE) {
                        // Normal command - start receiving data
                        self.state = DecoderState::Data {
                            bits_received: 0,
                            data: 0,
                        };
                    } else if tolerance(nec_timing::REPEAT_SPACE) {
                        // Repeat code
                        self.state = DecoderState::Idle;
                        if let Some(mut last) = self.last_command {
                            last.is_repeat = true;
                            return Some(last);
                        }
                    } else {
                        self.state = DecoderState::Idle;
                    }
                } else {
                    self.state = DecoderState::Idle;
                }
                None
            }

            DecoderState::Data { bits_received, data } => {
                if is_mark {
                    // Mark should always be ~562µs in NEC
                    if !tolerance(nec_timing::BIT_PULSE) {
                        self.state = DecoderState::Idle;
                    }
                    None
                } else {
                    // Space determines bit value
                    let bit = if tolerance(nec_timing::ONE_SPACE) {
                        1u32
                    } else if tolerance(nec_timing::ZERO_SPACE) {
                        0u32
                    } else {
                        self.state = DecoderState::Idle;
                        return None;
                    };

                    let new_data = data | (bit << bits_received);
                    let new_bits = bits_received + 1;

                    if new_bits >= 32 {
                        // Complete message received
                        self.state = DecoderState::Idle;

                        // Decode NEC format: address (8), ~address (8), command (8), ~command (8)
                        let addr_lo = (new_data & 0xFF) as u8;
                        let addr_hi = ((new_data >> 8) & 0xFF) as u8;
                        let cmd = ((new_data >> 16) & 0xFF) as u8;
                        let cmd_inv = ((new_data >> 24) & 0xFF) as u8;

                        // Verify command (cmd should be inverse of cmd_inv)
                        if cmd != !cmd_inv {
                            return None;
                        }

                        let address = match self.protocol {
                            IrProtocol::NecExtended => {
                                // Extended: full 16-bit address
                                ((addr_hi as u16) << 8) | (addr_lo as u16)
                            }
                            _ => {
                                // Standard: 8-bit address (verify inverse)
                                if addr_lo != !addr_hi {
                                    return None;
                                }
                                addr_lo as u16
                            }
                        };

                        let event = IrEvent {
                            button: self.button_map.get(cmd),
                            address,
                            command: cmd,
                            is_repeat: false,
                        };

                        self.last_command = Some(event);
                        return Some(event);
                    } else {
                        self.state = DecoderState::Data {
                            bits_received: new_bits,
                            data: new_data,
                        };
                    }
                    None
                }
            }

            DecoderState::Repeat => {
                self.state = DecoderState::Idle;
                None
            }
        }
    }

    /// Reset the decoder state
    pub fn reset(&mut self) {
        self.state = DecoderState::Idle;
    }

    /// Check if we have a valid last command (for repeat detection)
    pub fn has_last_command(&self) -> bool {
        self.last_command.is_some()
    }
}

impl Default for IrRemote {
    fn default() -> Self {
        Self::new(IrProtocol::Nec)
    }
}
