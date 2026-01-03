//! Boot image analysis and debugging module
//!
//! This module provides functionality for:
//! - Analyzing Raspberry Pi boot partition structure
//! - Validating boot configuration files
//! - Checking kernel and device tree compatibility
//! - Detecting common boot issues

pub mod config;
pub mod partition;
pub mod validate;

pub use config::BootConfig;
pub use partition::BootPartition;
pub use validate::BootValidator;
