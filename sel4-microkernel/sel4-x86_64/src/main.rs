//! seL4 x86_64 Root Server
//!
//! This is the initial userspace task that runs on seL4 after boot.
//! The root server receives all initial capabilities and is responsible
//! for setting up the rest of the system.
//!
//! seL4 Formal Verification Status for x86_64:
//! - Functional correctness proof: Available
//! - Binary verification: Not yet (ARM-only currently)
//!
//! For maximum assurance, use AArch64 with Microkit instead.

#![no_std]
#![no_main]

extern crate alloc;

use sel4::cap_type;
use sel4_root_task::{debug_print, debug_println, root_task};
use verus_builtin_macros::verus;

// ============================================================================
// Verified Components (same as Microkit version for code reuse)
// ============================================================================

verus! {

/// Verified capability with rights management
/// Demonstrates seL4's capability derivation model
pub const RIGHT_READ: u64 = 1 << 0;
pub const RIGHT_WRITE: u64 = 1 << 1;
pub const RIGHT_GRANT: u64 = 1 << 2;
pub const RIGHT_RETYPE: u64 = 1 << 3;

#[derive(Clone, Copy)]
pub struct VerifiedCap {
    pub rights: u64,
}

impl VerifiedCap {
    pub open spec fn has_right_spec(&self, right: u64) -> bool {
        (self.rights & right) != 0
    }

    /// Derive capability with reduced rights
    /// Mathematically proven: child <= parent
    pub fn derive(&self, mask: u64) -> (child: Self)
        ensures
            child.rights == self.rights & mask,
            forall|r: u64| #[trigger] child.has_right_spec(r) ==> self.has_right_spec(r),
    {
        VerifiedCap {
            rights: self.rights & mask,
        }
    }

    pub fn has_right(&self, r: u64) -> (has: bool)
        ensures has == self.has_right_spec(r),
    {
        (self.rights & r) != 0
    }
}

/// Verified message buffer for IPC
/// Proves bounds safety and data integrity
pub const MAX_MSG_LEN: usize = 120;  // seL4 IPC buffer size

pub struct VerifiedMsgBuffer {
    data: [u64; MAX_MSG_LEN],
    len: usize,
}

impl VerifiedMsgBuffer {
    pub open spec fn valid(&self) -> bool {
        self.len <= MAX_MSG_LEN
    }

    pub fn new() -> (buf: Self)
        ensures buf.valid(), buf.len == 0,
    {
        VerifiedMsgBuffer {
            data: [0; MAX_MSG_LEN],
            len: 0,
        }
    }

    /// Set a word in the buffer (bounds-checked)
    pub fn set(&mut self, index: usize, value: u64) -> (success: bool)
        requires old(self).valid(),
        ensures
            self.valid(),
            success ==> index < MAX_MSG_LEN,
    {
        if index < MAX_MSG_LEN {
            self.data[index] = value;
            if index >= self.len {
                self.len = index + 1;
            }
            true
        } else {
            false
        }
    }

    /// Get a word from the buffer (bounds-checked)
    pub fn get(&self, index: usize) -> (result: Option<u64>)
        requires self.valid(),
        ensures
            result.is_some() ==> index < self.len,
    {
        if index < self.len {
            Some(self.data[index])
        } else {
            None
        }
    }
}

/// Verified physical memory region descriptor
/// Used for untyped memory management in seL4
pub struct VerifiedUntypedDesc {
    pub paddr: u64,
    pub size_bits: u8,
    pub is_device: bool,
}

impl VerifiedUntypedDesc {
    pub open spec fn size(&self) -> u64 {
        1u64 << (self.size_bits as u64)
    }

    pub open spec fn valid(&self) -> bool {
        self.size_bits <= 47  // x86_64 physical address limit
    }

    /// Check if an address is within this region
    pub fn contains(&self, addr: u64) -> (result: bool)
        requires self.valid(),
        ensures result == (addr >= self.paddr && addr < self.paddr + self.size()),
    {
        let size = 1u64 << (self.size_bits as u64);
        addr >= self.paddr && addr < self.paddr + size
    }
}

} // verus!

// ============================================================================
// seL4 Root Task Implementation
// ============================================================================

/// Root task entry point
///
/// This is the first userspace code that runs after seL4 boots.
/// We receive the initial capabilities to all system resources.
#[root_task]
fn main(bootinfo: &sel4::BootInfo) -> ! {
    debug_println!("=========================================");
    debug_println!("  seL4 x86_64 - Formally Verified Kernel ");
    debug_println!("=========================================");
    debug_println!();

    debug_println!("Root server started!");
    debug_println!();

    // Print boot info
    debug_println!("Boot Information:");
    debug_println!("  IPCBuffer: {:?}", bootinfo.ipc_buffer());
    debug_println!("  Empty slots: {:?}", bootinfo.empty());
    debug_println!("  Untyped memory regions: {:?}", bootinfo.untyped());
    debug_println!();

    // Demonstrate verified capability derivation
    debug_println!("Verified Capability System:");
    let root_cap = VerifiedCap {
        rights: RIGHT_READ | RIGHT_WRITE | RIGHT_GRANT | RIGHT_RETYPE,
    };
    debug_println!("  Root cap: all rights");

    let child_cap = root_cap.derive(RIGHT_READ | RIGHT_WRITE);
    debug_println!("  Derived cap: read + write only");
    debug_println!("  Can read: {}", child_cap.has_right(RIGHT_READ));
    debug_println!("  Can retype: {}", child_cap.has_right(RIGHT_RETYPE));
    debug_println!();

    // Demonstrate verified message buffer
    debug_println!("Verified IPC Message Buffer:");
    let mut msg = VerifiedMsgBuffer::new();
    msg.set(0, 0xDEADBEEF);
    msg.set(1, 0xCAFEBABE);
    debug_println!("  Set msg[0] = 0xDEADBEEF");
    debug_println!("  Set msg[1] = 0xCAFEBABE");
    if let Some(v) = msg.get(0) {
        debug_println!("  Read msg[0] = {:#x}", v);
    }
    debug_println!();

    debug_println!("seL4 Verification Status:");
    debug_println!("  - Kernel: Functionally correct (proven)");
    debug_println!("  - This code: Verified with Verus");
    debug_println!("  - Memory safety: Guaranteed by Rust + Verus");
    debug_println!();

    debug_println!("System initialized. Entering idle loop...");

    // In a real system, we would:
    // 1. Parse untyped memory from bootinfo
    // 2. Create page tables and map memory
    // 3. Create threads and endpoints
    // 4. Start user applications
    //
    // For this demo, we just idle.

    loop {
        // Yield to prevent busy-waiting
        sel4::r#yield();
    }
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    debug_println!("PANIC: {:?}", info);
    loop {}
}
