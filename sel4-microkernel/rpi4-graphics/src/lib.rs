//! # Raspberry Pi 4 Graphics Library for seL4 Microkit
//!
//! This library provides framebuffer graphics for Raspberry Pi 4 running on seL4.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │         Graphics Protection Domain       │
//! │  ┌─────────────────────────────────┐    │
//! │  │      Verified Framebuffer       │    │
//! │  │   (bounds-checked pixel ops)    │    │
//! │  └──────────────┬──────────────────┘    │
//! │                 │                        │
//! │  ┌──────────────┴──────────────────┐    │
//! │  │      Mailbox Driver             │    │
//! │  │  (VideoCore communication)      │    │
//! │  └──────────────┬──────────────────┘    │
//! │                 │                        │
//! │  ┌──────────────┴──────────────────┐    │
//! │  │      TPM 2.0 Driver             │    │
//! │  │  (ST33KTPM2I3WBZA9 via SPI)     │    │
//! │  │  - Measured boot (PCR extend)   │    │
//! │  │  - Remote attestation           │    │
//! │  └──────────────┬──────────────────┘    │
//! ├─────────────────┼───────────────────────┤
//! │            seL4 Microkernel             │
//! ├─────────────────┼───────────────────────┤
//! │  BCM2711 Hardware (Raspberry Pi 4)      │
//! │  ┌──────────────┴──────────────────┐    │
//! │  │   VideoCore VI  │  ST33K TPM    │    │
//! │  │   GPU/Mailbox   │  (SPI)        │    │
//! │  └─────────────────────────────────┘    │
//! └─────────────────────────────────────────┘
//! ```

#![no_std]
#![allow(dead_code)]

pub mod mailbox;
pub mod framebuffer;
pub mod graphics;
pub mod font;
pub mod tpm;
pub mod crypto;

pub use mailbox::{Mailbox, MailboxError};
pub use framebuffer::{Framebuffer, FramebufferInfo};
pub use graphics::{Color, Point, Rect};
pub use tpm::{Tpm, TpmError};
pub use crypto::{Sha256, Sha256Digest, VerifyResult, constant_time_compare, verify_sha256};

/// BCM2711 peripheral base address (Raspberry Pi 4)
pub const BCM2711_PERIPH_BASE: usize = 0xFE00_0000;

/// Mailbox base address
pub const MAILBOX_BASE: usize = BCM2711_PERIPH_BASE + 0xB880;

/// GPU bus address to ARM physical address translation
/// The GPU sees memory differently than the ARM cores
#[inline]
pub const fn gpu_to_arm(gpu_addr: u32) -> usize {
    // GPU bus address 0xC0000000+ maps to ARM physical 0x00000000+
    // GPU bus address 0x80000000+ maps to ARM physical 0x00000000+ (uncached)
    (gpu_addr & 0x3FFF_FFFF) as usize
}

/// ARM physical address to GPU bus address translation
#[inline]
pub const fn arm_to_gpu(arm_addr: usize) -> u32 {
    // Use uncached alias for DMA coherency
    (arm_addr as u32) | 0xC000_0000
}
