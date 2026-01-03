//! Kernel image analysis for Raspberry Pi 4
//!
//! Analyzes kernel image format, architecture, and potential issues.

use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use colored::Colorize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// ARM64 Linux kernel image magic
const ARM64_MAGIC: u32 = 0x644d5241; // "ARM\x64" little-endian

/// ARM Linux kernel magic (zImage)
const ARM_ZIMAGE_MAGIC: u32 = 0x016F2818;

/// Gzip magic
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// seL4 ELF magic
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// Kernel image format
#[derive(Debug, Clone, PartialEq)]
pub enum KernelFormat {
    Arm64Image,     // ARM64 Linux Image format
    ArmZImage,      // ARM32 compressed zImage
    GzipCompressed, // gzip compressed image
    Elf,            // ELF binary (seL4, bare metal)
    RawBinary,      // Raw binary
    Unknown,
}

impl std::fmt::Display for KernelFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KernelFormat::Arm64Image => write!(f, "ARM64 Image"),
            KernelFormat::ArmZImage => write!(f, "ARM32 zImage"),
            KernelFormat::GzipCompressed => write!(f, "Gzip compressed"),
            KernelFormat::Elf => write!(f, "ELF"),
            KernelFormat::RawBinary => write!(f, "Raw binary"),
            KernelFormat::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Architecture detected from kernel
#[derive(Debug, Clone, PartialEq)]
pub enum Architecture {
    Arm64,
    Arm32,
    Unknown,
}

impl std::fmt::Display for Architecture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Architecture::Arm64 => write!(f, "AArch64 (64-bit)"),
            Architecture::Arm32 => write!(f, "ARM (32-bit)"),
            Architecture::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Kernel image information
#[derive(Debug)]
pub struct KernelImage {
    pub path: String,
    pub size: u64,
    pub format: KernelFormat,
    pub architecture: Architecture,
    pub load_address: Option<u64>,
    pub entry_point: Option<u64>,
    pub compressed: bool,
    pub issues: Vec<KernelIssue>,
}

/// Kernel image issue
#[derive(Debug, Clone)]
pub struct KernelIssue {
    pub severity: IssueSeverity,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

impl KernelImage {
    /// Analyze a kernel image file
    pub fn analyze(path: &Path) -> Result<Self> {
        let mut file = File::open(path)
            .with_context(|| format!("Failed to open kernel image: {}", path.display()))?;

        let metadata = file.metadata()?;
        let size = metadata.len();

        // Read header bytes
        let mut header = [0u8; 64];
        file.read_exact(&mut header)?;

        // Detect format
        let format = Self::detect_format(&header);

        // Detect architecture
        let architecture = Self::detect_architecture(&header, &format);

        // Get load address and entry point for known formats
        let (load_address, entry_point) = Self::parse_addresses(&mut file, &header, &format)?;

        // Check for compression
        let compressed = matches!(format, KernelFormat::GzipCompressed | KernelFormat::ArmZImage);

        let mut image = Self {
            path: path.to_string_lossy().to_string(),
            size,
            format,
            architecture,
            load_address,
            entry_point,
            compressed,
            issues: Vec::new(),
        };

        // Validate and collect issues
        image.validate()?;

        Ok(image)
    }

    /// Detect kernel format from header bytes
    fn detect_format(header: &[u8]) -> KernelFormat {
        // Check ELF
        if header.starts_with(&ELF_MAGIC) {
            return KernelFormat::Elf;
        }

        // Check gzip
        if header.starts_with(&GZIP_MAGIC) {
            return KernelFormat::GzipCompressed;
        }

        // Check ARM64 Image
        // ARM64 Image header: magic at offset 56
        if header.len() >= 60 {
            let magic = u32::from_le_bytes([header[56], header[57], header[58], header[59]]);
            if magic == ARM64_MAGIC {
                return KernelFormat::Arm64Image;
            }
        }

        // Check ARM zImage
        // zImage magic at offset 36
        if header.len() >= 40 {
            let magic = u32::from_le_bytes([header[36], header[37], header[38], header[39]]);
            if magic == ARM_ZIMAGE_MAGIC {
                return KernelFormat::ArmZImage;
            }
        }

        // Check for common boot code patterns
        // ARM branch instruction at start often indicates raw binary
        if header.len() >= 4 {
            let first_word = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            // ARM branch instruction pattern
            if (first_word & 0xFF000000) == 0xEA000000 || // ARM
               (first_word & 0xFC000000) == 0x14000000    // ARM64
            {
                return KernelFormat::RawBinary;
            }
        }

        KernelFormat::Unknown
    }

    /// Detect architecture from header
    fn detect_architecture(header: &[u8], format: &KernelFormat) -> Architecture {
        match format {
            KernelFormat::Arm64Image => Architecture::Arm64,
            KernelFormat::ArmZImage => Architecture::Arm32,
            KernelFormat::Elf => {
                // ELF architecture is at offset 18
                if header.len() >= 20 {
                    let machine = u16::from_le_bytes([header[18], header[19]]);
                    match machine {
                        183 => Architecture::Arm64, // EM_AARCH64
                        40 => Architecture::Arm32,  // EM_ARM
                        _ => Architecture::Unknown,
                    }
                } else {
                    Architecture::Unknown
                }
            }
            _ => Architecture::Unknown,
        }
    }

    /// Parse load address and entry point
    fn parse_addresses(
        file: &mut File,
        header: &[u8],
        format: &KernelFormat,
    ) -> Result<(Option<u64>, Option<u64>)> {
        match format {
            KernelFormat::Arm64Image => {
                // ARM64 Image header:
                // offset 8: text_offset (u64)
                // offset 16: image_size (u64)
                if header.len() >= 24 {
                    let text_offset = u64::from_le_bytes([
                        header[8], header[9], header[10], header[11],
                        header[12], header[13], header[14], header[15],
                    ]);
                    // Default Pi 4 kernel load address
                    let load_addr = 0x80000 + text_offset;
                    Ok((Some(load_addr), Some(load_addr)))
                } else {
                    Ok((None, None))
                }
            }
            KernelFormat::ArmZImage => {
                // zImage self-decompresses
                Ok((Some(0x8000), Some(0x8000)))
            }
            KernelFormat::Elf => {
                // Parse ELF header for entry point
                file.seek(SeekFrom::Start(0))?;
                let mut elf_header = [0u8; 64];
                file.read_exact(&mut elf_header)?;

                // Check ELF class (32 or 64 bit)
                let is_64bit = elf_header[4] == 2;

                if is_64bit {
                    // 64-bit ELF: entry point at offset 24
                    let entry = u64::from_le_bytes([
                        elf_header[24], elf_header[25], elf_header[26], elf_header[27],
                        elf_header[28], elf_header[29], elf_header[30], elf_header[31],
                    ]);
                    Ok((Some(entry), Some(entry)))
                } else {
                    // 32-bit ELF: entry point at offset 24
                    let entry = u32::from_le_bytes([
                        elf_header[24], elf_header[25], elf_header[26], elf_header[27],
                    ]) as u64;
                    Ok((Some(entry), Some(entry)))
                }
            }
            _ => Ok((None, None)),
        }
    }

    /// Validate kernel image
    fn validate(&mut self) -> Result<()> {
        // Check format
        if self.format == KernelFormat::Unknown {
            self.issues.push(KernelIssue {
                severity: IssueSeverity::Warning,
                message: "Unknown kernel format - may not boot correctly".to_string(),
            });
        }

        // Check size
        if self.size < 10_000 {
            self.issues.push(KernelIssue {
                severity: IssueSeverity::Warning,
                message: format!("Kernel image is very small ({} bytes)", self.size),
            });
        }

        // Check architecture for Pi 4
        if self.architecture == Architecture::Arm32 {
            self.issues.push(KernelIssue {
                severity: IssueSeverity::Info,
                message: "32-bit kernel detected. Pi 4 supports 64-bit (kernel8.img)".to_string(),
            });
        }

        // Check for common seL4/Microkit patterns
        if self.format == KernelFormat::Elf {
            self.issues.push(KernelIssue {
                severity: IssueSeverity::Info,
                message: "ELF format detected - ensure U-Boot or bootloader can load ELF".to_string(),
            });
        }

        Ok(())
    }

    /// Print analysis report
    pub fn print_report(&self) {
        println!("{}", "=".repeat(70));
        println!("{}", "Kernel Image Analysis".cyan().bold());
        println!("{}", "=".repeat(70));

        println!("\n{}: {}", "File".white().bold(), self.path);
        println!("{}: {} bytes ({:.2} MB)",
            "Size".white().bold(),
            self.size,
            self.size as f64 / 1_048_576.0
        );

        println!("\n{}", "Image Properties:".white().bold());
        println!("  Format: {}", self.format.to_string().cyan());
        println!("  Architecture: {}", self.architecture.to_string().cyan());
        println!("  Compressed: {}", if self.compressed { "Yes" } else { "No" });

        if let Some(addr) = self.load_address {
            println!("  Load Address: {}", format!("0x{:016x}", addr).cyan());
        }
        if let Some(addr) = self.entry_point {
            println!("  Entry Point: {}", format!("0x{:016x}", addr).cyan());
        }

        // Issues
        if !self.issues.is_empty() {
            println!("\n{}", "Analysis Notes:".white().bold());
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
            }
        }

        // Pi 4 specific recommendations
        println!("\n{}", "Raspberry Pi 4 Notes:".white().bold());
        match self.architecture {
            Architecture::Arm64 => {
                println!("  {} 64-bit kernel compatible with Pi 4", "✓".green());
                println!("  {} Ensure 'arm_64bit=1' in config.txt", "→".cyan());
            }
            Architecture::Arm32 => {
                println!("  {} 32-bit kernel will work but is not optimal", "⚠".yellow());
                println!("  {} Consider using 64-bit kernel for better performance", "→".cyan());
            }
            Architecture::Unknown => {
                println!("  {} Could not determine architecture", "⚠".yellow());
            }
        }

        if self.format == KernelFormat::Elf {
            println!("\n{}", "seL4/Microkit Notes:".white().bold());
            println!("  {} ELF format requires a bootloader (U-Boot) to load", "→".cyan());
            println!("  {} Or convert to raw binary with objcopy", "→".cyan());
            println!("  {} Command: aarch64-linux-gnu-objcopy -O binary input.elf output.img", "→".cyan());
        }

        println!("\n{}", "=".repeat(70));
    }

    /// Check if kernel is likely bootable on Pi 4
    pub fn is_bootable_pi4(&self) -> bool {
        // Must have a known format
        if self.format == KernelFormat::Unknown {
            return false;
        }

        // ELF needs special handling
        if self.format == KernelFormat::Elf {
            return false; // Needs U-Boot or conversion
        }

        // Size must be reasonable
        if self.size < 1000 {
            return false;
        }

        true
    }
}

/// Analyze multiple kernel images and find the best for Pi 4
pub fn find_best_kernel(kernels: &[KernelImage]) -> Option<&KernelImage> {
    // Prefer 64-bit ARM64 Image format
    kernels
        .iter()
        .find(|k| k.architecture == Architecture::Arm64 && k.format == KernelFormat::Arm64Image)
        .or_else(|| {
            // Fall back to any 64-bit kernel
            kernels.iter().find(|k| k.architecture == Architecture::Arm64)
        })
        .or_else(|| {
            // Fall back to 32-bit
            kernels.iter().find(|k| k.is_bootable_pi4())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_detection_elf() {
        let header = [0x7f, b'E', b'L', b'F', 2, 1, 1, 0,
                      0, 0, 0, 0, 0, 0, 0, 0,
                      2, 0, 183, 0, 0, 0, 0, 0]; // ARM64 ELF header start

        let format = KernelImage::detect_format(&header);
        assert_eq!(format, KernelFormat::Elf);
    }

    #[test]
    fn test_format_detection_gzip() {
        let header = [0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0];
        let format = KernelImage::detect_format(&header);
        assert_eq!(format, KernelFormat::GzipCompressed);
    }

    #[test]
    fn test_architecture_detection() {
        // ARM64 ELF header
        let mut header = vec![0x7f, b'E', b'L', b'F'];
        header.resize(64, 0);
        header[4] = 2; // 64-bit
        header[18] = 183; // EM_AARCH64
        header[19] = 0;

        let arch = KernelImage::detect_architecture(&header, &KernelFormat::Elf);
        assert_eq!(arch, Architecture::Arm64);
    }
}
