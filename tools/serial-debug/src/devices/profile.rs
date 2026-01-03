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

/// Error pattern for detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPattern {
    /// Pattern to match (substring or regex)
    pub pattern: String,
    /// Whether this is a regex pattern
    pub is_regex: bool,
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

impl DeviceProfile {
    /// Create a new empty profile
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            name: name.to_string(),
            id: id.to_string(),
            description: String::new(),
            manufacturer: String::new(),
            serial: SerialSettings::default(),
            boot_stages: Vec::new(),
            error_patterns: Vec::new(),
            success_patterns: Vec::new(),
            usb_vendor_ids: Vec::new(),
            usb_product_ids: Vec::new(),
            boot_files: Vec::new(),
            architecture: "unknown".to_string(),
        }
    }

    /// Add a boot stage
    pub fn add_boot_stage(&mut self, name: &str, patterns: Vec<&str>, description: &str) {
        self.boot_stages.push(BootStage {
            name: name.to_string(),
            patterns: patterns.iter().map(|s| s.to_string()).collect(),
            description: description.to_string(),
            expected_duration_secs: 0,
        });
    }

    /// Add an error pattern
    pub fn add_error_pattern(&mut self, pattern: &str, severity: &str, description: &str, suggestion: Option<&str>) {
        self.error_patterns.push(ErrorPattern {
            pattern: pattern.to_string(),
            is_regex: false,
            severity: severity.to_string(),
            description: description.to_string(),
            suggestion: suggestion.map(|s| s.to_string()),
        });
    }

    /// Check if a line matches any boot stage
    pub fn match_boot_stage(&self, line: &str) -> Option<&BootStage> {
        self.boot_stages.iter().find(|stage| {
            stage.patterns.iter().any(|p| line.contains(p))
        })
    }

    /// Check if a line matches any error pattern
    pub fn match_error(&self, line: &str) -> Option<&ErrorPattern> {
        self.error_patterns.iter().find(|err| {
            if err.is_regex {
                regex::Regex::new(&err.pattern)
                    .map(|re| re.is_match(line))
                    .unwrap_or(false)
            } else {
                line.to_lowercase().contains(&err.pattern.to_lowercase())
            }
        })
    }

    /// Check if a line indicates success
    pub fn is_success(&self, line: &str) -> bool {
        self.success_patterns.iter().any(|p| line.contains(p))
    }
}

// Add regex dependency for pattern matching
mod regex {
    pub struct Regex {
        pattern: String,
    }

    impl Regex {
        pub fn new(pattern: &str) -> Result<Self, ()> {
            Ok(Self { pattern: pattern.to_string() })
        }

        pub fn is_match(&self, text: &str) -> bool {
            // Simple substring match for now
            // Full regex would require the regex crate
            text.contains(&self.pattern)
        }
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
    fn test_profile_creation() {
        let mut profile = DeviceProfile::new("test", "Test Device");
        profile.add_boot_stage("Boot", vec!["Starting"], "Initial boot");
        profile.add_error_pattern("error", "error", "An error occurred", Some("Check logs"));

        assert_eq!(profile.boot_stages.len(), 1);
        assert_eq!(profile.error_patterns.len(), 1);
    }
}
