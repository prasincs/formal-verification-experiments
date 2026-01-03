//! Boot validation utilities for Raspberry Pi 4
//!
//! Provides comprehensive validation of boot configuration and files.

use super::{BootConfig, BootPartition};
use crate::image::KernelImage;
use anyhow::Result;
use colored::Colorize;
use std::path::Path;

/// Boot validation result
#[derive(Debug)]
pub struct ValidationResult {
    pub passed: bool,
    pub checks: Vec<ValidationCheck>,
    pub critical_failures: Vec<String>,
    pub warnings: Vec<String>,
    pub info: Vec<String>,
}

/// Individual validation check
#[derive(Debug)]
pub struct ValidationCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub severity: CheckSeverity,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckSeverity {
    Critical,
    Warning,
    Info,
}

/// Boot validator for comprehensive boot checks
pub struct BootValidator {
    boot_path: String,
    checks: Vec<ValidationCheck>,
}

impl BootValidator {
    /// Create a new boot validator
    pub fn new(boot_path: &str) -> Self {
        Self {
            boot_path: boot_path.to_string(),
            checks: Vec::new(),
        }
    }

    /// Run all validation checks
    pub fn validate(&mut self) -> Result<ValidationResult> {
        // Clone to avoid borrow conflicts
        let boot_path_str = self.boot_path.clone();
        let boot_path = Path::new(&boot_path_str);

        // Check boot partition
        self.check_boot_partition(boot_path)?;

        // Check config.txt
        self.check_config_txt(boot_path)?;

        // Check cmdline.txt
        self.check_cmdline_txt(boot_path)?;

        // Check kernel image
        self.check_kernel_image(boot_path)?;

        // Check device tree
        self.check_device_tree(boot_path)?;

        // Check permissions
        self.check_permissions(boot_path)?;

        // Compile results
        let critical_failures: Vec<String> = self.checks
            .iter()
            .filter(|c| !c.passed && c.severity == CheckSeverity::Critical)
            .map(|c| c.message.clone())
            .collect();

        let warnings: Vec<String> = self.checks
            .iter()
            .filter(|c| !c.passed && c.severity == CheckSeverity::Warning)
            .map(|c| c.message.clone())
            .collect();

        let info: Vec<String> = self.checks
            .iter()
            .filter(|c| c.severity == CheckSeverity::Info)
            .map(|c| c.message.clone())
            .collect();

        Ok(ValidationResult {
            passed: critical_failures.is_empty(),
            checks: std::mem::take(&mut self.checks),
            critical_failures,
            warnings,
            info,
        })
    }

    /// Check boot partition structure
    fn check_boot_partition(&mut self, path: &Path) -> Result<()> {
        match BootPartition::analyze(path) {
            Ok(partition) => {
                // Check required files
                self.checks.push(ValidationCheck {
                    name: "Boot partition accessible".to_string(),
                    passed: true,
                    message: format!("Boot partition at {} is accessible", path.display()),
                    severity: CheckSeverity::Critical,
                });

                // Check for critical issues from partition analysis
                for issue in &partition.issues {
                    match issue.severity {
                        super::partition::IssueSeverity::Error => {
                            self.checks.push(ValidationCheck {
                                name: "Boot file check".to_string(),
                                passed: false,
                                message: issue.message.clone(),
                                severity: CheckSeverity::Critical,
                            });
                        }
                        super::partition::IssueSeverity::Warning => {
                            self.checks.push(ValidationCheck {
                                name: "Boot file check".to_string(),
                                passed: false,
                                message: issue.message.clone(),
                                severity: CheckSeverity::Warning,
                            });
                        }
                        super::partition::IssueSeverity::Info => {
                            self.checks.push(ValidationCheck {
                                name: "Boot file check".to_string(),
                                passed: true,
                                message: issue.message.clone(),
                                severity: CheckSeverity::Info,
                            });
                        }
                    }
                }
            }
            Err(e) => {
                self.checks.push(ValidationCheck {
                    name: "Boot partition accessible".to_string(),
                    passed: false,
                    message: format!("Cannot access boot partition: {}", e),
                    severity: CheckSeverity::Critical,
                });
            }
        }

        Ok(())
    }

    /// Check config.txt
    fn check_config_txt(&mut self, path: &Path) -> Result<()> {
        let config_path = path.join("config.txt");

        if !config_path.exists() {
            self.checks.push(ValidationCheck {
                name: "config.txt exists".to_string(),
                passed: false,
                message: "config.txt is missing".to_string(),
                severity: CheckSeverity::Critical,
            });
            return Ok(());
        }

        match BootConfig::parse(&config_path) {
            Ok(config) => {
                self.checks.push(ValidationCheck {
                    name: "config.txt parseable".to_string(),
                    passed: true,
                    message: "config.txt parsed successfully".to_string(),
                    severity: CheckSeverity::Critical,
                });

                // Check for UART
                let uart_enabled = config.get("enable_uart")
                    .map(|e| e.value == "1")
                    .unwrap_or(false);

                self.checks.push(ValidationCheck {
                    name: "UART enabled".to_string(),
                    passed: uart_enabled,
                    message: if uart_enabled {
                        "Serial console enabled (enable_uart=1)".to_string()
                    } else {
                        "Serial console disabled. Add 'enable_uart=1' for debugging".to_string()
                    },
                    severity: CheckSeverity::Warning,
                });

                // Check for 64-bit mode if kernel8.img exists
                let kernel8_exists = path.join("kernel8.img").exists();
                let arm_64bit = config.get("arm_64bit")
                    .map(|e| e.value == "1")
                    .unwrap_or(false);

                if kernel8_exists && !arm_64bit {
                    self.checks.push(ValidationCheck {
                        name: "64-bit mode".to_string(),
                        passed: false,
                        message: "kernel8.img found but arm_64bit=1 not set".to_string(),
                        severity: CheckSeverity::Warning,
                    });
                }

                // Add config.txt issues
                for issue in &config.issues {
                    let severity = match issue.severity {
                        super::config::ConfigSeverity::Error => CheckSeverity::Critical,
                        super::config::ConfigSeverity::Warning => CheckSeverity::Warning,
                        super::config::ConfigSeverity::Info => CheckSeverity::Info,
                    };

                    self.checks.push(ValidationCheck {
                        name: "Config validation".to_string(),
                        passed: issue.severity != super::config::ConfigSeverity::Error,
                        message: issue.message.clone(),
                        severity,
                    });
                }
            }
            Err(e) => {
                self.checks.push(ValidationCheck {
                    name: "config.txt parseable".to_string(),
                    passed: false,
                    message: format!("Failed to parse config.txt: {}", e),
                    severity: CheckSeverity::Critical,
                });
            }
        }

        Ok(())
    }

    /// Check cmdline.txt
    fn check_cmdline_txt(&mut self, path: &Path) -> Result<()> {
        let cmdline_path = path.join("cmdline.txt");

        if !cmdline_path.exists() {
            self.checks.push(ValidationCheck {
                name: "cmdline.txt exists".to_string(),
                passed: false,
                message: "cmdline.txt is missing".to_string(),
                severity: CheckSeverity::Warning,
            });
            return Ok(());
        }

        match std::fs::read_to_string(&cmdline_path) {
            Ok(content) => {
                let content = content.trim();

                // Check for basic required parameters
                if content.is_empty() {
                    self.checks.push(ValidationCheck {
                        name: "cmdline.txt content".to_string(),
                        passed: false,
                        message: "cmdline.txt is empty".to_string(),
                        severity: CheckSeverity::Warning,
                    });
                } else {
                    self.checks.push(ValidationCheck {
                        name: "cmdline.txt content".to_string(),
                        passed: true,
                        message: format!("cmdline.txt: {}", if content.len() > 50 { &content[..50] } else { content }),
                        severity: CheckSeverity::Info,
                    });

                    // Check for console parameter
                    if !content.contains("console=") {
                        self.checks.push(ValidationCheck {
                            name: "Console parameter".to_string(),
                            passed: false,
                            message: "No console= parameter in cmdline.txt. Add 'console=serial0,115200' for serial output".to_string(),
                            severity: CheckSeverity::Warning,
                        });
                    }

                    // Check for root parameter
                    if !content.contains("root=") {
                        self.checks.push(ValidationCheck {
                            name: "Root parameter".to_string(),
                            passed: true,
                            message: "No root= parameter (OK for bare-metal/seL4)".to_string(),
                            severity: CheckSeverity::Info,
                        });
                    }
                }
            }
            Err(e) => {
                self.checks.push(ValidationCheck {
                    name: "cmdline.txt readable".to_string(),
                    passed: false,
                    message: format!("Failed to read cmdline.txt: {}", e),
                    severity: CheckSeverity::Warning,
                });
            }
        }

        Ok(())
    }

    /// Check kernel image
    fn check_kernel_image(&mut self, path: &Path) -> Result<()> {
        // Look for kernel images
        let kernel_files = ["kernel8.img", "kernel7l.img", "kernel7.img", "kernel.img"];

        let mut found_kernel = false;
        for kernel in &kernel_files {
            let kernel_path = path.join(kernel);
            if kernel_path.exists() {
                found_kernel = true;

                // Try to analyze the kernel image
                match KernelImage::analyze(&kernel_path) {
                    Ok(info) => {
                        self.checks.push(ValidationCheck {
                            name: format!("Kernel image {}", kernel),
                            passed: true,
                            message: format!("{}: {} format, {} bytes",
                                kernel, info.format, info.size),
                            severity: CheckSeverity::Info,
                        });

                        // Check for common issues
                        if info.size < 1000 {
                            self.checks.push(ValidationCheck {
                                name: "Kernel size".to_string(),
                                passed: false,
                                message: format!("{} is suspiciously small ({} bytes)", kernel, info.size),
                                severity: CheckSeverity::Warning,
                            });
                        }
                    }
                    Err(e) => {
                        self.checks.push(ValidationCheck {
                            name: format!("Kernel image {}", kernel),
                            passed: false,
                            message: format!("Failed to analyze {}: {}", kernel, e),
                            severity: CheckSeverity::Warning,
                        });
                    }
                }
            }
        }

        if !found_kernel {
            self.checks.push(ValidationCheck {
                name: "Kernel image present".to_string(),
                passed: false,
                message: "No kernel image found".to_string(),
                severity: CheckSeverity::Critical,
            });
        }

        Ok(())
    }

    /// Check device tree
    fn check_device_tree(&mut self, path: &Path) -> Result<()> {
        let pi4_dtb = path.join("bcm2711-rpi-4-b.dtb");

        if pi4_dtb.exists() {
            self.checks.push(ValidationCheck {
                name: "Pi 4 device tree".to_string(),
                passed: true,
                message: "bcm2711-rpi-4-b.dtb found".to_string(),
                severity: CheckSeverity::Info,
            });
        } else {
            // Check for any bcm2711 dtb
            let has_any_pi4_dtb = std::fs::read_dir(path)
                .map(|entries| {
                    entries.filter_map(|e| e.ok()).any(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .starts_with("bcm2711")
                    })
                })
                .unwrap_or(false);

            if has_any_pi4_dtb {
                self.checks.push(ValidationCheck {
                    name: "Pi 4 device tree".to_string(),
                    passed: true,
                    message: "Pi 4 device tree variant found".to_string(),
                    severity: CheckSeverity::Info,
                });
            } else {
                self.checks.push(ValidationCheck {
                    name: "Pi 4 device tree".to_string(),
                    passed: false,
                    message: "No Pi 4 device tree (bcm2711*.dtb) found".to_string(),
                    severity: CheckSeverity::Warning,
                });
            }
        }

        Ok(())
    }

    /// Check file permissions
    fn check_permissions(&mut self, path: &Path) -> Result<()> {
        // Check if we can read files in the boot partition
        let config_path = path.join("config.txt");
        if config_path.exists() {
            match std::fs::File::open(&config_path) {
                Ok(_) => {
                    self.checks.push(ValidationCheck {
                        name: "File permissions".to_string(),
                        passed: true,
                        message: "Boot files are readable".to_string(),
                        severity: CheckSeverity::Info,
                    });
                }
                Err(e) => {
                    self.checks.push(ValidationCheck {
                        name: "File permissions".to_string(),
                        passed: false,
                        message: format!("Cannot read boot files: {}", e),
                        severity: CheckSeverity::Warning,
                    });
                }
            }
        }

        Ok(())
    }

    /// Print validation report
    pub fn print_report(result: &ValidationResult) {
        println!("{}", "=".repeat(70));
        println!("{}", "Boot Validation Report".cyan().bold());
        println!("{}", "=".repeat(70));

        // Overall status
        let status = if result.passed {
            "PASS".green().bold()
        } else {
            "FAIL".red().bold()
        };
        println!("\n{}: {}", "Overall Status".white().bold(), status);

        // Detailed checks
        println!("\n{}", "Validation Checks:".white().bold());
        for check in &result.checks {
            let (marker, color) = if check.passed {
                ("[PASS]", "green")
            } else {
                match check.severity {
                    CheckSeverity::Critical => ("[FAIL]", "red"),
                    CheckSeverity::Warning => ("[WARN]", "yellow"),
                    CheckSeverity::Info => ("[INFO]", "cyan"),
                }
            };

            let marker_colored = match color {
                "green" => marker.green(),
                "red" => marker.red().bold(),
                "yellow" => marker.yellow(),
                _ => marker.cyan(),
            };

            println!("  {} {}: {}", marker_colored, check.name, check.message);
        }

        // Summary
        if !result.critical_failures.is_empty() {
            println!("\n{}", "Critical Failures:".red().bold());
            for failure in &result.critical_failures {
                println!("  {} {}", "✗".red(), failure);
            }
        }

        if !result.warnings.is_empty() {
            println!("\n{}", "Warnings:".yellow().bold());
            for warning in &result.warnings {
                println!("  {} {}", "⚠".yellow(), warning);
            }
        }

        // Recommendations
        if !result.passed {
            println!("\n{}", "Recommendations:".white().bold());
            println!("  1. Ensure all required boot files are present");
            println!("  2. Check config.txt for correct settings");
            println!("  3. Verify kernel image is compatible with Pi 4");
            println!("  4. Use 'rpi4-debug serial monitor' to capture boot output");
        }

        println!("\n{}", "=".repeat(70));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_validator_missing_partition() {
        let mut validator = BootValidator::new("/nonexistent/path");
        let result = validator.validate().unwrap();
        assert!(!result.passed);
        assert!(!result.critical_failures.is_empty());
    }

    #[test]
    fn test_validator_empty_partition() {
        let dir = tempdir().unwrap();
        let mut validator = BootValidator::new(dir.path().to_str().unwrap());
        let result = validator.validate().unwrap();
        assert!(!result.passed);
    }
}
