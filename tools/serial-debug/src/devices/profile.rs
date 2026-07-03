//! Device profile definitions
//!
//! Defines the structure for device profiles used in serial debugging.

use serde::{Deserialize, Serialize};

/// Serial port settings for a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialSettings {
    /// Default baud rate
    pub baud_rate: u32,
    /// Data bits (typically 8)
    pub data_bits: u8,
    /// Stop bits (1 or 2)
    pub stop_bits: u8,
    /// Parity ("none", "even", "odd")
    pub parity: String,
    /// Flow control ("none", "hardware", "software")
    pub flow_control: String,
    /// Common alternative baud rates
    pub alt_baud_rates: Vec<u32>,
}

impl Default for SerialSettings {
    fn default() -> Self {
        Self {
            baud_rate: 115200,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".to_string(),
            flow_control: "none".to_string(),
            alt_baud_rates: vec![9600, 19200, 38400, 57600, 230400, 460800, 921600],
        }
    }
}

/// Boot stage definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootStage {
    /// Stage name (e.g., "Bootloader", "Kernel", "Init")
    pub name: String,
    /// Patterns that indicate this stage has started
    pub patterns: Vec<String>,
    /// Description of the stage
    pub description: String,
    /// Expected duration in seconds (0 = unknown)
    pub expected_duration_secs: u32,
}

/// Error pattern for detection (case-insensitive substring match)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPattern {
    /// Substring to match (matched case-insensitively)
    pub pattern: String,
    /// Severity: "error", "warning", "info"
    pub severity: String,
    /// Description or suggestion when this error is detected
    pub description: String,
    /// Suggested fix or next steps
    pub suggestion: Option<String>,
}

/// Complete device profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    /// Device name
    pub name: String,
    /// Short identifier (e.g., "rpi4")
    pub id: String,
    /// Device description
    pub description: String,
    /// Device manufacturer
    pub manufacturer: String,
    /// Serial settings
    pub serial: SerialSettings,
    /// Boot stages
    pub boot_stages: Vec<BootStage>,
    /// Known error patterns
    pub error_patterns: Vec<ErrorPattern>,
    /// Success patterns
    pub success_patterns: Vec<String>,
    /// USB vendor IDs for auto-detection
    pub usb_vendor_ids: Vec<u16>,
    /// USB product IDs for auto-detection (paired with vendor IDs)
    pub usb_product_ids: Vec<u16>,
    /// Boot partition files to check (for devices with boot partitions)
    pub boot_files: Vec<BootFileCheck>,
    /// Architecture (arm64, arm32, xtensa, etc.)
    pub architecture: String,
}

/// Boot file check definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootFileCheck {
    /// File name
    pub name: String,
    /// Whether the file is required
    pub required: bool,
    /// Description
    pub description: String,
}

// Matching methods are only exercised by the serial monitor, which is
// behind the `serial` feature; keep them available on default builds too.
#[cfg_attr(not(feature = "serial"), allow(dead_code))]
impl DeviceProfile {
    /// Check if a line matches any boot stage (case-sensitive substring match)
    pub fn match_boot_stage(&self, line: &str) -> Option<&BootStage> {
        self.boot_stages.iter().find(|stage| {
            stage.patterns.iter().any(|p| line.contains(p))
        })
    }

    /// Check if a line matches any error pattern (case-insensitive substring match)
    pub fn match_error(&self, line: &str) -> Option<&ErrorPattern> {
        let line_lower = line.to_lowercase();
        self.error_patterns
            .iter()
            .find(|err| line_lower.contains(&err.pattern.to_lowercase()))
    }

    /// Check if a line indicates success
    pub fn is_success(&self, line: &str) -> bool {
        self.success_patterns.iter().any(|p| line.contains(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_serial_settings() {
        let settings = SerialSettings::default();
        assert_eq!(settings.baud_rate, 115200);
        assert_eq!(settings.data_bits, 8);
    }

    #[test]
    fn test_error_matching_is_case_insensitive() {
        let profile = DeviceProfile {
            name: "Test".to_string(),
            id: "test".to_string(),
            description: String::new(),
            manufacturer: String::new(),
            serial: SerialSettings::default(),
            boot_stages: vec![],
            error_patterns: vec![ErrorPattern {
                pattern: "Kernel Panic".to_string(),
                severity: "error".to_string(),
                description: "panic".to_string(),
                suggestion: None,
            }],
            success_patterns: vec![],
            usb_vendor_ids: vec![],
            usb_product_ids: vec![],
            boot_files: vec![],
            architecture: "unknown".to_string(),
        };

        assert!(profile.match_error("kernel panic - not syncing").is_some());
        assert!(profile.match_error("all good").is_none());
    }
}
