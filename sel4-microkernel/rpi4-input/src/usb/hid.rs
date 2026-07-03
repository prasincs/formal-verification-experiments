//! USB standard + HID boot-keyboard protocol helpers
//!
//! Pure, allocation-free helpers for enumerating a boot-protocol HID keyboard:
//! building the 8-byte SETUP packets for the control transfers used during
//! enumeration, and parsing the returned descriptors to locate the interrupt-IN
//! endpoint that delivers the 8-byte boot report.
//!
//! The boot keyboard report (HID 1.11, Appendix B.1) is:
//!
//! ```text
//! byte 0: modifier bitmap (ctrl/shift/alt/gui, left+right)
//! byte 1: reserved
//! bytes 2..8: up to six pressed key usage codes
//! ```
//!
//! Decoding that report into key events is handled by
//! [`crate::keyboard::Keyboard::process_hid_report`]; this module only gets the
//! device into boot protocol and finds the endpoint to poll.

/// `bmRequestType` direction bit: device → host.
pub const DIR_IN: u8 = 0x80;
/// `bmRequestType` direction bit: host → device.
pub const DIR_OUT: u8 = 0x00;

/// `bmRequestType` type field: standard request.
pub const TYPE_STANDARD: u8 = 0x00;
/// `bmRequestType` type field: class request.
pub const TYPE_CLASS: u8 = 0x20;

/// `bmRequestType` recipient: device.
pub const RECIP_DEVICE: u8 = 0x00;
/// `bmRequestType` recipient: interface.
pub const RECIP_INTERFACE: u8 = 0x01;

// Standard request codes (USB 2.0 table 9-4).
const REQ_SET_ADDRESS: u8 = 0x05;
const REQ_GET_DESCRIPTOR: u8 = 0x06;
const REQ_SET_CONFIGURATION: u8 = 0x09;

// HID class request codes (HID 1.11 section 7.2).
const REQ_SET_IDLE: u8 = 0x0A;
const REQ_SET_PROTOCOL: u8 = 0x0B;

/// Descriptor type: DEVICE.
pub const DESC_DEVICE: u8 = 0x01;
/// Descriptor type: CONFIGURATION.
pub const DESC_CONFIGURATION: u8 = 0x02;
/// Descriptor type within a configuration: INTERFACE.
pub const DESC_INTERFACE: u8 = 0x04;
/// Descriptor type within a configuration: ENDPOINT.
pub const DESC_ENDPOINT: u8 = 0x05;

/// USB HID class code (`bInterfaceClass`).
pub const CLASS_HID: u8 = 0x03;
/// HID boot subclass (`bInterfaceSubClass`).
pub const SUBCLASS_BOOT: u8 = 0x01;
/// HID keyboard protocol (`bInterfaceProtocol`).
pub const PROTOCOL_KEYBOARD: u8 = 0x01;

/// HID protocol selector for SET_PROTOCOL: boot protocol.
pub const HID_PROTOCOL_BOOT: u16 = 0;

/// A USB SETUP packet (8 bytes, little-endian on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupPacket {
    /// Request type/direction/recipient bitmap.
    pub request_type: u8,
    /// Request code.
    pub request: u8,
    /// Value (request-specific).
    pub value: u16,
    /// Index (request-specific: interface/endpoint/language).
    pub index: u16,
    /// Number of bytes in the data stage.
    pub length: u16,
}

impl SetupPacket {
    /// Serialize to the 8-byte little-endian on-the-wire form.
    pub fn to_bytes(&self) -> [u8; 8] {
        [
            self.request_type,
            self.request,
            self.value as u8,
            (self.value >> 8) as u8,
            self.index as u8,
            (self.index >> 8) as u8,
            self.length as u8,
            (self.length >> 8) as u8,
        ]
    }

    /// GET_DESCRIPTOR(type, index) into a buffer of `length` bytes.
    pub fn get_descriptor(desc_type: u8, desc_index: u8, length: u16) -> Self {
        Self {
            request_type: DIR_IN | TYPE_STANDARD | RECIP_DEVICE,
            request: REQ_GET_DESCRIPTOR,
            value: ((desc_type as u16) << 8) | desc_index as u16,
            index: 0,
            length,
        }
    }

    /// SET_ADDRESS(addr).
    pub fn set_address(addr: u8) -> Self {
        Self {
            request_type: DIR_OUT | TYPE_STANDARD | RECIP_DEVICE,
            request: REQ_SET_ADDRESS,
            value: addr as u16,
            index: 0,
            length: 0,
        }
    }

    /// SET_CONFIGURATION(config_value).
    pub fn set_configuration(config: u8) -> Self {
        Self {
            request_type: DIR_OUT | TYPE_STANDARD | RECIP_DEVICE,
            request: REQ_SET_CONFIGURATION,
            value: config as u16,
            index: 0,
            length: 0,
        }
    }

    /// SET_PROTOCOL(boot) on the given HID interface.
    pub fn set_boot_protocol(interface: u8) -> Self {
        Self {
            request_type: DIR_OUT | TYPE_CLASS | RECIP_INTERFACE,
            request: REQ_SET_PROTOCOL,
            value: HID_PROTOCOL_BOOT,
            index: interface as u16,
            length: 0,
        }
    }

    /// SET_IDLE(duration, report) on the given HID interface.
    ///
    /// `duration` is in 4ms units; 0 means "only report on change", which is
    /// what a boot keyboard wants (no periodic duplicate reports).
    pub fn set_idle(interface: u8, duration: u8, report_id: u8) -> Self {
        Self {
            request_type: DIR_OUT | TYPE_CLASS | RECIP_INTERFACE,
            request: REQ_SET_IDLE,
            value: ((duration as u16) << 8) | report_id as u16,
            index: interface as u16,
            length: 0,
        }
    }
}

/// The interrupt-IN endpoint of a boot keyboard, located in a configuration
/// descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootKeyboardEndpoint {
    /// The owning interface number (for SET_PROTOCOL / SET_IDLE).
    pub interface: u8,
    /// Endpoint number (low 4 bits of `bEndpointAddress`).
    pub endpoint: u8,
    /// Endpoint max packet size (bytes).
    pub max_packet: u16,
    /// Polling interval (`bInterval`, frames).
    pub interval: u8,
}

/// Scan a configuration descriptor blob for a boot-protocol HID keyboard's
/// interrupt-IN endpoint.
///
/// Walks the descriptor list by its length/type bytes, tracks the most recent
/// HID-keyboard interface, and returns the first interrupt-IN endpoint that
/// follows it. Returns `None` if the blob is malformed or contains no such
/// interface/endpoint.
pub fn find_boot_keyboard_endpoint(config: &[u8]) -> Option<BootKeyboardEndpoint> {
    let mut i = 0usize;
    let mut current_iface: Option<u8> = None;

    while i + 2 <= config.len() {
        let len = config[i] as usize;
        // A zero-length descriptor would loop forever; a descriptor running past
        // the buffer end is malformed.
        if len < 2 || i + len > config.len() {
            return None;
        }
        let dtype = config[i + 1];

        match dtype {
            DESC_INTERFACE if len >= 9 => {
                let class = config[i + 5];
                let subclass = config[i + 6];
                let protocol = config[i + 7];
                current_iface = if class == CLASS_HID
                    && subclass == SUBCLASS_BOOT
                    && protocol == PROTOCOL_KEYBOARD
                {
                    Some(config[i + 2])
                } else {
                    None
                };
            }
            DESC_ENDPOINT if len >= 7 => {
                if let Some(iface) = current_iface {
                    let addr = config[i + 2];
                    let attributes = config[i + 3];
                    let is_in = addr & DIR_IN != 0;
                    let is_interrupt = (attributes & 0x03) == 0x03;
                    if is_in && is_interrupt {
                        let max_packet = config[i + 4] as u16 | ((config[i + 5] as u16) << 8);
                        return Some(BootKeyboardEndpoint {
                            interface: iface,
                            endpoint: addr & 0x0F,
                            max_packet,
                            interval: config[i + 6],
                        });
                    }
                }
            }
            _ => {}
        }

        i += len;
    }
    None
}

/// The `wTotalLength` field of a configuration descriptor header (offset 2).
///
/// Returned so the caller can issue a second GET_DESCRIPTOR that fetches the
/// full configuration (interfaces + endpoints), not just the 9-byte header.
pub fn config_total_length(header: &[u8]) -> Option<u16> {
    if header.len() < 4 || header[1] != DESC_CONFIGURATION {
        return None;
    }
    Some(header[2] as u16 | ((header[3] as u16) << 8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_packet_serialization() {
        let s = SetupPacket::get_descriptor(DESC_DEVICE, 0, 18);
        assert_eq!(
            s.to_bytes(),
            [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00]
        );

        let a = SetupPacket::set_address(7);
        assert_eq!(
            a.to_bytes(),
            [0x00, 0x05, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00]
        );

        let c = SetupPacket::set_configuration(1);
        assert_eq!(
            c.to_bytes(),
            [0x00, 0x09, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn hid_requests_target_interface() {
        let p = SetupPacket::set_boot_protocol(0);
        assert_eq!(p.request_type, 0x21); // OUT | CLASS | INTERFACE
        assert_eq!(p.request, REQ_SET_PROTOCOL);
        assert_eq!(p.value, HID_PROTOCOL_BOOT);

        let idle = SetupPacket::set_idle(2, 0, 0);
        assert_eq!(idle.request_type, 0x21);
        assert_eq!(idle.request, REQ_SET_IDLE);
        assert_eq!(idle.index, 2);
        assert_eq!(idle.value, 0);
    }

    /// A realistic single-interface boot-keyboard configuration descriptor.
    fn sample_config() -> [u8; 34] {
        [
            // Configuration descriptor (9 bytes)
            0x09,
            DESC_CONFIGURATION,
            0x22,
            0x00,
            0x01,
            0x01,
            0x00,
            0xA0,
            0x32,
            // Interface descriptor (9 bytes): HID / boot / keyboard
            0x09,
            DESC_INTERFACE,
            0x00,
            0x00,
            0x01,
            CLASS_HID,
            SUBCLASS_BOOT,
            PROTOCOL_KEYBOARD,
            0x00,
            // HID descriptor (9 bytes)
            0x09,
            0x21,
            0x11,
            0x01,
            0x00,
            0x01,
            0x22,
            0x3F,
            0x00,
            // Endpoint descriptor (7 bytes): EP1 IN, interrupt, 8-byte, 10ms
            0x07,
            DESC_ENDPOINT,
            0x81,
            0x03,
            0x08,
            0x00,
            0x0A,
        ]
    }

    #[test]
    fn finds_boot_keyboard_endpoint() {
        let cfg = sample_config();
        let ep = find_boot_keyboard_endpoint(&cfg).expect("endpoint found");
        assert_eq!(ep.interface, 0);
        assert_eq!(ep.endpoint, 1);
        assert_eq!(ep.max_packet, 8);
        assert_eq!(ep.interval, 10);
    }

    #[test]
    fn ignores_non_keyboard_interface() {
        let mut cfg = sample_config();
        // Flip bInterfaceProtocol (offset 7 within the interface descriptor,
        // which starts at index 9) from keyboard to "mouse" (2).
        cfg[16] = 0x02;
        assert!(find_boot_keyboard_endpoint(&cfg).is_none());
    }

    #[test]
    fn rejects_out_endpoint() {
        let mut cfg = sample_config();
        // bEndpointAddress is at index 29 (endpoint descriptor starts at 27).
        // Clear the direction bit to make it an OUT endpoint.
        cfg[29] = 0x01;
        assert!(find_boot_keyboard_endpoint(&cfg).is_none());
    }

    #[test]
    fn malformed_zero_length_descriptor_terminates() {
        // A zero length byte must not spin forever.
        let bad = [0x00u8, DESC_CONFIGURATION, 0x00, 0x00];
        assert!(find_boot_keyboard_endpoint(&bad).is_none());
    }

    #[test]
    fn truncated_descriptor_rejected() {
        // Claims length 9 but only 4 bytes present.
        let bad = [0x09u8, DESC_INTERFACE, 0x00, 0x00];
        assert!(find_boot_keyboard_endpoint(&bad).is_none());
    }

    #[test]
    fn parses_config_total_length() {
        let cfg = sample_config();
        assert_eq!(config_total_length(&cfg), Some(0x22));
        // Wrong descriptor type is rejected.
        let dev = [0x12u8, DESC_DEVICE, 0x00, 0x02];
        assert_eq!(config_total_length(&dev), None);
    }
}
