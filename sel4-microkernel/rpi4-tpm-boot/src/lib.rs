//! # Verified TPM 2.0 Boot Measurement for seL4/Microkit
//!
//! This library provides formally verified boot measurement and attestation
//! for Raspberry Pi 4 with TPM 2.0 modules (specifically GeeekPi TPM9670
//! with Infineon SLB 9670 chip).
//!
//! ## Hardware Support
//!
//! - **GeeekPi TPM9670** (Infineon Optiga SLB 9670 TPM 2.0)
//! - **LetsTrust TPM** (Infineon SLB 9672)
//! - **Any TCG-compliant SPI TPM 2.0**
//!
//! ## Verification Guarantees
//!
//! All boot measurement operations are formally verified using Verus:
//! - PCR indices proven in valid range (0-23)
//! - Measurement chain integrity proven
//! - Hash extension operation correctness
//! - Policy evaluation soundness
//!
//! ## Boot Measurement Chain
//!
//! ```text
//! PCR 0: Firmware (bootcode.bin, start4.elf, fixup4.dat)
//! PCR 1: seL4 Kernel Image
//! PCR 2: Microkit System Configuration
//! PCR 3: Protection Domain Images
//! PCR 4: Runtime Configuration
//! PCR 7: Secure Boot Policy
//! ```

#![no_std]
#![allow(unused)]
#![allow(clippy::assign_op_pattern)]
#![allow(clippy::new_without_default)]

// Conditional Verus support
#[cfg(feature = "verus")]
use verus_builtin_macros::verus;

#[cfg(not(feature = "verus"))]
macro_rules! verus {
    ($($tt:tt)*) => {
        verus_stub!($($tt)*);
    };
}

#[cfg(not(feature = "verus"))]
macro_rules! verus_stub {
    (
        $(#[$attr:meta])*
        $vis:vis const $name:ident : $ty:ty = $val:expr ;
        $($rest:tt)*
    ) => {
        $(#[$attr])*
        $vis const $name: $ty = $val;
        verus_stub!($($rest)*);
    };
    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident { $($field:tt)* }
        $($rest:tt)*
    ) => {
        $(#[$attr])*
        $vis struct $name { $($field)* }
        verus_stub!($($rest)*);
    };
    (
        $(#[$attr:meta])*
        $vis:vis enum $name:ident { $($variant:tt)* }
        $($rest:tt)*
    ) => {
        $(#[$attr])*
        $vis enum $name { $($variant)* }
        verus_stub!($($rest)*);
    };
    (
        impl $type:ty { $($item:tt)* }
        $($rest:tt)*
    ) => {
        verus_stub_impl!($type, $($item)*);
        verus_stub!($($rest)*);
    };
    (
        $vis:vis fn $name:ident $($tt:tt)*
    ) => {
        // Skip standalone functions in stub mode
    };
    (
        $vis:vis open spec fn $name:ident $($tt:tt)*
    ) => {
        // Skip spec functions
    };
    (
        $vis:vis proof fn $name:ident $($tt:tt)*
    ) => {
        // Skip proof functions
    };
    () => {};
}

#[cfg(not(feature = "verus"))]
macro_rules! verus_stub_impl {
    ($type:ty, ) => {
        impl $type {}
    };
    ($type:ty,
        $(#[$attr:meta])*
        $vis:vis fn $name:ident (&self $(, $param:ident : $pty:ty)*) -> ($ret:ident : $rty:ty)
            $(requires $($req:tt)*)?
            $(ensures $($ens:tt)*)?
        { $($body:tt)* }
        $($rest:tt)*
    ) => {
        impl $type {
            $(#[$attr])*
            $vis fn $name(&self $(, $param: $pty)*) -> $rty {
                $($body)*
            }
        }
    };
    ($type:ty,
        $(#[$attr:meta])*
        $vis:vis fn $name:ident (&mut self $(, $param:ident : $pty:ty)*) -> ($ret:ident : $rty:ty)
            $(requires $($req:tt)*)?
            $(ensures $($ens:tt)*)?
        { $($body:tt)* }
        $($rest:tt)*
    ) => {
        impl $type {
            $(#[$attr])*
            $vis fn $name(&mut self $(, $param: $pty)*) -> $rty {
                $($body)*
            }
        }
    };
    ($type:ty,
        $(#[$attr:meta])*
        $vis:vis fn $name:ident ($($param:ident : $pty:ty),*) -> ($ret:ident : $rty:ty)
            $(requires $($req:tt)*)?
            $(ensures $($ens:tt)*)?
        { $($body:tt)* }
        $($rest:tt)*
    ) => {
        impl $type {
            $(#[$attr])*
            $vis fn $name($($param: $pty),*) -> $rty {
                $($body)*
            }
        }
    };
    ($type:ty, $($rest:tt)*) => {
        impl $type {}
    };
}

pub mod slb9670;
pub mod boot_chain;
pub mod pcr;
pub mod attestation;
pub mod spi;

// Re-exports
pub use slb9670::*;
pub use boot_chain::*;
pub use pcr::*;
pub use attestation::*;

// ============================================================================
// CORE TYPES
// ============================================================================

/// SHA-256 digest (32 bytes)
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Sha256Digest {
    pub bytes: [u8; 32],
}

impl Sha256Digest {
    /// Create a new digest from bytes
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Zero digest (used for PCR initial state)
    pub const fn zero() -> Self {
        Self { bytes: [0u8; 32] }
    }

    /// Check if this is the zero digest
    pub fn is_zero(&self) -> bool {
        self.bytes == [0u8; 32]
    }
}

impl core::fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Sha256Digest(")?;
        for byte in &self.bytes[..4] {
            write!(f, "{:02x}", byte)?;
        }
        write!(f, "...")?;
        for byte in &self.bytes[28..] {
            write!(f, "{:02x}", byte)?;
        }
        write!(f, ")")
    }
}

/// TPM response codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TpmRc {
    Success = 0x000,
    Failure = 0x101,
    BadTag = 0x01E,
    BadSequence = 0x003,
    BadParam = 0x004,
    AuthFail = 0x08E,
    Disabled = 0x120,
    Locality = 0x907,
    NvLocked = 0x148,
    Retry = 0x922,
    Unknown = 0xFFFF,
}

impl From<u32> for TpmRc {
    fn from(code: u32) -> Self {
        match code {
            0x000 => TpmRc::Success,
            0x101 => TpmRc::Failure,
            0x01E => TpmRc::BadTag,
            0x003 => TpmRc::BadSequence,
            0x004 => TpmRc::BadParam,
            0x08E => TpmRc::AuthFail,
            0x120 => TpmRc::Disabled,
            0x907 => TpmRc::Locality,
            0x148 => TpmRc::NvLocked,
            0x922 => TpmRc::Retry,
            _ => TpmRc::Unknown,
        }
    }
}

/// Boot stage identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BootStage {
    /// VideoCore firmware (bootcode.bin, start4.elf)
    Firmware = 0,
    /// seL4 kernel image
    Kernel = 1,
    /// Microkit system configuration
    System = 2,
    /// Protection domain images
    ProtectionDomains = 3,
    /// Runtime configuration and state
    Runtime = 4,
    /// Secure boot policy
    SecureBootPolicy = 7,
}

impl BootStage {
    /// Get the PCR index for this boot stage
    pub const fn pcr_index(&self) -> u8 {
        match self {
            BootStage::Firmware => 0,
            BootStage::Kernel => 1,
            BootStage::System => 2,
            BootStage::ProtectionDomains => 3,
            BootStage::Runtime => 4,
            BootStage::SecureBootPolicy => 7,
        }
    }
}

/// Result type for TPM operations
pub type TpmResult<T> = Result<T, TpmRc>;
