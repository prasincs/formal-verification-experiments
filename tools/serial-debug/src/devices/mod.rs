//! Device profiles for serial debugging
//!
//! This module provides built-in device profiles and support for custom
//! device configurations. Each profile defines serial settings, boot
//! patterns, and error signatures for a specific embedded device.

pub mod profile;
pub mod rpi4;
pub mod stm32;
pub mod esp32;
pub mod generic;

pub use profile::{DeviceProfile, BootStage, ErrorPattern, SerialSettings};
pub use rpi4::RPI4_PROFILE;
pub use stm32::STM32_PROFILE;
pub use esp32::ESP32_PROFILE;
pub use generic::GENERIC_PROFILE;

use std::collections::HashMap;
use once_cell::sync::Lazy;

/// Registry of built-in device profiles
pub static DEVICE_PROFILES: Lazy<HashMap<&'static str, &'static DeviceProfile>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("rpi4", &*RPI4_PROFILE);
    m.insert("raspberry-pi-4", &*RPI4_PROFILE);
    m.insert("stm32", &*STM32_PROFILE);
    m.insert("stm32f4", &*STM32_PROFILE);
    m.insert("esp32", &*ESP32_PROFILE);
    m.insert("esp32-wroom", &*ESP32_PROFILE);
    m.insert("generic", &*GENERIC_PROFILE);
    m.insert("default", &*GENERIC_PROFILE);
    m
});

/// Get a device profile by name
pub fn get_profile(name: &str) -> Option<&'static DeviceProfile> {
    DEVICE_PROFILES.get(name.to_lowercase().as_str()).copied()
}

/// List all available device profiles
pub fn list_profiles() -> Vec<(&'static str, &'static DeviceProfile)> {
    DEVICE_PROFILES
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect()
}

/// Get profile names only (deduplicated)
pub fn profile_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = vec!["rpi4", "stm32", "esp32", "generic"];
    names.sort();
    names
}
