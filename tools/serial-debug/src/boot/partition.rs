//! Boot partition analysis for Raspberry Pi 4
//!
//! Analyzes the boot partition structure and validates required files.

use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Required boot files for Raspberry Pi 4
const REQUIRED_BOOT_FILES: &[(&str, &str)] = &[
    ("bootcode.bin", "First-stage bootloader (Pi 3 and earlier)"),
    ("start4.elf", "GPU firmware for Pi 4"),
    ("fixup4.dat", "GPU memory split configuration for Pi 4"),
    ("config.txt", "Boot configuration file"),
    ("cmdline.txt", "Kernel command line parameters"),
];

/// Optional but recommended boot files
const OPTIONAL_BOOT_FILES: &[(&str, &str)] = &[
    ("start.elf", "GPU firmware (legacy/Pi 3)"),
    ("fixup.dat", "GPU memory split (legacy/Pi 3)"),
    ("start4x.elf", "GPU firmware with codec support"),
    ("fixup4x.dat", "Memory split for codec support"),
    ("start4cd.elf", "Cut-down GPU firmware (reduces memory)"),
    ("fixup4cd.dat", "Cut-down memory split"),
    ("start4db.elf", "Debug GPU firmware"),
    ("fixup4db.dat", "Debug memory split"),
];

/// Kernel image patterns
const KERNEL_PATTERNS: &[&str] = &[
    "kernel*.img",
    "kernel.img",
    "kernel7.img",
    "kernel7l.img",
    "kernel8.img",
    "Image",
    "Image.gz",
    "zImage",
    "vmlinuz*",
];

/// Device tree patterns for Pi 4
const DTB_PATTERNS: &[&str] = &[
    "bcm2711-rpi-4-b.dtb",
    "bcm2711-rpi-400.dtb",
    "bcm2711-rpi-cm4.dtb",
    "bcm2711-rpi-cm4s.dtb",
    "*.dtb",
];

/// Overlay patterns
const OVERLAY_DIR: &str = "overlays";

/// File information with size and checksum
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: PathBuf,
    pub size: u64,
    pub exists: bool,
    pub description: String,
}

/// Boot partition analysis result
#[derive(Debug)]
pub struct BootPartition {
    pub path: PathBuf,
    pub total_size: u64,
    pub free_space: u64,
    pub files: HashMap<String, FileInfo>,
    pub kernel_images: Vec<PathBuf>,
    pub device_trees: Vec<PathBuf>,
    pub overlays: Vec<PathBuf>,
    pub issues: Vec<BootIssue>,
}

/// Boot issue severity
#[derive(Debug, Clone, PartialEq)]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

/// Detected boot issue
#[derive(Debug, Clone)]
pub struct BootIssue {
    pub severity: IssueSeverity,
    pub message: String,
    pub suggestion: String,
}

impl BootPartition {
    /// Analyze a boot partition at the given path
    pub fn analyze(path: &Path) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!("Boot partition path does not exist: {}", path.display());
        }

        if !path.is_dir() {
            anyhow::bail!("Boot partition path is not a directory: {}", path.display());
        }

        let mut partition = Self {
            path: path.to_path_buf(),
            total_size: 0,
            free_space: 0,
            files: HashMap::new(),
            kernel_images: Vec::new(),
            device_trees: Vec::new(),
            overlays: Vec::new(),
            issues: Vec::new(),
        };

        // Get partition space info
        partition.get_space_info()?;

        // Scan required files
        partition.scan_required_files()?;

        // Scan optional files
        partition.scan_optional_files()?;

        // Find kernel images
        partition.find_kernel_images()?;

        // Find device trees
        partition.find_device_trees()?;

        // Find overlays
        partition.find_overlays()?;

        // Validate and find issues
        partition.validate()?;

        Ok(partition)
    }

    /// Get disk space information
    fn get_space_info(&mut self) -> Result<()> {
        // Calculate total size from files
        for entry in WalkDir::new(&self.path).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                if let Ok(meta) = entry.metadata() {
                    self.total_size += meta.len();
                }
            }
        }

        // Try to get free space using statvfs on Unix
        #[cfg(unix)]
        {
            use std::ffi::CString;
            use std::mem::MaybeUninit;

            let path_cstr = CString::new(self.path.to_string_lossy().as_bytes())
                .with_context(|| "Invalid path")?;

            unsafe {
                let mut stat: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();
                if libc::statvfs(path_cstr.as_ptr(), stat.as_mut_ptr()) == 0 {
                    let stat = stat.assume_init();
                    self.free_space = stat.f_bfree as u64 * stat.f_bsize as u64;
                }
            }
        }

        Ok(())
    }

    /// Scan for required boot files
    fn scan_required_files(&mut self) -> Result<()> {
        for (filename, description) in REQUIRED_BOOT_FILES {
            let file_path = self.path.join(filename);
            let exists = file_path.exists();
            let size = if exists {
                fs::metadata(&file_path)
                    .map(|m| m.len())
                    .unwrap_or(0)
            } else {
                0
            };

            self.files.insert(
                filename.to_string(),
                FileInfo {
                    path: file_path,
                    size,
                    exists,
                    description: description.to_string(),
                },
            );
        }

        Ok(())
    }

    /// Scan for optional boot files
    fn scan_optional_files(&mut self) -> Result<()> {
        for (filename, description) in OPTIONAL_BOOT_FILES {
            let file_path = self.path.join(filename);
            let exists = file_path.exists();
            let size = if exists {
                fs::metadata(&file_path)
                    .map(|m| m.len())
                    .unwrap_or(0)
            } else {
                0
            };

            self.files.insert(
                filename.to_string(),
                FileInfo {
                    path: file_path,
                    size,
                    exists,
                    description: description.to_string(),
                },
            );
        }

        Ok(())
    }

    /// Find kernel images in the boot partition
    fn find_kernel_images(&mut self) -> Result<()> {
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let filename = path.file_name().unwrap_or_default().to_string_lossy();

            // Check against kernel patterns
            for pattern in KERNEL_PATTERNS {
                if Self::matches_pattern(&filename, pattern) {
                    self.kernel_images.push(path.clone());
                    break;
                }
            }
        }

        Ok(())
    }

    /// Find device tree files
    fn find_device_trees(&mut self) -> Result<()> {
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if let Some(ext) = path.extension() {
                if ext == "dtb" {
                    self.device_trees.push(path);
                }
            }
        }

        Ok(())
    }

    /// Find overlay files
    fn find_overlays(&mut self) -> Result<()> {
        let overlay_path = self.path.join(OVERLAY_DIR);

        if !overlay_path.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&overlay_path)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if let Some(ext) = path.extension() {
                if ext == "dtbo" {
                    self.overlays.push(path);
                }
            }
        }

        Ok(())
    }

    /// Simple glob-like pattern matching
    fn matches_pattern(filename: &str, pattern: &str) -> bool {
        if pattern.contains('*') {
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                let (prefix, suffix) = (parts[0], parts[1]);
                return filename.starts_with(prefix) && filename.ends_with(suffix);
            }
        }
        filename == pattern
    }

    /// Validate the boot partition and collect issues
    fn validate(&mut self) -> Result<()> {
        // Check for missing required files
        for (filename, _) in REQUIRED_BOOT_FILES {
            if let Some(info) = self.files.get(*filename) {
                if !info.exists {
                    // Skip bootcode.bin for Pi 4 (it's in EEPROM)
                    if *filename == "bootcode.bin" {
                        self.issues.push(BootIssue {
                            severity: IssueSeverity::Info,
                            message: format!("{} not found (OK for Pi 4 with EEPROM boot)", filename),
                            suggestion: "Pi 4 boots from EEPROM, bootcode.bin is only needed for recovery".to_string(),
                        });
                    } else {
                        self.issues.push(BootIssue {
                            severity: IssueSeverity::Error,
                            message: format!("Required file missing: {}", filename),
                            suggestion: format!("Copy {} from Raspberry Pi firmware repository", filename),
                        });
                    }
                }
            }
        }

        // Check for kernel image
        if self.kernel_images.is_empty() {
            self.issues.push(BootIssue {
                severity: IssueSeverity::Error,
                message: "No kernel image found".to_string(),
                suggestion: "Copy kernel8.img (64-bit) or kernel7l.img (32-bit) to boot partition".to_string(),
            });
        }

        // Check for Pi 4 device tree
        let has_pi4_dtb = self.device_trees.iter().any(|p| {
            p.file_name()
                .map(|f| f.to_string_lossy().contains("bcm2711"))
                .unwrap_or(false)
        });

        if !has_pi4_dtb {
            self.issues.push(BootIssue {
                severity: IssueSeverity::Warning,
                message: "No Raspberry Pi 4 device tree (bcm2711*.dtb) found".to_string(),
                suggestion: "Copy bcm2711-rpi-4-b.dtb from Raspberry Pi firmware repository".to_string(),
            });
        }

        // Check for start4.elf specifically for Pi 4
        if let Some(info) = self.files.get("start4.elf") {
            if !info.exists {
                self.issues.push(BootIssue {
                    severity: IssueSeverity::Error,
                    message: "start4.elf missing (required for Pi 4)".to_string(),
                    suggestion: "Download start4.elf from https://github.com/raspberrypi/firmware".to_string(),
                });
            }
        }

        // Check for overlays directory
        let overlay_path = self.path.join(OVERLAY_DIR);
        if !overlay_path.exists() {
            self.issues.push(BootIssue {
                severity: IssueSeverity::Warning,
                message: "Overlays directory not found".to_string(),
                suggestion: "Create 'overlays' directory and copy dtbo files for device tree overlays".to_string(),
            });
        }

        Ok(())
    }

    /// Print a formatted report of the boot partition analysis
    pub fn print_report(&self) {
        println!("{}", "=".repeat(70));
        println!("{}", "Raspberry Pi 4 Boot Partition Analysis".cyan().bold());
        println!("{}", "=".repeat(70));

        // Partition info
        println!("\n{}", "Partition Information:".white().bold());
        println!("  Path: {}", self.path.display());
        println!("  Total used: {}", format_size(self.total_size));
        if self.free_space > 0 {
            println!("  Free space: {}", format_size(self.free_space));
        }

        // Required files
        println!("\n{}", "Required Boot Files:".white().bold());
        for (filename, _) in REQUIRED_BOOT_FILES {
            if let Some(info) = self.files.get(*filename) {
                let status = if info.exists {
                    format!("{} ({})", "[OK]".green(), format_size(info.size))
                } else {
                    "[MISSING]".red().to_string()
                };
                println!("  {} {}: {}", status, filename, info.description.dimmed());
            }
        }

        // Kernel images
        println!("\n{}", "Kernel Images:".white().bold());
        if self.kernel_images.is_empty() {
            println!("  {} No kernel images found", "[ERROR]".red());
        } else {
            for kernel in &self.kernel_images {
                let size = fs::metadata(kernel).map(|m| m.len()).unwrap_or(0);
                println!(
                    "  {} {} ({})",
                    "[OK]".green(),
                    kernel.file_name().unwrap_or_default().to_string_lossy(),
                    format_size(size)
                );
            }
        }

        // Device trees
        println!("\n{}", "Device Trees:".white().bold());
        if self.device_trees.is_empty() {
            println!("  {} No device trees found", "[WARNING]".yellow());
        } else {
            for dtb in &self.device_trees {
                let filename = dtb.file_name().unwrap_or_default().to_string_lossy();
                let is_pi4 = filename.contains("bcm2711");
                let marker = if is_pi4 { "(Pi 4)".cyan() } else { "".normal() };
                println!("  {} {} {}", "[OK]".green(), filename, marker);
            }
        }

        // Overlays
        println!("\n{}", "Device Tree Overlays:".white().bold());
        if self.overlays.is_empty() {
            println!("  No overlays found");
        } else {
            println!("  {} overlay(s) found in overlays/", self.overlays.len());
        }

        // Issues
        if !self.issues.is_empty() {
            println!("\n{}", "Issues Detected:".white().bold());
            for issue in &self.issues {
                let (marker, color) = match issue.severity {
                    IssueSeverity::Error => ("[ERROR]", "red"),
                    IssueSeverity::Warning => ("[WARNING]", "yellow"),
                    IssueSeverity::Info => ("[INFO]", "cyan"),
                };

                let marker_colored = match color {
                    "red" => marker.red().bold(),
                    "yellow" => marker.yellow().bold(),
                    _ => marker.cyan().bold(),
                };

                println!("  {} {}", marker_colored, issue.message);
                println!("    {} {}", "→".dimmed(), issue.suggestion.dimmed());
            }
        } else {
            println!("\n{}", "✓ No issues detected".green().bold());
        }

        println!("\n{}", "=".repeat(70));
    }

    /// Check if boot partition is likely bootable
    pub fn is_bootable(&self) -> bool {
        // Must have no error-level issues
        !self.issues.iter().any(|i| i.severity == IssueSeverity::Error)
    }
}

/// Format file size in human-readable format
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_pattern_matching() {
        assert!(BootPartition::matches_pattern("kernel8.img", "kernel*.img"));
        assert!(BootPartition::matches_pattern("kernel.img", "kernel*.img"));
        assert!(!BootPartition::matches_pattern("other.img", "kernel*.img"));
        assert!(BootPartition::matches_pattern("kernel.img", "kernel.img"));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 bytes");
        assert_eq!(format_size(512), "512 bytes");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(1048576), "1.00 MB");
    }
}
