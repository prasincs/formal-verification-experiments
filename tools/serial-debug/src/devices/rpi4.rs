//! Raspberry Pi 4 device profile
//!
//! Complete device profile for Raspberry Pi 4 serial debugging,
//! including boot stages, error patterns, and boot file validation.

use super::profile::{DeviceProfile, SerialSettings, BootStage, ErrorPattern, BootFileCheck};
use once_cell::sync::Lazy;

/// Raspberry Pi 4 device profile
pub static RPI4_PROFILE: Lazy<DeviceProfile> = Lazy::new(|| {
    let mut profile = DeviceProfile {
        name: "Raspberry Pi 4".to_string(),
        id: "rpi4".to_string(),
        description: "Raspberry Pi 4 Model B (BCM2711)".to_string(),
        manufacturer: "Raspberry Pi Foundation".to_string(),
        architecture: "aarch64".to_string(),
        serial: SerialSettings {
            baud_rate: 115200,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".to_string(),
            flow_control: "none".to_string(),
            alt_baud_rates: vec![9600, 19200, 38400, 57600, 230400, 460800, 921600, 1000000],
        },
        boot_stages: vec![
            BootStage {
                name: "GPU Firmware".to_string(),
                patterns: vec!["Raspberry Pi".to_string(), "bootcode.bin".to_string()],
                description: "GPU firmware initialization".to_string(),
                expected_duration_secs: 2,
            },
            BootStage {
                name: "Start.elf".to_string(),
                patterns: vec!["start".to_string(), "start4.elf".to_string()],
                description: "Second stage bootloader".to_string(),
                expected_duration_secs: 1,
            },
            BootStage {
                name: "U-Boot".to_string(),
                patterns: vec!["U-Boot".to_string(), "u-boot".to_string()],
                description: "U-Boot bootloader".to_string(),
                expected_duration_secs: 3,
            },
            BootStage {
                name: "Linux Kernel".to_string(),
                patterns: vec!["Linux version".to_string(), "Booting Linux".to_string()],
                description: "Linux kernel initialization".to_string(),
                expected_duration_secs: 5,
            },
            BootStage {
                name: "Kernel Init".to_string(),
                patterns: vec!["Run /init".to_string(), "systemd".to_string()],
                description: "Kernel init process".to_string(),
                expected_duration_secs: 10,
            },
            BootStage {
                name: "seL4 Kernel".to_string(),
                patterns: vec!["seL4".to_string(), "seL4 Microkit".to_string()],
                description: "seL4 microkernel boot".to_string(),
                expected_duration_secs: 1,
            },
            BootStage {
                name: "seL4 Microkit".to_string(),
                patterns: vec!["microkit".to_string(), "MON|".to_string()],
                description: "seL4 Microkit monitor".to_string(),
                expected_duration_secs: 1,
            },
            BootStage {
                name: "Login Prompt".to_string(),
                patterns: vec!["login:".to_string()],
                description: "System ready for login".to_string(),
                expected_duration_secs: 0,
            },
        ],
        error_patterns: vec![
            // SD Card errors
            ErrorPattern {
                pattern: "mmc0: error".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "SD card read error".to_string(),
                suggestion: Some("Check SD card connection or try a different card".to_string()),
            },
            ErrorPattern {
                pattern: "mmc0: timeout".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "SD card timeout".to_string(),
                suggestion: Some("SD card may be corrupted or incompatible".to_string()),
            },
            // Kernel panics
            ErrorPattern {
                pattern: "kernel panic".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Kernel panic".to_string(),
                suggestion: Some("Check kernel image and device tree compatibility".to_string()),
            },
            ErrorPattern {
                pattern: "Kernel panic - not syncing".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Fatal kernel panic".to_string(),
                suggestion: Some("Check kernel command line and root filesystem".to_string()),
            },
            // Filesystem errors
            ErrorPattern {
                pattern: "VFS: Cannot open root device".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Root filesystem not found".to_string(),
                suggestion: Some("Check boot config root= parameter and partition table".to_string()),
            },
            ErrorPattern {
                pattern: "Kernel image is corrupt".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Corrupted kernel image".to_string(),
                suggestion: Some("Re-flash the SD card with a fresh image".to_string()),
            },
            // Boot file errors
            ErrorPattern {
                pattern: "start.elf not found".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Boot files missing".to_string(),
                suggestion: Some("Copy boot files from Raspberry Pi firmware repository".to_string()),
            },
            ErrorPattern {
                pattern: "fixup.dat not found".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "GPU memory config missing".to_string(),
                suggestion: Some("Copy fixup4.dat to boot partition".to_string()),
            },
            ErrorPattern {
                pattern: "device tree not found".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Device tree blob missing".to_string(),
                suggestion: Some("Copy bcm2711-rpi-4-b.dtb to boot partition".to_string()),
            },
            // Memory errors
            ErrorPattern {
                pattern: "RAMDISK: incomplete write".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Initramfs load error".to_string(),
                suggestion: Some("Check initrd image or increase memory allocation".to_string()),
            },
            ErrorPattern {
                pattern: "Unable to handle kernel paging".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Memory management fault".to_string(),
                suggestion: Some("Check kernel and device tree memory settings".to_string()),
            },
            // seL4 specific errors
            ErrorPattern {
                pattern: "seL4: cap fault".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "seL4 capability fault".to_string(),
                suggestion: Some("Check seL4 system configuration and capability setup".to_string()),
            },
            ErrorPattern {
                pattern: "seL4: vaddr".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "seL4 virtual address error".to_string(),
                suggestion: Some("Check memory mappings in system description".to_string()),
            },
            ErrorPattern {
                pattern: "seL4: invalid invocation".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "seL4 invalid syscall".to_string(),
                suggestion: Some("Check IPC setup between protection domains".to_string()),
            },
            // Generic errors
            ErrorPattern {
                pattern: "error".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "Error detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "fail".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "Failure detected".to_string(),
                suggestion: None,
            },
            ErrorPattern {
                pattern: "timeout".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "Timeout occurred".to_string(),
                suggestion: None,
            },
        ],
        success_patterns: vec![
            "login:".to_string(),
            "Welcome to".to_string(),
            "System ready".to_string(),
            "Boot successful".to_string(),
        ],
        usb_vendor_ids: vec![
            0x0403, // FTDI
            0x10c4, // Silicon Labs CP210x
            0x1a86, // WCH CH340
            0x067b, // Prolific PL2303
        ],
        usb_product_ids: vec![
            0x6001, // FTDI FT232
            0xea60, // CP2102
            0x7523, // CH340
            0x2303, // PL2303
        ],
        boot_files: vec![
            BootFileCheck {
                name: "config.txt".to_string(),
                required: true,
                description: "Boot configuration file".to_string(),
            },
            BootFileCheck {
                name: "cmdline.txt".to_string(),
                required: true,
                description: "Kernel command line".to_string(),
            },
            BootFileCheck {
                name: "start4.elf".to_string(),
                required: true,
                description: "GPU firmware for Pi 4".to_string(),
            },
            BootFileCheck {
                name: "fixup4.dat".to_string(),
                required: true,
                description: "GPU memory split config".to_string(),
            },
            BootFileCheck {
                name: "kernel8.img".to_string(),
                required: false,
                description: "64-bit Linux kernel".to_string(),
            },
            BootFileCheck {
                name: "bcm2711-rpi-4-b.dtb".to_string(),
                required: false,
                description: "Device tree for Pi 4B".to_string(),
            },
            BootFileCheck {
                name: "overlays".to_string(),
                required: false,
                description: "Device tree overlays directory".to_string(),
            },
        ],
    };

    profile
});

/// Generate debug-friendly config.txt for RPi4
pub fn generate_debug_config() -> String {
    r#"# Raspberry Pi 4 Debug Configuration
# Generated by serial-debug tool

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

/// Generate debug-friendly cmdline.txt for RPi4
pub fn generate_debug_cmdline() -> String {
    "console=serial0,115200 console=tty1 earlyprintk loglevel=7".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpi4_profile() {
        let profile = &*RPI4_PROFILE;
        assert_eq!(profile.id, "rpi4");
        assert_eq!(profile.serial.baud_rate, 115200);
        assert!(!profile.boot_stages.is_empty());
        assert!(!profile.error_patterns.is_empty());
    }

    #[test]
    fn test_boot_stage_detection() {
        let profile = &*RPI4_PROFILE;

        let stage = profile.match_boot_stage("Linux version 5.15.0");
        assert!(stage.is_some());
        assert_eq!(stage.unwrap().name, "Linux Kernel");
    }

    #[test]
    fn test_error_pattern_detection() {
        let profile = &*RPI4_PROFILE;

        let error = profile.match_error("kernel panic - not syncing");
        assert!(error.is_some());
        assert_eq!(error.unwrap().severity, "error");
    }
}
