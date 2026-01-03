//! Generic device profile
//!
//! A generic device profile for unknown or unsupported devices.
//! Uses common serial settings and basic error pattern detection.

use super::profile::{DeviceProfile, SerialSettings, BootStage, ErrorPattern, BootFileCheck};
use once_cell::sync::Lazy;

/// Generic device profile
pub static GENERIC_PROFILE: Lazy<DeviceProfile> = Lazy::new(|| {
    DeviceProfile {
        name: "Generic Device".to_string(),
        id: "generic".to_string(),
        description: "Generic serial device with common settings".to_string(),
        manufacturer: "Unknown".to_string(),
        architecture: "unknown".to_string(),
        serial: SerialSettings {
            baud_rate: 115200,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".to_string(),
            flow_control: "none".to_string(),
            alt_baud_rates: vec![
                300, 1200, 2400, 4800, 9600, 19200, 38400, 57600,
                115200, 230400, 460800, 500000, 576000, 921600, 1000000,
            ],
        },
        boot_stages: vec![
            BootStage {
                name: "Boot".to_string(),
                patterns: vec![
                    "boot".to_string(),
                    "Boot".to_string(),
                    "BOOT".to_string(),
                    "Starting".to_string(),
                    "Initializing".to_string(),
                ],
                description: "Device boot sequence".to_string(),
                expected_duration_secs: 5,
            },
            BootStage {
                name: "Ready".to_string(),
                patterns: vec![
                    "Ready".to_string(),
                    "ready".to_string(),
                    ">".to_string(),
                    "#".to_string(),
                    "$".to_string(),
                ],
                description: "Device ready for input".to_string(),
                expected_duration_secs: 0,
            },
        ],
        error_patterns: vec![
            ErrorPattern {
                pattern: "error".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Error detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "Error".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Error detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "ERROR".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Error detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "fail".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Failure detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "Fail".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Failure detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "FAIL".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Failure detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "panic".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Panic detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "Panic".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Panic detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "PANIC".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Panic detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "warning".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "Warning detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "Warning".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "Warning detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "WARNING".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "Warning detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "timeout".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "Timeout detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "Timeout".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "Timeout detected".to_string(),
                suggestion: None,
            },
        ],
        success_patterns: vec![
            "OK".to_string(),
            "ok".to_string(),
            "success".to_string(),
            "Success".to_string(),
            "done".to_string(),
            "Done".to_string(),
            "ready".to_string(),
            "Ready".to_string(),
        ],
        usb_vendor_ids: vec![
            0x0403, // FTDI
            0x10c4, // Silicon Labs
            0x1a86, // WCH
            0x067b, // Prolific
        ],
        usb_product_ids: vec![
            0x6001, // FTDI FT232
            0xea60, // CP2102
            0x7523, // CH340
            0x2303, // PL2303
        ],
        boot_files: vec![],
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generic_profile() {
        let profile = &*GENERIC_PROFILE;
        assert_eq!(profile.id, "generic");
        assert_eq!(profile.serial.baud_rate, 115200);
    }
}
