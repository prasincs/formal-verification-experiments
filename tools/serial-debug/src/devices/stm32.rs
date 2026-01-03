//! STM32 device profile
//!
//! Device profile for STM32 microcontrollers serial debugging.

use super::profile::{DeviceProfile, SerialSettings, BootStage, ErrorPattern, BootFileCheck};
use once_cell::sync::Lazy;

/// STM32 device profile
pub static STM32_PROFILE: Lazy<DeviceProfile> = Lazy::new(|| {
    DeviceProfile {
        name: "STM32".to_string(),
        id: "stm32".to_string(),
        description: "STM32 ARM Cortex-M microcontrollers".to_string(),
        manufacturer: "STMicroelectronics".to_string(),
        architecture: "arm-cortex-m".to_string(),
        serial: SerialSettings {
            baud_rate: 115200,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".to_string(),
            flow_control: "none".to_string(),
            alt_baud_rates: vec![9600, 19200, 38400, 57600, 230400, 460800, 921600, 1000000, 2000000],
        },
        boot_stages: vec![
            BootStage {
                name: "Bootloader".to_string(),
                patterns: vec!["STM32 Bootloader".to_string(), "System Bootloader".to_string()],
                description: "STM32 system bootloader".to_string(),
                expected_duration_secs: 1,
            },
            BootStage {
                name: "HAL Init".to_string(),
                patterns: vec!["HAL_Init".to_string(), "SystemClock_Config".to_string()],
                description: "Hardware Abstraction Layer initialization".to_string(),
                expected_duration_secs: 1,
            },
            BootStage {
                name: "RTOS Init".to_string(),
                patterns: vec!["FreeRTOS".to_string(), "osKernelStart".to_string(), "ThreadX".to_string()],
                description: "Real-time OS initialization".to_string(),
                expected_duration_secs: 1,
            },
            BootStage {
                name: "Application".to_string(),
                patterns: vec!["main()".to_string(), "App started".to_string()],
                description: "Application code running".to_string(),
                expected_duration_secs: 0,
            },
        ],
        error_patterns: vec![
            ErrorPattern {
                pattern: "Hard Fault".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Hard fault exception".to_string(),
                suggestion: Some("Check stack overflow, null pointer access, or memory corruption".to_string()),
            },
            ErrorPattern {
                pattern: "HardFault_Handler".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Hard fault handler invoked".to_string(),
                suggestion: Some("Enable fault handlers for detailed diagnostics".to_string()),
            },
            ErrorPattern {
                pattern: "MemManage Fault".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Memory management fault".to_string(),
                suggestion: Some("Check MPU configuration and memory access permissions".to_string()),
            },
            ErrorPattern {
                pattern: "Bus Fault".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Bus fault on memory access".to_string(),
                suggestion: Some("Check peripheral addresses and bus configuration".to_string()),
            },
            ErrorPattern {
                pattern: "Usage Fault".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Usage fault (invalid instruction)".to_string(),
                suggestion: Some("Check for undefined instructions or unaligned access".to_string()),
            },
            ErrorPattern {
                pattern: "assert failed".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Assertion failure".to_string(),
                suggestion: Some("Check assertion conditions in the firmware".to_string()),
            },
            ErrorPattern {
                pattern: "Error_Handler".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "Error handler called".to_string(),
                suggestion: Some("Check HAL initialization and peripheral configuration".to_string()),
            },
            ErrorPattern {
                pattern: "HAL_ERROR".to_string(),
                is_regex: false,
                severity: "error".to_string(),
                description: "HAL function returned error".to_string(),
                suggestion: Some("Check peripheral initialization parameters".to_string()),
            },
            ErrorPattern {
                pattern: "HAL_TIMEOUT".to_string(),
                is_regex: false,
                severity: "warning".to_string(),
                description: "HAL timeout".to_string(),
                suggestion: Some("Check peripheral clock and initialization".to_string()),
            },
        ],
        success_patterns: vec![
            "System initialized".to_string(),
            "Ready".to_string(),
            "OK".to_string(),
        ],
        usb_vendor_ids: vec![
            0x0483, // STMicroelectronics
            0x0403, // FTDI
            0x10c4, // Silicon Labs
        ],
        usb_product_ids: vec![
            0x5740, // STM32 Virtual COM Port
            0x374b, // ST-Link VCP
            0x6001, // FTDI FT232
            0xea60, // CP2102
        ],
        boot_files: vec![], // STM32 doesn't use boot files in the same way
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stm32_profile() {
        let profile = &*STM32_PROFILE;
        assert_eq!(profile.id, "stm32");
        assert_eq!(profile.serial.baud_rate, 115200);
    }
}
