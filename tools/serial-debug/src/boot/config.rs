//! Boot configuration (config.txt) parser and analyzer
//!
//! Parses and validates Raspberry Pi config.txt settings.

use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Known config.txt parameters with their descriptions and valid values
const CONFIG_PARAMS: &[(&str, &str, Option<&[&str]>)] = &[
    // Boot options
    ("arm_64bit", "Enable 64-bit kernel mode", Some(&["0", "1"])),
    ("arm_boost", "Enable CPU boost mode on Pi 4", Some(&["0", "1"])),
    ("kernel", "Kernel image filename", None),
    ("kernel_address", "Memory address to load kernel", None),
    ("initramfs", "Initial RAM filesystem", None),
    ("device_tree", "Device tree blob to use", None),
    ("device_tree_address", "Memory address for device tree", None),
    ("enable_uart", "Enable UART serial console", Some(&["0", "1"])),
    ("uart_2ndstage", "Enable UART during 2nd stage boot", Some(&["0", "1"])),

    // Video/display
    ("hdmi_safe", "HDMI safe mode", Some(&["0", "1"])),
    ("hdmi_force_hotplug", "Force HDMI even if not detected", Some(&["0", "1"])),
    ("hdmi_group", "HDMI output group (CEA/DMT)", Some(&["0", "1", "2"])),
    ("hdmi_mode", "HDMI output mode number", None),
    ("disable_overscan", "Disable overscan compensation", Some(&["0", "1"])),
    ("framebuffer_width", "Framebuffer width in pixels", None),
    ("framebuffer_height", "Framebuffer height in pixels", None),

    // Memory
    ("gpu_mem", "GPU memory in MB", None),
    ("gpu_mem_256", "GPU memory for 256MB Pi", None),
    ("gpu_mem_512", "GPU memory for 512MB Pi", None),
    ("gpu_mem_1024", "GPU memory for 1GB+ Pi", None),
    ("total_mem", "Limit total memory", None),

    // Overclocking
    ("arm_freq", "ARM CPU frequency in MHz", None),
    ("over_voltage", "CPU/GPU core voltage offset", None),
    ("core_freq", "GPU core frequency in MHz", None),
    ("sdram_freq", "SDRAM frequency in MHz", None),

    // Pi 4 specific
    ("arm_freq_min", "Minimum ARM frequency", None),
    ("core_freq_min", "Minimum GPU core frequency", None),

    // GPIO
    ("gpio", "GPIO pin configuration", None),
    ("enable_jtag_gpio", "Enable JTAG debugging on GPIO", Some(&["0", "1"])),

    // USB
    ("usb_max_current_enable", "Enable more USB current", Some(&["0", "1"])),
    ("otg_mode", "OTG mode (host/device)", Some(&["0", "1"])),

    // Audio
    ("dtparam=audio", "Enable onboard audio", Some(&["on", "off"])),

    // I2C/SPI
    ("dtparam=i2c_arm", "Enable I2C on GPIO", Some(&["on", "off"])),
    ("dtparam=spi", "Enable SPI on GPIO", Some(&["on", "off"])),

    // Camera/Display
    ("start_x", "Enable camera/codec firmware", Some(&["0", "1"])),
    ("camera_auto_detect", "Auto-detect camera", Some(&["0", "1"])),
    ("display_auto_detect", "Auto-detect display", Some(&["0", "1"])),
];

/// Parsed config.txt entry
#[derive(Debug, Clone)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    pub line_number: usize,
    pub description: Option<String>,
    pub is_conditional: bool,
    pub condition: Option<String>,
}

/// Parsed boot configuration
#[derive(Debug)]
pub struct BootConfig {
    pub path: String,
    pub entries: Vec<ConfigEntry>,
    pub filters: Vec<String>,
    pub issues: Vec<ConfigIssue>,
    raw_content: String,
}

/// Configuration issue
#[derive(Debug, Clone)]
pub struct ConfigIssue {
    pub line: usize,
    pub severity: ConfigSeverity,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigSeverity {
    Error,
    Warning,
    Info,
}

impl BootConfig {
    /// Parse config.txt from a file path
    pub fn parse(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config.txt: {}", path.display()))?;

        Self::parse_content(&content, path.to_string_lossy().to_string())
    }

    /// Parse config.txt content
    pub fn parse_content(content: &str, path: String) -> Result<Self> {
        let mut config = Self {
            path,
            entries: Vec::new(),
            filters: Vec::new(),
            issues: Vec::new(),
            raw_content: content.to_string(),
        };

        let mut current_filter: Option<String> = None;

        for (line_num, line) in content.lines().enumerate() {
            let line_number = line_num + 1;
            let trimmed = line.trim();

            // Skip empty lines
            if trimmed.is_empty() {
                continue;
            }

            // Handle comments
            if trimmed.starts_with('#') {
                continue;
            }

            // Handle conditional sections [filter]
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let filter = &trimmed[1..trimmed.len() - 1];

                if filter == "all" {
                    current_filter = None;
                } else {
                    current_filter = Some(filter.to_string());
                    if !config.filters.contains(&filter.to_string()) {
                        config.filters.push(filter.to_string());
                    }
                }
                continue;
            }

            // Parse key=value
            if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();

                // Find description from known params
                let description = CONFIG_PARAMS
                    .iter()
                    .find(|(k, _, _)| *k == key)
                    .map(|(_, d, _)| d.to_string());

                config.entries.push(ConfigEntry {
                    key,
                    value,
                    line_number,
                    description,
                    is_conditional: current_filter.is_some(),
                    condition: current_filter.clone(),
                });
            } else {
                // Handle dtoverlay and dtparam without = sign
                if trimmed.starts_with("include ") {
                    let include_file = trimmed.strip_prefix("include ").unwrap().trim();
                    config.entries.push(ConfigEntry {
                        key: "include".to_string(),
                        value: include_file.to_string(),
                        line_number,
                        description: Some("Include another config file".to_string()),
                        is_conditional: current_filter.is_some(),
                        condition: current_filter.clone(),
                    });
                } else {
                    config.issues.push(ConfigIssue {
                        line: line_number,
                        severity: ConfigSeverity::Warning,
                        message: format!("Unrecognized line format: {}", trimmed),
                    });
                }
            }
        }

        // Validate configuration
        config.validate();

        Ok(config)
    }

    /// Validate the configuration
    fn validate(&mut self) {
        // Check for common Pi 4 boot settings
        let has_arm_64bit = self.get("arm_64bit").is_some();
        let has_enable_uart = self.get("enable_uart").is_some();
        let kernel = self.get("kernel");

        // Check for 64-bit mode
        if let Some(arm_64) = self.get("arm_64bit") {
            if arm_64.value == "1" && kernel.is_none() {
                // Suggest using kernel8.img for 64-bit
                self.issues.push(ConfigIssue {
                    line: arm_64.line_number,
                    severity: ConfigSeverity::Info,
                    message: "64-bit mode enabled. Ensure kernel8.img is present or specify kernel=".to_string(),
                });
            }
        }

        // Check UART for serial debugging
        if !has_enable_uart {
            self.issues.push(ConfigIssue {
                line: 0,
                severity: ConfigSeverity::Info,
                message: "enable_uart not set. Add 'enable_uart=1' for serial console debugging".to_string(),
            });
        }

        // Check for known incompatibilities
        if let Some(gpu_mem) = self.get("gpu_mem") {
            if let Ok(mem) = gpu_mem.value.parse::<u32>() {
                if mem < 16 {
                    self.issues.push(ConfigIssue {
                        line: gpu_mem.line_number,
                        severity: ConfigSeverity::Warning,
                        message: format!("gpu_mem={} is very low, may cause display issues", mem),
                    });
                }
            }
        }

        // Check for overclocking without over_voltage
        if self.get("arm_freq").is_some() && self.get("over_voltage").is_none() {
            self.issues.push(ConfigIssue {
                line: 0,
                severity: ConfigSeverity::Warning,
                message: "arm_freq set without over_voltage. High frequencies may require voltage adjustment".to_string(),
            });
        }
    }

    /// Get a configuration entry by key
    pub fn get(&self, key: &str) -> Option<&ConfigEntry> {
        self.entries.iter().find(|e| e.key == key)
    }

    /// Get all entries for a key (may have multiple with filters)
    pub fn get_all(&self, key: &str) -> Vec<&ConfigEntry> {
        self.entries.iter().filter(|e| e.key == key).collect()
    }

    /// Print formatted configuration report
    pub fn print_report(&self) {
        println!("{}", "=".repeat(70));
        println!("{}", "Boot Configuration Analysis (config.txt)".cyan().bold());
        println!("{}", "=".repeat(70));

        println!("\n{}: {}", "File".white().bold(), self.path);

        // Print filters if any
        if !self.filters.is_empty() {
            println!(
                "\n{}: {}",
                "Conditional sections".white().bold(),
                self.filters.join(", ")
            );
        }

        // Group entries by category
        println!("\n{}", "Configuration Entries:".white().bold());

        // Boot-related
        let boot_keys = ["arm_64bit", "kernel", "initramfs", "device_tree", "enable_uart", "uart_2ndstage"];
        self.print_category("Boot Settings", &boot_keys);

        // Memory
        let mem_keys = ["gpu_mem", "gpu_mem_256", "gpu_mem_512", "gpu_mem_1024", "total_mem"];
        self.print_category("Memory Settings", &mem_keys);

        // Display
        let display_keys = ["hdmi_safe", "hdmi_force_hotplug", "hdmi_group", "hdmi_mode",
                           "framebuffer_width", "framebuffer_height", "disable_overscan"];
        self.print_category("Display Settings", &display_keys);

        // Overclocking
        let oc_keys = ["arm_freq", "over_voltage", "core_freq", "sdram_freq"];
        self.print_category("Overclocking", &oc_keys);

        // Other entries
        let all_known: Vec<&str> = boot_keys.iter()
            .chain(mem_keys.iter())
            .chain(display_keys.iter())
            .chain(oc_keys.iter())
            .copied()
            .collect();

        let other_entries: Vec<_> = self.entries
            .iter()
            .filter(|e| !all_known.contains(&e.key.as_str()))
            .collect();

        if !other_entries.is_empty() {
            println!("\n  {}:", "Other Settings".cyan());
            for entry in other_entries {
                let condition = if let Some(ref c) = entry.condition {
                    format!(" [{}]", c).dimmed().to_string()
                } else {
                    String::new()
                };
                println!("    {} = {}{}", entry.key, entry.value.white(), condition);
            }
        }

        // Print issues
        if !self.issues.is_empty() {
            println!("\n{}", "Issues:".white().bold());
            for issue in &self.issues {
                let (marker, color) = match issue.severity {
                    ConfigSeverity::Error => ("[ERROR]", "red"),
                    ConfigSeverity::Warning => ("[WARNING]", "yellow"),
                    ConfigSeverity::Info => ("[INFO]", "cyan"),
                };

                let marker_colored = match color {
                    "red" => marker.red().bold(),
                    "yellow" => marker.yellow().bold(),
                    _ => marker.cyan().bold(),
                };

                let line_info = if issue.line > 0 {
                    format!(" (line {})", issue.line)
                } else {
                    String::new()
                };

                println!("  {}{} {}", marker_colored, line_info.dimmed(), issue.message);
            }
        }

        // Recommendations for debugging
        println!("\n{}", "Debugging Recommendations:".white().bold());

        if self.get("enable_uart").map(|e| e.value.as_str()) != Some("1") {
            println!("  {} Add 'enable_uart=1' for serial console output", "→".cyan());
        }

        if self.get("uart_2ndstage").map(|e| e.value.as_str()) != Some("1") {
            println!("  {} Add 'uart_2ndstage=1' for early boot serial output", "→".cyan());
        }

        println!("\n{}", "=".repeat(70));
    }

    /// Print a category of settings
    fn print_category(&self, name: &str, keys: &[&str]) {
        let entries: Vec<_> = self.entries
            .iter()
            .filter(|e| keys.contains(&e.key.as_str()))
            .collect();

        if !entries.is_empty() {
            println!("\n  {}:", name.cyan());
            for entry in entries {
                let desc = entry.description.as_deref().unwrap_or("");
                let condition = if let Some(ref c) = entry.condition {
                    format!(" [{}]", c).dimmed().to_string()
                } else {
                    String::new()
                };
                println!(
                    "    {} = {}{}  {}",
                    entry.key,
                    entry.value.white(),
                    condition,
                    format!("# {}", desc).dimmed()
                );
            }
        }
    }

    /// Generate a recommended config.txt for debugging
    pub fn generate_debug_config() -> String {
        r#"# Raspberry Pi 4 Debug Configuration
# Generated by rpi4-debug tool

# Enable 64-bit mode (for kernel8.img)
arm_64bit=1

# Enable UART serial console for debugging
enable_uart=1
uart_2ndstage=1

# Disable Bluetooth to free up UART
dtoverlay=disable-bt

# GPU memory allocation
gpu_mem=256

# HDMI settings (force output even without display)
hdmi_force_hotplug=1

# Camera/display auto-detect (disable if not needed)
camera_auto_detect=0
display_auto_detect=0

# For seL4/custom kernels, you may need:
# kernel=your_kernel.img
# device_tree=bcm2711-rpi-4-b.dtb

# JTAG debugging (optional)
# enable_jtag_gpio=1

[all]
"#.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_config() {
        let content = r#"
# Test config
arm_64bit=1
enable_uart=1
gpu_mem=256
"#;
        let config = BootConfig::parse_content(content.to_string(), "test".to_string()).unwrap();

        assert_eq!(config.entries.len(), 3);
        assert_eq!(config.get("arm_64bit").unwrap().value, "1");
        assert_eq!(config.get("enable_uart").unwrap().value, "1");
        assert_eq!(config.get("gpu_mem").unwrap().value, "256");
    }

    #[test]
    fn test_parse_conditional_sections() {
        let content = r#"
arm_64bit=1

[pi4]
arm_freq=1500

[all]
enable_uart=1
"#;
        let config = BootConfig::parse_content(content.to_string(), "test".to_string()).unwrap();

        assert!(config.filters.contains(&"pi4".to_string()));

        let arm_freq = config.get("arm_freq").unwrap();
        assert!(arm_freq.is_conditional);
        assert_eq!(arm_freq.condition, Some("pi4".to_string()));
    }
}
