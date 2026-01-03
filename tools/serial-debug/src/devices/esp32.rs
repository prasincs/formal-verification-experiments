//! ESP32 device profile
//!
//! Device profile for ESP32 microcontrollers serial debugging.

use super::profile::{DeviceProfile, SerialSettings, BootStage, ErrorPattern, BootFileCheck};
use once_cell::sync::Lazy;

/// ESP32 device profile
pub static ESP32_PROFILE: Lazy<DeviceProfile> = Lazy::new(|| {
    DeviceProfile {
        name: "ESP32".to_string(),
        id: "esp32".to_string(),
        description: "Espressif ESP32 series (ESP32, ESP32-S2, ESP32-S3, ESP32-C3)".to_string(),
        manufacturer: "Espressif Systems".to_string(),
        architecture: "xtensa".to_string(),
        serial: SerialSettings {
            baud_rate: 115200,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".to_string(),
            flow_control: "none".to_string(),
            alt_baud_rates: vec![9600, 19200, 38400, 57600, 230400, 460800, 921600, 1500000, 2000000],
        },
        boot_stages: vec![
            BootStage {
                name: "ROM Bootloader".to_string(),
                patterns: vec!["rst:".to_string(), "boot:".to_string()],
                description: "First-stage ROM bootloader".to_string(),
                expected_duration_secs: 1,
            },
            BootStage {
                name: "Second Stage Bootloader".to_string(),
                patterns: vec!["ESP-IDF".to_string(), "2nd stage bootloader".to_string()],
                description: "ESP-IDF second stage bootloader".to_string(),
                expected_duration_secs: 1,
            },
            BootStage {
                name: "Application".to_string(),
                patterns: vec!["app_main".to_string(), "Starting".to_string()],
                description: "Application starting".to_string(),
                expected_duration_secs: 2,
            },
            BootStage {
                name: "WiFi Init".to_string(),
                patterns: vec!["wifi".to_string(), "WiFi".to_string()],
                description: "WiFi initialization".to_string(),
                expected_duration_secs: 3,
            },
        ],
        error_patterns: vec![
            // Reset reasons
            ErrorPattern {
                pattern: "rst:0x1 (POWERON_RESET)".to_string(),
                is_regex: false,
                severity: "info".to_string(),
                description: "Power-on reset".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "rst:0x3 (SW_RESET)".to_string(),
                is_regex: false,
                severity: "info".to_string(),
                description: "Software reset".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "rst:0xc (SW_CPU_RESET)".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "Software CPU reset (often from exception)".to_string(),
                suggestion: Some("Check for stack overflow or panic".to_string()),
            },
            // Panics and exceptions
            ErrorPattern {
                pattern: "Guru Meditation Error".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "ESP-IDF panic".to_string(),
                suggestion: Some("Check backtrace for crash location".to_string()),
            },
            ErrorPattern {
                pattern: "abort()".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Program abort".to_string(),
                suggestion: Some("Check assertion failures or panic calls".to_string()),
            },
            ErrorPattern {
                pattern: "Stack overflow".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Task stack overflow".to_string(),
                suggestion: Some("Increase task stack size in xTaskCreate".to_string()),
            },
            ErrorPattern {
                pattern: "LoadProhibited".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Invalid memory read".to_string(),
                suggestion: Some("Check for null pointer access".to_string()),
            },
            ErrorPattern {
                pattern: "StoreProhibited".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Invalid memory write".to_string(),
                suggestion: Some("Check for null pointer or const memory write".to_string()),
            },
            ErrorPattern {
                pattern: "InstrFetchProhibited".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Invalid instruction fetch".to_string(),
                suggestion: Some("Check for corrupted function pointers".to_string()),
            },
            // Flash errors
            ErrorPattern {
                pattern: "flash read err".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Flash read error".to_string(),
                suggestion: Some("Check flash connection or re-flash firmware".to_string()),
            },
            ErrorPattern {
                pattern: "invalid header".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Invalid app header".to_string(),
                suggestion: Some("Flash may be corrupted, try erasing and re-flashing".to_string()),
            },
            // Watchdog
            ErrorPattern {
                pattern: "Task watchdog got triggered".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Task watchdog timeout".to_string(),
                suggestion: Some("Check for blocking operations in task or increase timeout".to_string()),
            },
            ErrorPattern {
                pattern: "Interrupt wdt timeout".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Interrupt watchdog timeout".to_string(),
                suggestion: Some("Check for long-running interrupt handlers".to_string()),
            },
            // WiFi/Network
            ErrorPattern {
                pattern: "E (wifi".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "WiFi error".to_string(),
                suggestion: Some("Check WiFi configuration and signal strength".to_string()),
            },
            // Brownout
            ErrorPattern {
                pattern: "Brownout detector".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Power supply brownout".to_string(),
                suggestion: Some("Check power supply voltage and stability".to_string()),
            },
        ],
        success_patterns: vec![
            "Ready".to_string(),
            "Connected".to_string(),
            "IP:".to_string(),
        ],
        usb_vendor_ids: vec![
            0x303a, // Espressif
            0x10c4, // Silicon Labs CP210x (common on DevKits)
            0x1a86, // WCH CH340 (common on cheap boards)
        ],
        usb_product_ids: vec![
            0x1001, // ESP32-S2
            0x80d1, // ESP32-S3
            0xea60, // CP2102
            0x7523, // CH340
        ],
        boot_files: vec![],
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_esp32_profile() {
        let profile = &*ESP32_PROFILE;
        assert_eq!(profile.id, "esp32");
        assert_eq!(profile.serial.baud_rate, 115200);
    }

    #[test]
    fn test_esp32_error_detection() {
        let profile = &*ESP32_PROFILE;
        let error = profile.match_error("Guru Meditation Error: Core 0 panic'ed");
        assert!(error.is_some());
    }
}
