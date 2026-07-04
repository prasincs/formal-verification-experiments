use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const WORK_RING_VADDR: usize = 0x5_0400_0000;
pub const WORK_RING_CAPACITY: u32 = 16;
pub const SUPERVISOR_CHANNEL_ID: usize = 1;

pub const COMMAND_NONE: u32 = 0;
pub const COMMAND_POISON: u32 = 0x504f_4953;
pub const COMMAND_WATCHDOG_STALL: u32 = 0x5744_4f47;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OddGeneration(pub u32);

/// IC-1 header followed by demo control words. The header offsets remain the
/// canonical `write_idx`, `read_idx`, `capacity`, `generation` layout.
#[repr(C, align(64))]
pub struct WorkRing {
    pub write_idx: AtomicU32,
    pub read_idx: AtomicU32,
    pub capacity: u32,
    pub generation: AtomicU32,
    pub heartbeat: AtomicU32,
    pub command: AtomicU32,
    pub command_sequence: AtomicU32,
    pub reserved: AtomicU32,
    /// Linked address of the worker-local stack-reset trampoline. The worker
    /// publishes it on every boot; the supervisor captures it before reset.
    pub restart_entry: AtomicU64,
    pub entries: [AtomicU32; WORK_RING_CAPACITY as usize],
}

impl WorkRing {
    /// # Safety
    /// The fixed virtual address must be mapped to the `work_ring` region and
    /// the caller must not create a mutable alias.
    pub unsafe fn mapped_mut() -> &'static mut Self {
        &mut *(WORK_RING_VADDR as *mut Self)
    }

    /// # Safety
    /// The fixed virtual address must be mapped to the `work_ring` region.
    pub unsafe fn mapped() -> &'static Self {
        &*(WORK_RING_VADDR as *const Self)
    }

    /// Called by the supervisor before the child can run.
    pub fn initialize(&mut self) {
        self.write_idx.store(0, Ordering::Relaxed);
        self.read_idx.store(0, Ordering::Relaxed);
        self.capacity = WORK_RING_CAPACITY;
        self.generation.store(0, Ordering::Release);
        self.heartbeat.store(0, Ordering::Release);
        self.command.store(COMMAND_NONE, Ordering::Release);
        self.command_sequence.store(0, Ordering::Release);
        self.reserved.store(0, Ordering::Release);
        self.restart_entry.store(0, Ordering::Release);
        for entry in &self.entries {
            entry.store(0, Ordering::Relaxed);
        }
    }

    pub fn resync(&self) -> Result<u32, OddGeneration> {
        let generation = self.generation.load(Ordering::Acquire);
        if generation & 1 != 0 {
            return Err(OddGeneration(generation));
        }
        Ok(generation)
    }

    pub fn boot_generation(&self) -> Result<u32, OddGeneration> {
        Ok(self.resync()? / 2 + 1)
    }

    pub fn publish_heartbeat(&self) -> u32 {
        self.heartbeat.fetch_add(1, Ordering::Release) + 1
    }

    pub fn heartbeat(&self) -> u32 {
        self.heartbeat.load(Ordering::Acquire)
    }

    pub fn set_command(&self, command: u32) -> u32 {
        self.command.store(command, Ordering::Release);
        self.command_sequence.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn command(&self) -> u32 {
        self.command.load(Ordering::Acquire)
    }

    pub fn publish_restart_entry(&self, entry: u64) {
        self.restart_entry.store(entry, Ordering::Release);
    }

    pub fn restart_entry(&self) -> u64 {
        self.restart_entry.load(Ordering::Acquire)
    }
}

const _: () = assert!(core::mem::offset_of!(WorkRing, generation) == 0x0c);
const _: () = assert!(core::mem::size_of::<WorkRing>() <= 0x1000);
